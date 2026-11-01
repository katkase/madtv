//! Parser degli eseguibili AmigaOS in formato "hunk" (quello caricato da LoadSeg).
//!
//! Un eseguibile è una sequenza di hunk (segmenti): CODE, DATA o BSS, ognuno
//! con eventuali tabelle di rilocazione RELOC32. Al caricamento AmigaDOS
//! alloca ogni segmento, copia i dati, applica le rilocazioni (somma l'indirizzo
//! del segmento di destinazione alle longword indicate) e collega i segmenti in
//! una "seglist" BCPL.

use std::fmt;

pub const HUNK_CODE: u32 = 0x3E9;
pub const HUNK_DATA: u32 = 0x3EA;
pub const HUNK_BSS: u32 = 0x3EB;
const HUNK_RELOC32: u32 = 0x3EC;
const HUNK_SYMBOL: u32 = 0x3F0;
const HUNK_DEBUG: u32 = 0x3F1;
const HUNK_END: u32 = 0x3F2;
const HUNK_HEADER: u32 = 0x3F3;

#[derive(Debug)]
pub struct HunkError(pub String);

impl fmt::Display for HunkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "eseguibile hunk non valido: {}", self.0)
    }
}
impl std::error::Error for HunkError {}

#[derive(Debug, Clone)]
pub struct Segment {
    /// HUNK_CODE, HUNK_DATA o HUNK_BSS.
    pub kind: u32,
    /// Dimensione di allocazione in byte (dalla tabella dell'header).
    pub alloc_size: u32,
    /// Bit di memoria richiesti (bit 30 = CHIP, bit 31 = FAST).
    pub mem_flags: u32,
    /// Dati iniziali (vuoto per BSS).
    pub data: Vec<u8>,
    /// Rilocazioni: (indice del segmento di destinazione, offset nel segmento corrente).
    pub relocs: Vec<(usize, u32)>,
}

impl Segment {
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            HUNK_CODE => "CODE",
            HUNK_DATA => "DATA",
            HUNK_BSS => "BSS",
            _ => "?",
        }
    }
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn long(&mut self) -> Result<u32, HunkError> {
        let b = self
            .d
            .get(self.p..self.p + 4)
            .ok_or_else(|| HunkError(format!("fine file inattesa a 0x{:x}", self.p)))?;
        self.p += 4;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn bytes(&mut self, n: usize) -> Result<&[u8], HunkError> {
        let b = self
            .d
            .get(self.p..self.p + n)
            .ok_or_else(|| HunkError(format!("fine file inattesa a 0x{:x}", self.p)))?;
        self.p += n;
        Ok(b)
    }
}

pub fn parse(data: &[u8]) -> Result<Vec<Segment>, HunkError> {
    let mut r = Reader { d: data, p: 0 };
    if r.long()? != HUNK_HEADER {
        return Err(HunkError("manca HUNK_HEADER".into()));
    }
    // Nomi di librerie residenti (mai usati in pratica): lista terminata da 0.
    loop {
        let n = r.long()?;
        if n == 0 {
            break;
        }
        r.bytes(n as usize * 4)?;
    }
    let _table_size = r.long()?;
    let first = r.long()?;
    let last = r.long()?;
    let count = (last - first + 1) as usize;
    let mut segs = Vec::with_capacity(count);
    for _ in 0..count {
        let v = r.long()?;
        let mut flags = v & 0xC000_0000;
        if flags == 0xC000_0000 {
            flags = r.long()?; // flag estesi
        }
        segs.push(Segment {
            kind: 0,
            alloc_size: (v & 0x3FFF_FFFF) * 4,
            mem_flags: flags,
            data: Vec::new(),
            relocs: Vec::new(),
        });
    }

    let mut cur = 0usize;
    while cur < count {
        let t = r.long()? & 0x3FFF_FFFF;
        match t {
            HUNK_CODE | HUNK_DATA => {
                let n = r.long()? as usize * 4;
                segs[cur].kind = t;
                segs[cur].data = r.bytes(n)?.to_vec();
            }
            HUNK_BSS => {
                r.long()?;
                segs[cur].kind = t;
            }
            HUNK_RELOC32 => loop {
                let n = r.long()?;
                if n == 0 {
                    break;
                }
                let target = r.long()? as usize;
                for _ in 0..n {
                    let off = r.long()?;
                    segs[cur].relocs.push((target, off));
                }
            },
            HUNK_SYMBOL => loop {
                let n = r.long()?;
                if n == 0 {
                    break;
                }
                r.bytes(n as usize * 4 + 4)?;
            },
            HUNK_DEBUG => {
                let n = r.long()? as usize * 4;
                r.bytes(n)?;
            }
            HUNK_END => cur += 1,
            _ => {
                return Err(HunkError(format!(
                    "tipo di hunk sconosciuto 0x{t:x} a 0x{:x}",
                    r.p - 4
                )));
            }
        }
    }
    Ok(segs)
}
