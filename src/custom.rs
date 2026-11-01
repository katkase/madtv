//! Chip custom dell'Amiga (OCS) e CIA, nella misura in cui servono a Mad TV.
//!
//! Dall'analisi dell'eseguibile (vedi `docs/analisi-amiga.md`) il gioco accede
//! direttamente a: blitter (BLTxxx), copper (COP1LC/COPJMP1), palette
//! (COLORxx), DMACON/DMACONR, interrupt (INTENA/INTREQ), joystick/mouse
//! (JOY0DAT, POTGO/POTGOR) e due registri delle CIA. L'audio (Paula) è usato
//! dal replayer TFMX.
//!
//! Fase 1: ogni scrittura viene memorizzata e contata; il blitter in modalità
//! "area" è già implementato perché è il modo in cui il gioco disegna tutto.
//! La modalità linea viene solo segnalata.

pub const NAMES: [(u16, &str); 72] = [
    (0x002, "DMACONR"),
    (0x004, "VPOSR"),
    (0x006, "VHPOSR"),
    (0x00A, "JOY0DAT"),
    (0x00C, "JOY1DAT"),
    (0x010, "ADKCONR"),
    (0x016, "POTGOR"),
    (0x018, "SERDATR"),
    (0x01C, "INTENAR"),
    (0x01E, "INTREQR"),
    (0x020, "DSKPTH"),
    (0x024, "DSKLEN"),
    (0x02A, "VPOSW"),
    (0x02E, "COPCON"),
    (0x034, "POTGO"),
    (0x036, "JOYTEST"),
    (0x040, "BLTCON0"),
    (0x042, "BLTCON1"),
    (0x044, "BLTAFWM"),
    (0x046, "BLTALWM"),
    (0x048, "BLTCPTH"),
    (0x04A, "BLTCPTL"),
    (0x04C, "BLTBPTH"),
    (0x04E, "BLTBPTL"),
    (0x050, "BLTAPTH"),
    (0x052, "BLTAPTL"),
    (0x054, "BLTDPTH"),
    (0x056, "BLTDPTL"),
    (0x058, "BLTSIZE"),
    (0x060, "BLTCMOD"),
    (0x062, "BLTBMOD"),
    (0x064, "BLTAMOD"),
    (0x066, "BLTDMOD"),
    (0x070, "BLTCDAT"),
    (0x072, "BLTBDAT"),
    (0x074, "BLTADAT"),
    (0x080, "COP1LCH"),
    (0x082, "COP1LCL"),
    (0x084, "COP2LCH"),
    (0x086, "COP2LCL"),
    (0x088, "COPJMP1"),
    (0x08A, "COPJMP2"),
    (0x08E, "DIWSTRT"),
    (0x090, "DIWSTOP"),
    (0x092, "DDFSTRT"),
    (0x094, "DDFSTOP"),
    (0x096, "DMACON"),
    (0x098, "CLXCON"),
    (0x09A, "INTENA"),
    (0x09C, "INTREQ"),
    (0x09E, "ADKCON"),
    (0x0A0, "AUD0LCH"),
    (0x0A6, "AUD0PER"),
    (0x0A8, "AUD0VOL"),
    (0x0B0, "AUD1LCH"),
    (0x0B6, "AUD1PER"),
    (0x0C0, "AUD2LCH"),
    (0x0C6, "AUD2PER"),
    (0x0D0, "AUD3LCH"),
    (0x0D6, "AUD3PER"),
    (0x0E0, "BPL1PTH"),
    (0x0E4, "BPL2PTH"),
    (0x0E8, "BPL3PTH"),
    (0x0EC, "BPL4PTH"),
    (0x0F0, "BPL5PTH"),
    (0x0F4, "BPL6PTH"),
    (0x100, "BPLCON0"),
    (0x102, "BPLCON1"),
    (0x104, "BPLCON2"),
    (0x108, "BPL1MOD"),
    (0x10A, "BPL2MOD"),
    (0x120, "SPR0PTH"),
];

pub fn reg_name(off: u16) -> String {
    if (0x180..0x1C0).contains(&off) {
        return format!("COLOR{:02}", (off - 0x180) / 2);
    }
    NAMES
        .iter()
        .find(|(o, _)| *o == off)
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| format!("reg_{off:03X}"))
}

/// Cicli CPU (7,09 MHz) per riga di scansione PAL: 227 color clock × 2.
pub const CYCLES_PER_LINE: u64 = 454;
pub const LINES_PER_FRAME: u64 = 313;

pub struct Custom {
    /// Ultimo valore scritto in ciascun registro (indice = offset/2).
    pub regs: [u16; 256],
    /// Numero di scritture per registro (per il rapporto diagnostico).
    pub write_count: [u64; 256],
    pub read_count: [u64; 256],
    pub dmacon: u16,
    pub intena: u16,
    pub intreq: u16,
    /// Contatore di cicli CPU, da cui derivano riga e colonna del pennello elettronico.
    pub cycles: u64,
    pub frame: u64,
    /// Coordinate del mouse in formato JOY0DAT (contatori a 8 bit).
    pub mouse_x: u8,
    pub mouse_y: u8,
    pub left_button: bool,
    pub right_button: bool,
    /// Registri CIA (CIA-A in [0], CIA-B in [1]), 16 registri ciascuno.
    pub cia: [[u8; 16]; 2],
    /// Statistiche del blitter.
    pub blits: u64,
    pub line_blits: u64,
    /// Liste copper "accese": indirizzo dell'ultimo COPJMP1/strobe.
    pub cop1lc: u32,
    pub cop2lc: u32,
    /// Stato del copper in tempo reale: indirizzo della prossima istruzione,
    /// eventuale WAIT in corso (riga, maschera), fermo fino al prossimo frame.
    pub cop_pc: u32,
    pub cop_wait: Option<(u16, u16)>,
    pub cop_halted: bool,
    pub copper_moves: u64,
    /// Immagine a schermo, costruita riga per riga.
    pub display: crate::render::Display,
}

impl Custom {
    pub fn new() -> Custom {
        let mut c = Custom {
            regs: [0; 256],
            write_count: [0; 256],
            read_count: [0; 256],
            dmacon: 0x03F0, // come lo lascia AmigaOS: DMAEN, BPLEN, COPEN, BLTEN, SPREN, DSKEN
            intena: 0x602C, // INTEN, EXTER, VERTB, PORTS, SOFTINT
            intreq: 0,
            cycles: 0,
            frame: 0,
            mouse_x: 0,
            mouse_y: 0,
            left_button: false,
            right_button: false,
            cia: [[0; 16]; 2],
            blits: 0,
            line_blits: 0,
            cop1lc: 0,
            cop2lc: 0,
            cop_pc: 0,
            cop_wait: None,
            cop_halted: true,
            copper_moves: 0,
            display: crate::render::Display::new(),
        };
        c.cia[0][0] = 0xFF; // PRA: nessun pulsante premuto, disco inserito
        c
    }

    pub fn line(&self) -> u64 {
        (self.cycles / CYCLES_PER_LINE) % LINES_PER_FRAME
    }

    /// Avanza il tempo di `cycles` cicli CPU. Per ogni riga di scansione
    /// attraversata esegue il copper e disegna la riga. Restituisce true se è
    /// iniziato almeno un nuovo frame (vertical blank).
    pub fn advance(&mut self, cycles: u64, ram: &mut [u8]) -> bool {
        let line_before = self.cycles / CYCLES_PER_LINE;
        self.cycles += cycles;
        let line_after = self.cycles / CYCLES_PER_LINE;
        if line_after == line_before {
            return false;
        }
        // Dopo un salto lungo (Delay, STOP) basta emulare l'ultimo frame intero.
        let first = (line_before + 1).max(line_after.saturating_sub(LINES_PER_FRAME - 1));
        let mut new_frame = false;
        for abs in first..=line_after {
            let y = (abs % LINES_PER_FRAME) as u32;
            if y == 0 {
                self.frame = abs / LINES_PER_FRAME;
                self.intreq |= 0x0020; // VERTB
                // All'inizio di ogni frame il copper riparte da COP1LC.
                self.cop_pc = self.cop1lc;
                self.cop_wait = None;
                self.cop_halted = self.cop1lc == 0;
                self.display.end_frame();
                new_frame = true;
            }
            self.run_copper(y as u16, ram);
            self.display.line(y, &mut self.regs, self.dmacon, ram);
        }
        new_frame
    }

    /// Esegue il copper per la riga `line`, finché non incontra una WAIT non soddisfatta.
    fn run_copper(&mut self, line: u16, ram: &mut [u8]) {
        if self.cop_halted || self.dmacon & 0x0280 != 0x0280 {
            return;
        }
        let rd = |ram: &[u8], a: u32| -> u16 {
            let a = (a & 0x1F_FFFE) as usize;
            if a + 1 < ram.len() {
                u16::from_be_bytes([ram[a], ram[a + 1]])
            } else {
                0xFFFF
            }
        };
        let beam = line & 0xFF;
        for _ in 0..120 {
            if let Some((vp, mask)) = self.cop_wait {
                if (beam & mask) >= (vp & mask) && !(line >= 256 && vp >= 0x80) {
                    self.cop_wait = None;
                } else {
                    return;
                }
            }
            let w1 = rd(ram, self.cop_pc);
            let w2 = rd(ram, self.cop_pc + 2);
            self.cop_pc += 4;
            if w1 & 1 == 0 {
                let reg = w1 & 0x1FE;
                // Il copper non può scrivere i registri sotto $40 (senza il bit "danger" di COPCON).
                if reg < 0x40 && !(reg >= 0x20 && self.regs[0x2E / 2] & 2 != 0) {
                    self.cop_halted = true;
                    return;
                }
                self.copper_moves += 1;
                self.write(reg as u32, w2, ram);
                if reg == 0x88 {
                    self.cop_pc = self.cop1lc;
                } else if reg == 0x8A {
                    self.cop_pc = self.cop2lc;
                }
            } else if w2 & 1 == 0 {
                if w1 == 0xFFFF && w2 == 0xFFFE {
                    self.cop_halted = true;
                    return;
                }
                self.cop_wait = Some((w1 >> 8, ((w2 >> 8) & 0x7F) | 0x80));
            }
            // SKIP: ignorato
        }
    }

    pub fn read(&mut self, off: u32) -> u16 {
        let off = off as u16;
        self.read_count[(off / 2) as usize] += 1;
        match off {
            0x002 => self.dmacon & 0x07FF, // DMACONR: bit 14 (blitter occupato) sempre 0
            0x004 => {
                // VPOSR: bit 15 = long frame, bit 0 = V8
                let v = self.line();
                0x8000 | ((v >> 8) as u16 & 1)
            }
            0x006 => {
                let v = self.line() as u16;
                let h = ((self.cycles % CYCLES_PER_LINE) / 2) as u16;
                (v << 8) | (h & 0xFF)
            }
            0x00A => ((self.mouse_y as u16) << 8) | self.mouse_x as u16,
            0x00C => 0,
            0x016 => {
                // POTGOR: bit 10 = tasto destro porta 0 (attivo basso)
                if self.right_button { 0xFB00 } else { 0xFF00 }
            }
            0x01C => self.intena,
            0x01E => self.intreq,
            _ => self.regs[(off / 2) as usize],
        }
    }

    pub fn write(&mut self, off: u32, val: u16, ram: &mut [u8]) {
        let off = off as u16;
        let idx = (off / 2) as usize;
        self.write_count[idx] += 1;
        self.regs[idx] = val;
        match off {
            0x096 => set_clr(&mut self.dmacon, val),
            0x09A => set_clr(&mut self.intena, val),
            0x09C => set_clr(&mut self.intreq, val),
            0x080 | 0x082 => self.cop1lc = self.long_reg(0x080),
            0x088 => {
                // COPJMP1 scritto dalla CPU: il copper riparte subito da COP1LC.
                self.cop_pc = self.cop1lc;
                self.cop_wait = None;
                self.cop_halted = false;
            }
            0x084 | 0x086 => self.cop2lc = self.long_reg(0x084),
            0x058 => self.blit(ram),
            0x120..=0x13E => self
                .display
                .sprite_pointer_written(((off - 0x120) / 4) as usize),
            _ => {}
        }
    }

    pub fn long_reg(&self, off: u16) -> u32 {
        ((self.regs[(off / 2) as usize] as u32) << 16) | self.regs[(off / 2 + 1) as usize] as u32
    }

    pub fn cia_read(&mut self, a: u32) -> u8 {
        // CIA-A: indirizzi dispari 0xBFE001 + reg*0x100; CIA-B: pari 0xBFD000 + reg*0x100
        let which = if a & 1 == 1 { 0 } else { 1 };
        let reg = ((a >> 8) & 0xF) as usize;
        if which == 0 && reg == 0 {
            // PRA: bit 6 = tasto sinistro porta 0, bit 7 = fuoco porta 1 (attivi bassi)
            let mut v = self.cia[0][0] | 0xC0;
            if self.left_button {
                v &= !0x40;
            }
            return v;
        }
        if reg == 0xD {
            return 0; // ICR: nessun interrupt in sospeso
        }
        self.cia[which][reg]
    }

    pub fn cia_write(&mut self, a: u32, v: u8) {
        let which = if a & 1 == 1 { 0 } else { 1 };
        let reg = ((a >> 8) & 0xF) as usize;
        self.cia[which][reg] = v;
    }

    /// Esegue una blittata quando viene scritto BLTSIZE.
    fn blit(&mut self, ram: &mut [u8]) {
        self.blits += 1;
        let con0 = self.regs[0x40 / 2];
        let con1 = self.regs[0x42 / 2];
        if con1 & 1 != 0 {
            // Modalità linea: verrà implementata quando servirà (fase 2).
            self.line_blits += 1;
            return;
        }
        let size = self.regs[0x58 / 2];
        let mut h = (size >> 6) as u32;
        let mut w = (size & 0x3F) as u32;
        if h == 0 {
            h = 1024;
        }
        if w == 0 {
            w = 64;
        }
        let use_a = con0 & 0x0800 != 0;
        let use_b = con0 & 0x0400 != 0;
        let use_c = con0 & 0x0200 != 0;
        let use_d = con0 & 0x0100 != 0;
        let minterm = (con0 & 0xFF) as u8;
        let ash = (con0 >> 12) as u32;
        let bsh = (con1 >> 12) as u32;
        let desc = con1 & 0x0002 != 0;
        let fill = con1 & 0x0018; // IFE (bit 3) / EFE (bit 4)
        let fci = con1 & 0x0004 != 0;
        let afwm = self.regs[0x44 / 2];
        let alwm = self.regs[0x46 / 2];
        let mods = |r: u16| (r as i16 as i32) & !1;
        let (amod, bmod, cmod, dmod) = (
            mods(self.regs[0x64 / 2]),
            mods(self.regs[0x62 / 2]),
            mods(self.regs[0x60 / 2]),
            mods(self.regs[0x66 / 2]),
        );
        let mut apt = self.long_reg(0x50) & 0x1F_FFFE;
        let mut bpt = self.long_reg(0x4C) & 0x1F_FFFE;
        let mut cpt = self.long_reg(0x48) & 0x1F_FFFE;
        let mut dpt = self.long_reg(0x54) & 0x1F_FFFE;
        let mut adat = self.regs[0x74 / 2];
        let mut bdat = self.regs[0x72 / 2];
        let mut cdat = self.regs[0x70 / 2];
        let mut aold: u16 = 0;
        let mut bold: u16 = 0;
        let step: i32 = if desc { -2 } else { 2 };
        let rd = |ram: &[u8], a: u32| -> u16 {
            let a = a as usize;
            if a + 1 < ram.len() {
                u16::from_be_bytes([ram[a], ram[a + 1]])
            } else {
                0
            }
        };
        let mut any_nonzero = false;
        for _y in 0..h {
            let mut fill_carry = fci;
            for x in 0..w {
                if use_a {
                    adat = rd(ram, apt);
                    apt = (apt as i32 + step) as u32;
                }
                if use_b {
                    bdat = rd(ram, bpt);
                    bpt = (bpt as i32 + step) as u32;
                }
                if use_c {
                    cdat = rd(ram, cpt);
                    cpt = (cpt as i32 + step) as u32;
                }
                let mut amask = 0xFFFFu16;
                if x == 0 {
                    amask &= afwm;
                }
                if x == w - 1 {
                    amask &= alwm;
                }
                let am = adat & amask;
                // Scorrimento: verso destra in modalità ascendente, verso sinistra in discendente.
                let (ashifted, bshifted) = if desc {
                    (
                        (((am as u32) << 16 | aold as u32) >> (16 - ash)) as u16,
                        (((bdat as u32) << 16 | bold as u32) >> (16 - bsh)) as u16,
                    )
                } else {
                    (
                        (((aold as u32) << 16 | am as u32) >> ash) as u16,
                        (((bold as u32) << 16 | bdat as u32) >> bsh) as u16,
                    )
                };
                aold = am;
                bold = bdat;
                let mut d = minterm_apply(minterm, ashifted, bshifted, cdat);
                if fill != 0 {
                    d = fill_word(d, &mut fill_carry, fill & 0x0010 != 0, desc);
                }
                if d != 0 {
                    any_nonzero = true;
                }
                if use_d {
                    let a = dpt as usize;
                    if a + 1 < ram.len() {
                        ram[a] = (d >> 8) as u8;
                        ram[a + 1] = d as u8;
                    }
                    dpt = (dpt as i32 + step) as u32;
                }
            }
            if use_a {
                apt = (apt as i32 + if desc { -amod } else { amod }) as u32;
            }
            if use_b {
                bpt = (bpt as i32 + if desc { -bmod } else { bmod }) as u32;
            }
            if use_c {
                cpt = (cpt as i32 + if desc { -cmod } else { cmod }) as u32;
            }
            if use_d {
                dpt = (dpt as i32 + if desc { -dmod } else { dmod }) as u32;
            }
        }
        // I puntatori finali restano nei registri, come sull'hardware.
        self.set_long(0x50, apt);
        self.set_long(0x4C, bpt);
        self.set_long(0x48, cpt);
        self.set_long(0x54, dpt);
        self.regs[0x74 / 2] = adat;
        self.regs[0x72 / 2] = bdat;
        self.regs[0x70 / 2] = cdat;
        // BZERO in DMACONR (bit 13) se il risultato è tutto zero
        if any_nonzero {
            self.dmacon &= !0x2000;
        } else {
            self.dmacon |= 0x2000;
        }
        self.intreq |= 0x0040; // BLIT finito
    }

    fn set_long(&mut self, off: u16, v: u32) {
        self.regs[(off / 2) as usize] = (v >> 16) as u16;
        self.regs[(off / 2 + 1) as usize] = v as u16;
    }
}

impl Default for Custom {
    fn default() -> Self {
        Self::new()
    }
}

fn set_clr(reg: &mut u16, val: u16) {
    if val & 0x8000 != 0 {
        *reg |= val & 0x7FFF;
    } else {
        *reg &= !(val & 0x7FFF);
    }
}

/// Funzione logica del blitter: ogni bit del minterm sceglie una delle 8
/// combinazioni di A, B, C.
fn minterm_apply(m: u8, a: u16, b: u16, c: u16) -> u16 {
    let mut d = 0u16;
    if m & 0x01 != 0 {
        d |= !a & !b & !c;
    }
    if m & 0x02 != 0 {
        d |= !a & !b & c;
    }
    if m & 0x04 != 0 {
        d |= !a & b & !c;
    }
    if m & 0x08 != 0 {
        d |= !a & b & c;
    }
    if m & 0x10 != 0 {
        d |= a & !b & !c;
    }
    if m & 0x20 != 0 {
        d |= a & !b & c;
    }
    if m & 0x40 != 0 {
        d |= a & b & !c;
    }
    if m & 0x80 != 0 {
        d |= a & b & c;
    }
    d
}

/// Riempimento di aree (inclusive o exclusive fill), da destra a sinistra
/// come sull'hardware (il fill si usa sempre in modalità discendente).
fn fill_word(d: u16, carry: &mut bool, exclusive: bool, _desc: bool) -> u16 {
    let mut out = 0u16;
    for bit in 0..16 {
        let m = 1u16 << bit;
        let src = d & m != 0;
        if exclusive {
            if src {
                *carry = !*carry;
            }
            if *carry {
                out |= m;
            }
        } else {
            if *carry || src {
                out |= m;
            }
            if src {
                *carry = !*carry;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_copy(c: &mut Custom, src: u32, dst: u32, w: u16, h: u16) {
        c.regs[0x40 / 2] = 0x09F0; // usa A e D, minterm D = A
        c.regs[0x42 / 2] = 0;
        c.regs[0x44 / 2] = 0xFFFF;
        c.regs[0x46 / 2] = 0xFFFF;
        c.regs[0x50 / 2] = (src >> 16) as u16;
        c.regs[0x52 / 2] = src as u16;
        c.regs[0x54 / 2] = (dst >> 16) as u16;
        c.regs[0x56 / 2] = dst as u16;
        c.regs[0x64 / 2] = 0;
        c.regs[0x66 / 2] = 0;
        c.regs[0x58 / 2] = (h << 6) | w;
    }

    #[test]
    fn blitter_copia_semplice() {
        let mut c = Custom::new();
        let mut ram = vec![0u8; 0x1000];
        ram[0x100..0x104].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        setup_copy(&mut c, 0x100, 0x200, 2, 1);
        c.blit(&mut ram);
        assert_eq!(&ram[0x200..0x204], &[0x12, 0x34, 0x56, 0x78]);
        assert_eq!(
            c.dmacon & 0x2000,
            0,
            "BZERO deve essere 0 se il risultato non è nullo"
        );
    }

    #[test]
    fn blitter_scorrimento_a() {
        let mut c = Custom::new();
        let mut ram = vec![0u8; 0x1000];
        ram[0x100..0x104].copy_from_slice(&[0x80, 0x01, 0x00, 0x00]);
        setup_copy(&mut c, 0x100, 0x200, 2, 1);
        c.regs[0x40 / 2] |= 4 << 12; // ASH = 4
        c.blit(&mut ram);
        assert_eq!(&ram[0x200..0x204], &[0x08, 0x00, 0x10, 0x00]);
    }

    #[test]
    fn minterm_cookie_cut() {
        // $CA = (A AND B) OR (NOT A AND C): disegno con maschera
        assert_eq!(minterm_apply(0xCA, 0xFF00, 0xAAAA, 0x5555), 0xAA55);
    }
}
