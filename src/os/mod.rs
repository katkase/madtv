//! AmigaOS emulato ad alto livello (HLE).
//!
//! Il gioco chiama le librerie di sistema con `jsr -offset(a6)`, dove a6 è la
//! base della libreria. Qui costruiamo in memoria delle librerie "finte": ogni
//! voce della tabella di salto (6 byte) contiene un opcode A-line
//! `$A000 | (libreria << 9) | indice_funzione`. Sul 68000 gli opcode `$Axxx`
//! non esistono e provocano un'eccezione, che il core `m68k` ci segnala come
//! `StepResult::AlineTrap`: a quel punto eseguiamo la funzione in Rust,
//! mettiamo il risultato in d0 ed emuliamo l'`rts`.
//!
//! I numeri di libreria 0..=5 sono quelli di [`LibId`]; il 7 è riservato alle
//! trap interne (dispatcher degli interrupt, uscita del programma).

pub mod dos;
pub mod exec;
pub mod graphics;
pub mod intuition;
pub mod names;
pub mod vfs;

use crate::bus::Bus;
use m68k::CpuCore;
use std::collections::{BTreeMap, HashMap};
use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LibId {
    Exec = 0,
    Dos = 1,
    Intuition = 2,
    Graphics = 3,
    Icon = 4,
    Internal = 7,
}

impl LibId {
    pub fn from_num(n: u16) -> Option<LibId> {
        Some(match n {
            0 => LibId::Exec,
            1 => LibId::Dos,
            2 => LibId::Intuition,
            3 => LibId::Graphics,
            4 => LibId::Icon,
            7 => LibId::Internal,
            _ => return None,
        })
    }
    pub fn lib_name(self) -> &'static str {
        match self {
            LibId::Exec => "exec.library",
            LibId::Dos => "dos.library",
            LibId::Intuition => "intuition.library",
            LibId::Graphics => "graphics.library",
            LibId::Icon => "icon.library",
            LibId::Internal => "(interno)",
        }
    }
    pub fn func_name(self, idx: usize) -> String {
        let t = match self {
            LibId::Exec => names::EXEC,
            LibId::Dos => names::DOS,
            LibId::Intuition => names::INTUITION,
            LibId::Graphics => names::GRAPHICS,
            LibId::Icon => names::ICON,
            LibId::Internal => &[],
        };
        names::lvo_name(t, idx)
    }
}

/// Trap interne (libreria 7).
pub const TRAP_EXIT: u16 = 0;
pub const TRAP_INT_NEXT: u16 = 1;
pub const TRAP_INT_END: u16 = 2;
/// Trap 0x100+n: eccezione inattesa con vettore n (bus error, istruzione illegale...).
pub const TRAP_EXC_BASE: u16 = 0x100;

pub fn trap_opcode(lib: LibId, idx: u16) -> u16 {
    0xA000 | ((lib as u16) << 9) | (idx & 0x1FF)
}

/// Cosa fare dopo aver gestito una trap.
pub enum Outcome {
    /// Funzione di libreria completata: emula `rts`.
    Return,
    /// Trap "in linea" (codice interno): prosegui all'istruzione successiva.
    Continue,
    /// Il programma è terminato con questo codice.
    Exit(i32),
    /// La funzione ha già impostato il PC (es. Supervisor): non toccarlo.
    Jumped,
    /// Funzione non ancora implementata: ferma l'emulazione.
    Unimplemented,
}

/// Contesto di una chiamata: registri della CPU e memoria.
pub struct Ctx<'a> {
    pub cpu: &'a mut CpuCore,
    pub bus: &'a mut Bus,
}

impl Ctx<'_> {
    pub fn d(&self, n: usize) -> u32 {
        self.cpu.d(n)
    }
    pub fn a(&self, n: usize) -> u32 {
        self.cpu.a(n)
    }
    pub fn set_d0(&mut self, v: u32) {
        self.cpu.set_d(0, v);
    }
}

/// Allocatore di memoria semplice (first fit con unione dei blocchi liberi).
pub struct Heap {
    /// Blocchi liberi: inizio → dimensione.
    free: BTreeMap<u32, u32>,
    /// Blocchi allocati: inizio → dimensione (per diagnostica e FreeMem senza dimensione).
    pub used: BTreeMap<u32, u32>,
}

impl Heap {
    pub fn new(start: u32, end: u32) -> Heap {
        let mut free = BTreeMap::new();
        free.insert(start, end - start);
        Heap {
            free,
            used: BTreeMap::new(),
        }
    }

    pub fn alloc(&mut self, size: u32) -> Option<u32> {
        let size = (size.max(1) + 7) & !7;
        let (&start, &len) = self.free.iter().find(|(_, l)| **l >= size)?;
        self.free.remove(&start);
        if len > size {
            self.free.insert(start + size, len - size);
        }
        self.used.insert(start, size);
        Some(start)
    }

    /// Alloca a un indirizzo preciso (AllocAbs).
    pub fn alloc_at(&mut self, addr: u32, size: u32) -> Option<u32> {
        let addr = addr & !7;
        let size = (size + 7) & !7;
        let (&start, &len) = self.free.range(..=addr).next_back()?;
        if start + len < addr + size {
            return None;
        }
        self.free.remove(&start);
        if addr > start {
            self.free.insert(start, addr - start);
        }
        if start + len > addr + size {
            self.free.insert(addr + size, start + len - addr - size);
        }
        self.used.insert(addr, size);
        Some(addr)
    }

    pub fn free(&mut self, addr: u32) -> bool {
        let Some(size) = self.used.remove(&addr) else {
            return false;
        };
        let mut start = addr;
        let mut len = size;
        if let Some((&ps, &pl)) = self.free.range(..start).next_back()
            && ps + pl == start
        {
            self.free.remove(&ps);
            start = ps;
            len += pl;
        }
        if let Some(&nl) = self.free.get(&(start + len)) {
            self.free.remove(&(start + len));
            len += nl;
        }
        self.free.insert(start, len);
        true
    }

    pub fn available(&self) -> u32 {
        self.free.values().sum()
    }
    pub fn largest(&self) -> u32 {
        self.free.values().copied().max().unwrap_or(0)
    }
}

pub struct Library {
    pub id: LibId,
    pub base: u32,
}

/// File aperto dal programma.
pub struct OpenFile {
    pub path: vfs::Resolved,
    pub data: Vec<u8>,
    pub pos: usize,
    pub writable: bool,
    pub dirty: bool,
}

/// Lock su file o directory.
pub struct LockInfo {
    pub path: vfs::Resolved,
    /// Stato di ExNext: indice della prossima voce da restituire.
    pub exnext: usize,
}

pub struct Os {
    pub heap: Heap,
    pub libs: Vec<Library>,
    pub exec_base: u32,
    pub process: u32,
    pub vfs: vfs::Vfs,
    pub cur_dir: vfs::Resolved,
    /// FileHandle (BPTR) → file aperto.
    pub files: HashMap<u32, OpenFile>,
    /// FileLock (BPTR) → lock.
    pub locks: HashMap<u32, LockInfo>,
    pub io_err: u32,
    /// Server di interrupt per livello (AddIntServer): (is_Data, is_Code, nodo).
    pub int_servers: BTreeMap<u32, Vec<(u32, u32, u32)>>,
    pub int_dispatch_index: usize,
    pub int_stub: u32,
    pub exit_stub: u32,
    /// Conteggio delle chiamate per funzione, per il rapporto finale.
    pub call_counts: BTreeMap<(LibId, u16), u64>,
    pub trace: Option<std::io::BufWriter<std::fs::File>>,
    /// Ultima funzione non implementata incontrata.
    pub missing: Option<(LibId, u16)>,
    /// Output del programma verso la console (Write su Output()).
    pub console: Vec<u8>,
    pub stdout_fh: u32,
    pub stdin_fh: u32,
    /// Argomenti passati sulla riga di comando (come da startup-sequence).
    pub args: String,
    pub signals_alloc: u32,
    /// Strutture grafiche create dal programma (schermi, finestre...).
    pub gfx: graphics::GfxState,
    /// Cinquantesimi di secondo richiesti da Delay() e non ancora trascorsi.
    pub pending_delay: u64,
    /// Vertical blank attesi da WaitTOF() e non ancora trascorsi.
    pub pending_frames: u64,
    /// Eccezione inattesa (numero di vettore), se si è verificata.
    pub exception: Option<u8>,
    /// Numero di Open/Read eseguiti (usato dal turbo automatico).
    pub disk_activity: u64,
}

impl Os {
    pub fn new(vfs: vfs::Vfs, heap_start: u32, heap_end: u32) -> Os {
        Os {
            heap: Heap::new(heap_start, heap_end),
            libs: Vec::new(),
            exec_base: 0,
            process: 0,
            vfs,
            cur_dir: vfs::Resolved {
                vol: 0,
                path: String::new(),
            },
            files: HashMap::new(),
            locks: HashMap::new(),
            io_err: 0,
            int_servers: BTreeMap::new(),
            int_dispatch_index: 0,
            int_stub: 0,
            exit_stub: 0,
            call_counts: BTreeMap::new(),
            trace: None,
            missing: None,
            console: Vec::new(),
            stdout_fh: 0,
            stdin_fh: 0,
            args: String::new(),
            signals_alloc: 0xFFFF, // i segnali 0..15 sono riservati al sistema
            gfx: graphics::GfxState::default(),
            pending_delay: 0,
            pending_frames: 0,
            exception: None,
            disk_activity: 0,
        }
    }

    pub fn alloc_clear(&mut self, bus: &mut Bus, size: u32) -> u32 {
        let a = self.heap.alloc(size).expect("memoria emulata esaurita");
        for i in 0..size {
            bus.poke8(a + i, 0);
        }
        a
    }

    /// Crea una libreria emulata con `nfuncs` funzioni e `pos_size` byte di dati.
    pub fn make_library(
        &mut self,
        bus: &mut Bus,
        id: LibId,
        nfuncs: u16,
        pos_size: u32,
        version: u16,
    ) -> u32 {
        let neg = nfuncs as u32 * 6;
        let block = self.alloc_clear(bus, neg + pos_size + 8);
        let base = (block + neg + 3) & !3;
        for i in 0..nfuncs {
            let e = base - 30 - 6 * i as u32;
            // Non tutte le librerie hanno nfuncs voci "vere": le voci in eccesso sono innocue.
            if e < block {
                break;
            }
            bus.poke16(e, trap_opcode(id, i));
            bus.poke16(e + 2, 0x4E75); // rts (non usato: l'rts lo emula la trap)
            bus.poke16(e + 4, 0x4E71); // nop
        }
        // struct Library
        let name = self.alloc_cstr(bus, id.lib_name());
        bus.poke8(base + 8, 9); // ln_Type = NT_LIBRARY
        bus.poke32(base + 10, name);
        bus.poke16(base + 16, neg as u16); // lib_NegSize
        bus.poke16(base + 18, pos_size as u16); // lib_PosSize
        bus.poke16(base + 20, version); // lib_Version
        bus.poke16(base + 22, 0); // lib_Revision
        bus.poke32(base + 24, name); // lib_IdString
        self.libs.push(Library { id, base });
        base
    }

    pub fn alloc_cstr(&mut self, bus: &mut Bus, s: &str) -> u32 {
        let a = self.alloc_clear(bus, s.len() as u32 + 1);
        bus.write_bytes(a, s.as_bytes());
        a
    }

    pub fn lib_base(&self, id: LibId) -> u32 {
        self.libs
            .iter()
            .find(|l| l.id == id)
            .map(|l| l.base)
            .unwrap_or(0)
    }

    pub fn trace_line(&mut self, s: &str) {
        if let Some(t) = self.trace.as_mut() {
            let _ = writeln!(t, "{s}");
        }
    }

    /// Gestisce una trap A-line. `pc` è l'indirizzo dell'istruzione che l'ha generata.
    pub fn handle_trap(&mut self, ctx: &mut Ctx, opcode: u16, pc: u32) -> Outcome {
        let lib_num = (opcode >> 9) & 7;
        let idx = opcode & 0x1FF;
        let Some(lib) = LibId::from_num(lib_num) else {
            return Outcome::Unimplemented;
        };
        if lib == LibId::Internal {
            return self.internal_trap(ctx, idx);
        }
        *self.call_counts.entry((lib, idx)).or_insert(0) += 1;
        let caller = ctx.bus.peek32(ctx.a(7));
        let handled = match lib {
            LibId::Exec => exec::call(self, ctx, idx),
            LibId::Dos => {
                if idx == 0 || idx == 2 {
                    self.disk_activity += 1; // Open o Read
                }
                dos::call(self, ctx, idx)
            }
            LibId::Intuition => intuition::call(self, ctx, idx),
            LibId::Graphics => graphics::call(self, ctx, idx),
            LibId::Icon => None,
            LibId::Internal => unreachable!(),
        };
        match handled {
            Some(desc) => {
                if self.trace.is_some() {
                    let line = format!(
                        "[{:06x}<-{:06x}] {}.{} {} => d0={:08x}",
                        pc,
                        caller,
                        lib.lib_name().trim_end_matches(".library"),
                        lib.func_name(idx as usize),
                        desc,
                        ctx.d(0)
                    );
                    self.trace_line(&line);
                }
                if let Some(code) = desc.strip_prefix("EXIT:") {
                    let code = code.split_whitespace().next().unwrap_or("0");
                    return Outcome::Exit(code.parse().unwrap_or(0));
                }
                if desc.starts_with("JUMP") {
                    return Outcome::Jumped;
                }
                Outcome::Return
            }
            None => {
                self.missing = Some((lib, idx));
                Outcome::Unimplemented
            }
        }
    }

    fn internal_trap(&mut self, ctx: &mut Ctx, idx: u16) -> Outcome {
        match idx {
            TRAP_EXIT => Outcome::Exit(ctx.d(0) as i32),
            TRAP_INT_NEXT => {
                // Dispatcher degli interrupt di livello 3 (VERTB): restituisce il
                // prossimo server in a1/a5 e azzera il flag Z, oppure imposta Z.
                let servers = self.int_servers.get(&5).cloned().unwrap_or_default();
                let sr = ctx.cpu.get_sr();
                if let Some(&(data, code, _)) = servers.get(self.int_dispatch_index) {
                    self.int_dispatch_index += 1;
                    ctx.cpu.set_a(1, data);
                    ctx.cpu.set_a(5, code);
                    ctx.cpu.set_a(0, 0xDFF000);
                    ctx.cpu.set_a(6, self.exec_base);
                    ctx.cpu.set_sr(sr & !0x0004);
                } else {
                    ctx.cpu.set_sr(sr | 0x0004);
                }
                Outcome::Continue
            }
            n if n >= TRAP_EXC_BASE => {
                self.missing = None;
                self.exception = Some((n - TRAP_EXC_BASE) as u8);
                Outcome::Unimplemented
            }
            TRAP_INT_END => {
                self.int_dispatch_index = 0;
                // exec azzera il bit di richiesta dopo aver chiamato i server
                ctx.bus.custom.intreq &= !0x0070;
                Outcome::Continue
            }
            _ => Outcome::Unimplemented,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Heap;

    #[test]
    fn heap_alloca_libera_e_unisce() {
        let mut h = Heap::new(0x1000, 0x2000);
        let a = h.alloc(100).unwrap();
        let b = h.alloc(200).unwrap();
        assert!(b > a);
        assert_eq!(h.available(), 0x1000 - 104 - 200);
        assert!(h.free(a));
        assert!(h.free(b));
        // Dopo aver liberato tutto deve restare un unico blocco libero.
        assert_eq!(h.largest(), 0x1000);
        assert!(!h.free(a), "doppia liberazione rilevata");
    }

    #[test]
    fn heap_alloc_abs() {
        let mut h = Heap::new(0x1000, 0x2000);
        assert_eq!(h.alloc_at(0x1800, 0x100), Some(0x1800));
        assert_eq!(h.alloc_at(0x1880, 0x10), None);
        assert_eq!(h.available(), 0x1000 - 0x100);
    }
}
