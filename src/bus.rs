//! Bus di memoria dell'"Amiga minimo".
//!
//! Mappa (semplificata rispetto a un A500 reale):
//! - `0x000000..RAM_SIZE`: RAM piatta. La trattiamo tutta come Chip RAM: i chip
//!   custom sono emulati in software, quindi il limite dei 512 KB non serve.
//! - `0xBFD000..0xBFEFFF`: CIA-A / CIA-B (per ora solo registrati).
//! - `0xDFF000..0xDFF1FF`: registri dei chip custom, gestiti da [`Custom`].
//! - tutto il resto legge 0 e ignora le scritture (con un contatore per il log).

use crate::custom::Custom;

pub const RAM_SIZE: usize = 2 * 1024 * 1024;

pub struct Bus {
    pub ram: Vec<u8>,
    pub custom: Custom,
    /// Accessi a indirizzi non mappati (per diagnostica).
    pub unmapped_reads: u64,
    pub unmapped_writes: u64,
    /// Ultimi accessi non mappati, per il rapporto finale.
    pub unmapped_log: Vec<(u32, bool)>,
}

impl Bus {
    pub fn new() -> Bus {
        Bus {
            ram: vec![0; RAM_SIZE],
            custom: Custom::new(),
            unmapped_reads: 0,
            unmapped_writes: 0,
            unmapped_log: Vec::new(),
        }
    }

    // --- accesso diretto alla RAM, usato dal codice Rust del sistema operativo emulato ---

    pub fn peek8(&self, a: u32) -> u8 {
        self.ram.get((a & 0xFF_FFFF) as usize).copied().unwrap_or(0)
    }
    pub fn peek16(&self, a: u32) -> u16 {
        u16::from_be_bytes([self.peek8(a), self.peek8(a + 1)])
    }
    pub fn peek32(&self, a: u32) -> u32 {
        u32::from_be_bytes([
            self.peek8(a),
            self.peek8(a + 1),
            self.peek8(a + 2),
            self.peek8(a + 3),
        ])
    }
    pub fn poke8(&mut self, a: u32, v: u8) {
        if let Some(m) = self.ram.get_mut((a & 0xFF_FFFF) as usize) {
            *m = v;
        }
    }
    pub fn poke16(&mut self, a: u32, v: u16) {
        let b = v.to_be_bytes();
        self.poke8(a, b[0]);
        self.poke8(a + 1, b[1]);
    }
    pub fn poke32(&mut self, a: u32, v: u32) {
        let b = v.to_be_bytes();
        for (i, x) in b.iter().enumerate() {
            self.poke8(a + i as u32, *x);
        }
    }
    pub fn write_bytes(&mut self, a: u32, data: &[u8]) {
        let s = a as usize;
        self.ram[s..s + data.len()].copy_from_slice(data);
    }
    pub fn read_bytes(&self, a: u32, n: usize) -> Vec<u8> {
        let s = a as usize;
        self.ram[s..s + n].to_vec()
    }
    /// Stringa C terminata da zero.
    pub fn cstr(&self, a: u32) -> String {
        let mut s = String::new();
        let mut p = a;
        while s.len() < 256 {
            let c = self.peek8(p);
            if c == 0 {
                break;
            }
            s.push(c as char);
            p += 1;
        }
        s
    }
    /// Stringa BCPL (primo byte = lunghezza), dato un indirizzo in byte.
    pub fn bstr(&self, a: u32) -> String {
        let n = self.peek8(a) as u32;
        (0..n).map(|i| self.peek8(a + 1 + i) as char).collect()
    }

    fn note_unmapped(&mut self, a: u32, write: bool) {
        if write {
            self.unmapped_writes += 1
        } else {
            self.unmapped_reads += 1
        }
        if self.unmapped_log.len() < 64 {
            self.unmapped_log.push((a, write));
        }
    }

    fn is_ram(a: u32) -> bool {
        (a as usize) < RAM_SIZE
    }
    fn is_custom(a: u32) -> bool {
        (0xDFF000..0xDFF200).contains(&a)
    }
    fn is_cia(a: u32) -> bool {
        (0xBFD000..0xBFF000).contains(&a)
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

impl m68k::AddressBus for Bus {
    fn read_byte(&mut self, address: u32) -> u8 {
        let a = address & 0xFF_FFFF;
        if Self::is_ram(a) {
            self.ram[a as usize]
        } else if Self::is_custom(a) {
            let w = self.custom.read(a & 0x1FE);
            if a & 1 == 0 { (w >> 8) as u8 } else { w as u8 }
        } else if Self::is_cia(a) {
            self.custom.cia_read(a)
        } else {
            self.note_unmapped(a, false);
            0
        }
    }
    fn read_word(&mut self, address: u32) -> u16 {
        let a = address & 0xFF_FFFF;
        if Self::is_ram(a) && Self::is_ram(a + 1) {
            u16::from_be_bytes([self.ram[a as usize], self.ram[a as usize + 1]])
        } else if Self::is_custom(a) {
            self.custom.read(a & 0x1FE)
        } else {
            u16::from_be_bytes([self.read_byte(a), self.read_byte(a + 1)])
        }
    }
    fn read_long(&mut self, address: u32) -> u32 {
        ((self.read_word(address) as u32) << 16) | self.read_word(address.wrapping_add(2)) as u32
    }
    fn write_byte(&mut self, address: u32, value: u8) {
        let a = address & 0xFF_FFFF;
        if Self::is_ram(a) {
            self.ram[a as usize] = value;
        } else if Self::is_custom(a) {
            // Le scritture a byte sui registri custom replicano il byte su entrambe le metà.
            self.custom
                .write(a & 0x1FE, u16::from_be_bytes([value, value]), &mut self.ram);
        } else if Self::is_cia(a) {
            self.custom.cia_write(a, value);
        } else {
            self.note_unmapped(a, true);
        }
    }
    fn write_word(&mut self, address: u32, value: u16) {
        let a = address & 0xFF_FFFF;
        if Self::is_ram(a) && Self::is_ram(a + 1) {
            let b = value.to_be_bytes();
            self.ram[a as usize] = b[0];
            self.ram[a as usize + 1] = b[1];
        } else if Self::is_custom(a) {
            self.custom.write(a & 0x1FE, value, &mut self.ram);
        } else {
            let b = value.to_be_bytes();
            self.write_byte(a, b[0]);
            self.write_byte(a + 1, b[1]);
        }
    }
    fn write_long(&mut self, address: u32, value: u32) {
        self.write_word(address, (value >> 16) as u16);
        self.write_word(address.wrapping_add(2), value as u16);
    }
}
