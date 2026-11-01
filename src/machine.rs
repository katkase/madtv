//! La macchina emulata: CPU 68000 + bus + AmigaOS HLE.
//!
//! Prepara l'ambiente che AmigaDOS creerebbe per un programma lanciato dalla
//! Shell (processo, CLI, librerie, seglist), carica l'eseguibile del gioco
//! (ancora compresso con Imploder: il decompressore originale gira
//! nell'emulatore, esattamente come su un Amiga vero) e lo esegue.

use crate::bus::{Bus, RAM_SIZE};
use crate::custom::{CYCLES_PER_LINE, LINES_PER_FRAME};
use crate::hunk;
use crate::os::{self, Ctx, LibId, Os, Outcome, dos, exec};
use m68k::{AddressBus, CpuCore, CpuType, StepResult};

/// Pila del supervisore (interrupt ed eccezioni).
const SSP_TOP: u32 = 0x2000;
/// Inizio e fine della memoria gestita da AllocMem.
const HEAP_START: u32 = 0x2000;
const HEAP_END: u32 = RAM_SIZE as u32 - 0x1000;
const USER_STACK: u32 = 64 * 1024;

pub const FRAME_CYCLES: u64 = CYCLES_PER_LINE * LINES_PER_FRAME;

/// Variabili del gioco che descrivono il mouse (vedi `Machine::game_mouse`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameMouse {
    /// Posizione del puntatore.
    pub pos: (i32, i32),
    /// Movimento ricevuto e non ancora applicato al puntatore.
    pub pending: (i32, i32),
    /// Limiti del puntatore nella schermata corrente.
    pub min: (i32, i32),
    pub max: (i32, i32),
}

#[derive(Debug)]
pub enum StopReason {
    Exited(i32),
    Unimplemented(String),
    Exception(String),
    InstructionLimit,
    FrameLimit,
    CpuHalted,
}

pub struct Machine {
    pub cpu: CpuCore,
    pub bus: Bus,
    pub os: Os,
    pub instructions: u64,
    pub entry: u32,
    pub seglist: u32,
    /// Segmenti caricati: (indirizzo dei dati, dimensione, tipo).
    pub segments: Vec<(u32, u32, &'static str)>,
    /// Velocità della CPU in percentuale rispetto a un A500 (100 = 7,09 MHz).
    /// Valori più alti fanno eseguire più istruzioni per frame: il gioco
    /// calcola e disegna più in fretta, ma il suo ritmo (scandito dagli
    /// interrupt di frame) non cambia.
    pub cpu_speed: u32,
    cycle_acc: u64,
    /// Turbo automatico: la CPU va a `turbo_speed` dopo una lettura dal disco,
    /// finché il gioco non torna a sincronizzarsi sul contatore dei frame
    /// (chiamata della sua funzione a `pacing_pc`). Così caricamenti e
    /// decompressioni sono rapidi, mentre le animazioni (alcune temporizzate
    /// dalla CPU) restano alla velocità originale.
    pub auto_turbo: bool,
    pub turbo_speed: u32,
    pub turbo_active: bool,
    last_disk_activity: u64,
    /// Indirizzo della funzione del gioco che legge il contatore dei frame.
    pub pacing_pc: u32,
    /// Statistiche: frame passati in turbo, chiamate al contatore dei frame.
    pub turbo_frames: u64,
    pub pacing_calls: u64,
    /// Ultimi PC eseguiti (diagnostica in caso di errore).
    pub pc_history: [u32; 64],
    pub pc_hist_pos: usize,
}

impl Machine {
    /// Ultimi PC eseguiti, dal più vecchio al più recente.
    pub fn recent_pcs(&self) -> Vec<u32> {
        (0..64)
            .map(|i| self.pc_history[(self.pc_hist_pos + i) & 63])
            .collect()
    }

    pub fn new(vfs: os::vfs::Vfs) -> Machine {
        let mut cpu = CpuCore::new();
        cpu.set_cpu_type(CpuType::M68000);
        Machine {
            cpu,
            bus: Bus::new(),
            os: Os::new(vfs, HEAP_START, HEAP_END),
            instructions: 0,
            entry: 0,
            seglist: 0,
            segments: Vec::new(),
            cpu_speed: 100,
            cycle_acc: 0,
            auto_turbo: false,
            turbo_speed: 400,
            turbo_active: false,
            last_disk_activity: 0,
            pacing_pc: 0,
            turbo_frames: 0,
            pacing_calls: 0,
            pc_history: [0; 64],
            pc_hist_pos: 0,
        }
    }

    /// Costruisce librerie, vettori di eccezione e processo.
    pub fn boot_os(&mut self) {
        let bus = &mut self.bus;
        let os = &mut self.os;

        // Stub per le eccezioni inattese: un opcode A-line per vettore.
        let exc_stubs = os.alloc_clear(bus, 256 * 2);
        for v in 2..256u32 {
            bus.poke16(
                exc_stubs + v * 2,
                os::trap_opcode(LibId::Internal, os::TRAP_EXC_BASE + v as u16),
            );
            bus.poke32(v * 4, exc_stubs + v * 2);
        }

        // Dispatcher degli interrupt di livello 3 (VERTB/COPER/BLIT), in codice 68000:
        //   movem.l d0-d7/a0-a6,-(a7)
        // loop: <trap INT_NEXT>        ; a1=is_Data, a5=is_Code, Z=1 se finiti
        //   beq.s done
        //   jsr (a5)
        //   bra.s loop
        //   nop
        // done: <trap INT_END>
        //   movem.l (a7)+,d0-d7/a0-a6
        //   rte
        let s = os.alloc_clear(bus, 32);
        let code: [u16; 11] = [
            0x48E7,
            0xFFFE,
            os::trap_opcode(LibId::Internal, os::TRAP_INT_NEXT),
            0x6706,
            0x4E95,
            0x60F8,
            0x4E71,
            os::trap_opcode(LibId::Internal, os::TRAP_INT_END),
            0x4CDF,
            0x7FFF,
            0x4E73,
        ];
        for (i, w) in code.iter().enumerate() {
            bus.poke16(s + i as u32 * 2, *w);
        }
        os.int_stub = s;
        bus.poke32(0x6C, s); // vettore di livello 3

        // Gli altri livelli di autovettore: semplice RTE (finché il gioco non installa i propri).
        let rte = os.alloc_clear(bus, 4);
        bus.poke16(rte, 0x4E73);
        for v in [25u32, 26, 28, 29, 30] {
            bus.poke32(v * 4, rte);
        }

        let exit = os.alloc_clear(bus, 4);
        bus.poke16(exit, os::trap_opcode(LibId::Internal, os::TRAP_EXIT));
        os.exit_stub = exit;

        // Librerie.
        let eb = os.make_library(bus, LibId::Exec, 100, exec::EB_POSSIZE, 34);
        os.exec_base = eb;
        bus.poke32(4, eb);
        bus.poke16(eb + exec::EB_ATTNFLAGS, 0); // 68000, nessuna FPU
        bus.poke8(eb + exec::EB_VBLANKFREQ, 50);
        bus.poke8(eb + exec::EB_POWERFREQ, 50);
        os.make_library(bus, LibId::Dos, 40, 0x40, 34);
        os.make_library(bus, LibId::Intuition, 70, 0x100, 34);
        let gfx = os.make_library(bus, LibId::Graphics, 110, 0x200, 34);
        bus.poke16(gfx + 0xCE, 4); // DisplayFlags = PAL
        os.make_library(bus, LibId::Icon, 16, 0x30, 34);

        // Processo (struct Process, 188 byte) con la sua struct CommandLineInterface.
        let pr = os.alloc_clear(bus, 188);
        os.process = pr;
        let pname = os.alloc_cstr(bus, "mtv");
        bus.poke8(pr + 8, 13); // NT_PROCESS
        bus.poke32(pr + 10, pname);
        bus.poke16(pr + 18, 0xFFFF); // tc_SigAlloc: segnali di sistema occupati
        exec::new_list(bus, pr + 92 + 20); // pr_MsgPort.mp_MsgList
        bus.poke8(pr + 92 + 8, 4); // NT_MSGPORT
        bus.poke32(eb + exec::EB_THISTASK, pr);
    }

    /// Carica un eseguibile hunk come farebbe LoadSeg e prepara l'avvio.
    pub fn load_executable(&mut self, data: &[u8], args: &str) -> Result<(), hunk::HunkError> {
        let segs = hunk::parse(data)?;
        let bus = &mut self.bus;
        let os = &mut self.os;
        // Ogni segmento: [lunghezza totale][BPTR al successivo][dati...]
        let mut blocks = Vec::new();
        for s in &segs {
            let size = s.alloc_size.max(s.data.len() as u32) + 8;
            let a = os.alloc_clear(bus, size);
            bus.poke32(a, size);
            bus.write_bytes(a + 8, &s.data);
            blocks.push(a);
            self.segments.push((a + 8, s.alloc_size, s.kind_name()));
        }
        for (i, &a) in blocks.iter().enumerate() {
            let next = blocks.get(i + 1).map(|n| (n + 4) >> 2).unwrap_or(0);
            bus.poke32(a + 4, next);
            for &(target, off) in &segs[i].relocs {
                let p = a + 8 + off;
                let v = bus.peek32(p).wrapping_add(blocks[target] + 8);
                bus.poke32(p, v);
            }
        }
        self.seglist = (blocks[0] + 4) >> 2;
        // Funzione del gioco che restituisce il contatore dei frame (offset nel
        // codice decompresso, segmento 1): vedi NOTES.md, "Velocità".
        if blocks.len() > 1 {
            self.pacing_pc = blocks[1] + 8 + 0x40364;
        }
        self.entry = blocks[0] + 8;

        // CLI: nome del comando e seglist.
        let pr = os.process;
        let cli = os.alloc_clear(bus, 64);
        let cmd = os.alloc_clear(bus, 8);
        bus.poke8(cmd, 3);
        bus.write_bytes(cmd + 1, b"mtv");
        bus.poke32(cli + 16, cmd >> 2); // cli_CommandName
        bus.poke32(cli + 52, USER_STACK / 4); // cli_DefaultStack (in longword)
        bus.poke32(cli + 60, self.seglist); // cli_Module
        bus.poke32(pr + 172, cli >> 2); // pr_CLI
        bus.poke32(pr + 128, self.seglist); // pr_SegList (semplificato)
        bus.poke32(pr + 132, USER_STACK); // pr_StackSize

        // Console e directory corrente (radice del disco 1).
        let mut ctx = Ctx {
            cpu: &mut self.cpu,
            bus,
        };
        os.stdin_fh = dos::new_handle(os, &mut ctx, dos::console_resolved(), Vec::new(), false);
        os.stdout_fh = dos::new_handle(os, &mut ctx, dos::console_resolved(), Vec::new(), true);
        let root = dos::new_lock(
            os,
            &mut ctx,
            os::vfs::Resolved {
                vol: 0,
                path: String::new(),
            },
        );
        let bus = ctx.bus;
        bus.poke32(pr + 156, os.stdin_fh); // pr_CIS
        bus.poke32(pr + 160, os.stdout_fh); // pr_COS
        bus.poke32(pr + 152, root); // pr_CurrentDir
        bus.poke32(cli + 28, os.stdin_fh);
        bus.poke32(cli + 56, os.stdout_fh);

        // Pila utente: [indirizzo di ritorno → uscita][dimensione della pila]
        let stack = os.alloc_clear(bus, USER_STACK);
        let top = stack + USER_STACK - 8;
        bus.poke32(top, os.exit_stub);
        bus.poke32(top + 4, USER_STACK);
        bus.poke32(pr + 58, stack); // tc_SPLower
        bus.poke32(pr + 62, stack + USER_STACK); // tc_SPUpper

        // Argomenti come li passa la Shell: a0 = stringa terminata da '\n', d0 = lunghezza.
        let argline = format!("{args}\n");
        let argp = os.alloc_cstr(bus, &argline);
        os.args = args.to_string();

        // Registri iniziali: modo supervisore per impostare SSP, poi modo utente.
        self.cpu.set_sr(0x2700);
        self.cpu.set_sp(SSP_TOP);
        self.cpu.set_sr(0x0000);
        self.cpu.set_sp(top);
        self.cpu.set_d(0, argline.len() as u32);
        self.cpu.set_a(0, argp);
        {
            self.cpu.pc = self.entry;
            self.cpu.invalidate_prefetch();
        }
        Ok(())
    }

    fn irq_level(&self) -> u8 {
        let c = &self.bus.custom;
        if c.intena & 0x4000 == 0 {
            return 0;
        }
        let p = c.intena & c.intreq & 0x3FFF;
        if p == 0 {
            return 0;
        }
        let hi = 15 - p.leading_zeros() as u16; // bit più alto attivo
        match hi {
            0..=2 => 1,
            3 => 2,
            4..=6 => 3,
            7..=10 => 4,
            11..=12 => 5,
            _ => 6,
        }
    }

    /// Stato del mouse secondo il gioco, letto dalle sue variabili nel
    /// segmento dati (segmento 2 della seglist). Vedi NOTES.md, "Mouse".
    pub fn game_mouse(&self) -> Option<GameMouse> {
        let (base, size, _) = *self.segments.get(2)?;
        if size < 0x2C32 {
            return None;
        }
        let w = |off: u32| self.bus.peek16(base + off) as i16 as i32;
        let pos = (w(0x40), w(0x42));
        if !(0..=400).contains(&pos.0) || !(0..=300).contains(&pos.1) {
            return None;
        }
        Some(GameMouse {
            pos,
            // Movimento accumulato dall'interrupt e non ancora applicato dal ciclo principale.
            pending: (w(0x2C2E), w(0x2C30)),
            min: (w(0x2BD2), w(0x2BD4)),
            max: (w(0x2BD6), w(0x2BD8)),
        })
    }

    /// Posizione del puntatore del mouse secondo il gioco (pixel dello schermo).
    pub fn game_pointer(&self) -> Option<(i32, i32)> {
        self.game_mouse().map(|g| g.pos)
    }

    /// Esegue un frame completo (fino al prossimo vertical blank).
    /// Restituisce None se il frame è terminato normalmente.
    pub fn run_frame(&mut self) -> Option<StopReason> {
        if self.auto_turbo && self.turbo_active {
            self.turbo_frames += 1;
        }
        let target = self.bus.custom.frame + 1;
        match self.run(u64::MAX, target) {
            StopReason::FrameLimit => None,
            other => Some(other),
        }
    }

    /// Esegue fino a un evento di arresto.
    pub fn run(&mut self, max_instructions: u64, max_frames: u64) -> StopReason {
        loop {
            if self.instructions >= max_instructions {
                return StopReason::InstructionLimit;
            }
            if self.bus.custom.frame >= max_frames {
                return StopReason::FrameLimit;
            }
            // Come la linea IPL dell'hardware: il livello va ripresentato a ogni
            // istruzione (il core lo azzera dopo aver servito l'interrupt).
            let lvl = self.irq_level();
            self.cpu.set_irq(lvl);
            self.instructions += 1;
            if self.cpu.pc == self.pacing_pc {
                self.pacing_calls += 1;
                self.turbo_active = false;
            }
            if self.os.disk_activity != self.last_disk_activity {
                self.last_disk_activity = self.os.disk_activity;
                self.turbo_active = true;
            }
            self.pc_history[self.pc_hist_pos & 63] = self.cpu.pc;
            self.pc_hist_pos += 1;
            let r = self.cpu.step(&mut self.bus);
            let cycles = match r {
                StepResult::Ok { cycles } => cycles.max(4) as u64,
                StepResult::AlineTrap { opcode } => {
                    let pc = self.cpu.ppc;
                    let mut ctx = Ctx {
                        cpu: &mut self.cpu,
                        bus: &mut self.bus,
                    };
                    match self.os.handle_trap(&mut ctx, opcode, pc) {
                        Outcome::Return => {
                            let sp = self.cpu.a(7);
                            {
                                self.cpu.pc = self.bus.read_long(sp);
                                self.cpu.invalidate_prefetch();
                            }
                            self.cpu.set_a(7, sp + 4);
                        }
                        Outcome::Continue => {
                            self.cpu.pc = pc + 2;
                            self.cpu.invalidate_prefetch();
                        }
                        Outcome::Jumped => {}
                        Outcome::Exit(code) => return StopReason::Exited(code),
                        Outcome::Unimplemented => {
                            if let Some(v) = self.os.exception {
                                // Il quadro dell'eccezione è sulla pila del supervisore.
                                let sp = self.cpu.a(7);
                                let fault_pc = if v == 2 || v == 3 {
                                    self.bus.peek32(sp + 10)
                                } else {
                                    self.bus.peek32(sp + 2)
                                };
                                let mut d = exception_desc(v, fault_pc);
                                if v == 2 || v == 3 {
                                    d += &format!(
                                        " (indirizzo di accesso ${:06x})",
                                        self.bus.peek32(sp + 2)
                                    );
                                }
                                return StopReason::Exception(d);
                            }
                            let desc = match self.os.missing {
                                Some((lib, idx)) => format!(
                                    "{}: {} (LVO {}) chiamata da ${:06x}",
                                    lib.lib_name(),
                                    lib.func_name(idx as usize),
                                    -(30 + 6 * idx as i32),
                                    self.bus.peek32(self.cpu.a(7))
                                ),
                                None => format!("trap sconosciuta ${opcode:04x} a ${pc:06x}"),
                            };
                            return StopReason::Unimplemented(desc);
                        }
                    }
                    40
                }
                StepResult::Stopped => {
                    // STOP: avanza fino al prossimo frame (o al prossimo interrupt).

                    FRAME_CYCLES - self.bus.custom.cycles % FRAME_CYCLES
                }
                // TRAP, F-line, BKPT, ILLEGAL: come sull'hardware, la CPU prende
                // l'eccezione e salta al vettore. Le intro lo usano per passare in
                // modo supervisore (TRAP #0 con il vettore puntato sul proprio codice);
                // se il vettore è ancora il nostro, la trap interna segnala l'errore.
                StepResult::TrapInstruction { trap_num } => {
                    self.cpu.take_trap_exception(&mut self.bus, trap_num).max(4) as u64
                }
                StepResult::FlineTrap { .. } => {
                    self.cpu.take_fline_exception(&mut self.bus).max(4) as u64
                }
                StepResult::IllegalInstruction { .. } => {
                    self.cpu.take_illegal_exception(&mut self.bus).max(4) as u64
                }
                other => {
                    return StopReason::Exception(format!("{other:?} a ${:06x}", self.cpu.ppc));
                }
            };
            if self.cpu.is_halted() {
                return StopReason::CpuHalted;
            }
            let mut extra = 0u64;
            if self.os.pending_frames > 0 || self.os.pending_delay > 0 {
                let frames = self.os.pending_frames.max(self.os.pending_delay);
                self.os.pending_frames = 0;
                self.os.pending_delay = 0;
                // Salta al prossimo vertical blank (WaitTOF) o di `frames` frame (Delay).
                let to_next = FRAME_CYCLES - self.bus.custom.cycles % FRAME_CYCLES;
                extra = to_next + (frames - 1) * FRAME_CYCLES;
            }
            // Con una CPU più veloce, ogni istruzione consuma meno tempo del chipset.
            let speed = if self.auto_turbo && self.turbo_active {
                self.turbo_speed.max(self.cpu_speed)
            } else {
                self.cpu_speed
            }
            .max(1) as u64;
            self.cycle_acc += cycles * 100;
            let chip_cycles = self.cycle_acc / speed;
            self.cycle_acc %= speed;
            self.bus
                .custom
                .advance(chip_cycles + extra, &mut self.bus.ram);
        }
    }
}

fn exception_desc(v: u8, pc: u32) -> String {
    let name = match v {
        2 => "bus error",
        3 => "address error",
        4 => "istruzione illegale",
        5 => "divisione per zero",
        6 => "CHK",
        7 => "TRAPV",
        8 => "violazione di privilegio",
        9 => "trace",
        10 => "line-A",
        11 => "line-F",
        24 => "interrupt spurio",
        25..=31 => "interrupt (autovettore)",
        32..=47 => "TRAP #n",
        _ => "eccezione",
    };
    format!("eccezione inattesa: vettore {v} ({name}) a ${pc:06x}")
}
