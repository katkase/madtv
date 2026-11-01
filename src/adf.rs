//! Lettore di immagini disco Amiga (ADF, 880 KB) con filesystem OFS/FFS.
//!
//! Supporta solo la lettura, che è tutto ciò che serve per caricare il gioco:
//! - blocchi da 512 byte, root block al blocco 880;
//! - tabella hash di 72 voci nel root block e nelle directory;
//! - catene hash per i nomi in collisione;
//! - file OFS (blocchi dati con intestazione di 24 byte, collegati in catena)
//!   e FFS (blocchi dati "nudi", elencati nella tabella del file header e
//!   nei blocchi di estensione).
//!
//! I nomi AmigaDOS non distinguono maiuscole e minuscole: le ricerche
//! confrontano i nomi in maiuscolo.

use std::collections::BTreeMap;
use std::fmt;

const BLOCK: usize = 512;
const ROOT_BLOCK: u32 = 880;
const HT_SIZE: usize = 72;

const ST_ROOT: i32 = 1;
const ST_USERDIR: i32 = 2;
const ST_FILE: i32 = -3;

#[derive(Debug)]
pub enum AdfError {
    BadSize(usize),
    NotDos,
    BadBlock(u32),
    NotFound(String),
}

impl fmt::Display for AdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdfError::BadSize(s) => write!(f, "dimensione ADF non valida: {s} byte"),
            AdfError::NotDos => write!(f, "il disco non ha un filesystem AmigaDOS"),
            AdfError::BadBlock(b) => write!(f, "blocco {b} non valido"),
            AdfError::NotFound(p) => write!(f, "file non trovato: {p}"),
        }
    }
}

impl std::error::Error for AdfError {}

/// Una voce di directory (file o sottodirectory).
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u32,
    pub header_block: u32,
}

/// Un'immagine ADF caricata in memoria.
#[derive(Clone)]
pub struct Adf {
    data: Vec<u8>,
    ffs: bool,
    pub volume_name: String,
    /// Indice di tutti i file: percorso in MAIUSCOLO (es. "S/STARTUP-SEQUENCE") → voce.
    index: BTreeMap<String, Entry>,
}

impl Adf {
    pub fn from_bytes(data: Vec<u8>) -> Result<Adf, AdfError> {
        if data.len() != 901_120 && data.len() != 1_802_240 {
            return Err(AdfError::BadSize(data.len()));
        }
        if &data[0..3] != b"DOS" {
            return Err(AdfError::NotDos);
        }
        let ffs = data[3] & 1 == 1;
        let mut adf = Adf {
            data,
            ffs,
            volume_name: String::new(),
            index: BTreeMap::new(),
        };
        let root = adf.block(ROOT_BLOCK)?;
        if be32(root, BLOCK - 4) as i32 != ST_ROOT {
            return Err(AdfError::BadBlock(ROOT_BLOCK));
        }
        adf.volume_name = header_name(root);
        let mut idx = BTreeMap::new();
        adf.walk_dir(ROOT_BLOCK, "", &mut idx, 0)?;
        adf.index = idx;
        Ok(adf)
    }

    pub fn from_file(path: &std::path::Path) -> Result<Adf, Box<dyn std::error::Error>> {
        Ok(Adf::from_bytes(std::fs::read(path)?)?)
    }

    fn block(&self, n: u32) -> Result<&[u8], AdfError> {
        let start = n as usize * BLOCK;
        self.data
            .get(start..start + BLOCK)
            .ok_or(AdfError::BadBlock(n))
    }

    fn walk_dir(
        &self,
        dir_block: u32,
        prefix: &str,
        out: &mut BTreeMap<String, Entry>,
        depth: u32,
    ) -> Result<(), AdfError> {
        if depth > 16 {
            return Ok(()); // protezione contro dischi corrotti con cicli
        }
        let dir = self.block(dir_block)?;
        for slot in 0..HT_SIZE {
            let mut key = be32(dir, 24 + slot * 4);
            let mut guard = 0;
            while key != 0 && guard < 1000 {
                guard += 1;
                let hdr = self.block(key)?;
                let name = header_name(hdr);
                let sec_type = be32(hdr, BLOCK - 4) as i32;
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                let is_dir = sec_type == ST_USERDIR;
                let size = if sec_type == ST_FILE {
                    be32(hdr, 0x144)
                } else {
                    0
                };
                out.insert(
                    path.to_uppercase(),
                    Entry {
                        name: path.clone(),
                        is_dir,
                        size,
                        header_block: key,
                    },
                );
                if is_dir {
                    self.walk_dir(key, &path, out, depth + 1)?;
                }
                key = be32(hdr, 0x1F0); // hash_chain
            }
        }
        Ok(())
    }

    /// Tutte le voci, in ordine alfabetico.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.index.values()
    }

    /// Cerca un file o una directory (percorso relativo alla radice, separatore '/').
    pub fn find(&self, path: &str) -> Option<&Entry> {
        self.index.get(&path.trim_matches('/').to_uppercase())
    }

    /// Legge il contenuto completo di un file.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, AdfError> {
        let e = self
            .find(path)
            .ok_or_else(|| AdfError::NotFound(path.to_string()))?;
        if e.is_dir {
            return Err(AdfError::NotFound(path.to_string()));
        }
        self.read_by_header(e.header_block, e.size)
    }

    fn read_by_header(&self, header: u32, size: u32) -> Result<Vec<u8>, AdfError> {
        let mut out = Vec::with_capacity(size as usize);
        if self.ffs {
            // FFS: la tabella dei blocchi dati è memorizzata al contrario
            // (la prima voce è all'ultimo slot), con eventuali blocchi di estensione.
            let mut hdr_block = header;
            'outer: loop {
                let hdr = self.block(hdr_block)?;
                for i in (0..HT_SIZE).rev() {
                    let b = be32(hdr, 24 + i * 4);
                    if b == 0 {
                        break 'outer;
                    }
                    let blk = self.block(b)?;
                    let take = (size as usize - out.len()).min(BLOCK);
                    out.extend_from_slice(&blk[..take]);
                    if out.len() >= size as usize {
                        break 'outer;
                    }
                }
                hdr_block = be32(hdr, 0x1F8); // estensione
                if hdr_block == 0 {
                    break;
                }
            }
        } else {
            // OFS: catena di blocchi dati, ognuno con 24 byte di intestazione.
            let hdr = self.block(header)?;
            let mut b = be32(hdr, 16); // first_data
            let mut guard = 0;
            while b != 0 && out.len() < size as usize && guard < 2000 {
                guard += 1;
                let blk = self.block(b)?;
                let n = (be32(blk, 12) as usize).min(BLOCK - 24);
                out.extend_from_slice(&blk[24..24 + n]);
                b = be32(blk, 16); // next_data
            }
        }
        out.truncate(size as usize);
        Ok(out)
    }
}

fn be32(b: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// Nome memorizzato come BSTR all'offset 0x1B0 di un header block.
fn header_name(b: &[u8]) -> String {
    let len = (b[0x1B0] as usize).min(30);
    b[0x1B1..0x1B1 + len].iter().map(|&c| c as char).collect()
}
