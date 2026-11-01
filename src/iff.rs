//! Decodifica delle immagini IFF ILBM (il formato grafico standard dell'Amiga),
//! usata per la schermata dei credits `u3.lbm` mostrata prima del gioco.
//!
//! Chunk gestiti: BMHD (dimensioni, numero di bitplane, compressione),
//! CMAP (palette a 8 bit per componente), CAMG (modo video: HAM e Extra
//! Half-Brite), BODY (righe di bitplane interlacciate, non compresse o
//! compresse ByteRun1). Gli altri chunk (CRNG, DPPS...) vengono ignorati.

use std::fmt;

#[derive(Debug)]
pub struct IffError(pub String);

impl fmt::Display for IffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "immagine IFF non valida: {}", self.0)
    }
}
impl std::error::Error for IffError {}

pub struct Image {
    pub width: usize,
    pub height: usize,
    /// Pixel in formato 0x00RRGGBB, riga per riga.
    pub pixels: Vec<u32>,
}

fn be16(d: &[u8], p: usize) -> u16 {
    u16::from_be_bytes([d[p], d[p + 1]])
}
fn be32(d: &[u8], p: usize) -> u32 {
    u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
}

pub fn decode(d: &[u8]) -> Result<Image, IffError> {
    if d.len() < 12 || &d[0..4] != b"FORM" || &d[8..12] != b"ILBM" {
        return Err(IffError("manca FORM ILBM".into()));
    }
    let end = (8 + be32(d, 4) as usize).min(d.len());
    let mut p = 12;
    let (mut w, mut h, mut planes, mut masking, mut comp) = (0usize, 0usize, 0usize, 0u8, 0u8);
    let mut cmap: Vec<[u8; 3]> = Vec::new();
    let mut camg = 0u32;
    let mut body: Option<&[u8]> = None;
    while p + 8 <= end {
        let id = &d[p..p + 4];
        let n = be32(d, p + 4) as usize;
        let data = d
            .get(p + 8..p + 8 + n)
            .ok_or_else(|| IffError("chunk troncato".into()))?;
        match id {
            b"BMHD" if n >= 20 => {
                w = be16(data, 0) as usize;
                h = be16(data, 2) as usize;
                planes = data[8] as usize;
                masking = data[9];
                comp = data[10];
            }
            b"CMAP" => cmap = data.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
            b"CAMG" if n >= 4 => camg = be32(data, 0),
            b"BODY" => body = Some(data),
            _ => {}
        }
        p += 8 + n + (n & 1);
    }
    let body = body.ok_or_else(|| IffError("manca BODY".into()))?;
    if w == 0 || h == 0 || planes == 0 || planes > 8 {
        return Err(IffError(format!(
            "intestazione non valida ({w}x{h}, {planes} bitplane)"
        )));
    }
    let row_bytes = w.div_ceil(16) * 2;
    let stored_planes = planes + if masking == 1 { 1 } else { 0 };

    // Decompressione: per ogni riga, un blocco di row_bytes per ogni bitplane.
    let mut raw = vec![0u8; row_bytes * stored_planes * h];
    match comp {
        0 => {
            let n = raw.len().min(body.len());
            raw[..n].copy_from_slice(&body[..n]);
        }
        1 => byterun1(body, &mut raw),
        c => return Err(IffError(format!("compressione {c} non supportata"))),
    }

    let ham = camg & 0x800 != 0;
    let ehb = camg & 0x80 != 0 || (planes == 6 && !ham && cmap.len() <= 32);
    let color = |i: usize| -> u32 {
        let c = cmap.get(i).copied().unwrap_or([0, 0, 0]);
        // Sull'Amiga OCS contano solo i 4 bit alti di ogni componente.
        let q = |v: u8| ((v >> 4) * 17) as u32;
        (q(c[0]) << 16) | (q(c[1]) << 8) | q(c[2])
    };
    let mut pixels = vec![0u32; w * h];
    for y in 0..h {
        let mut prev = color(0);
        for x in 0..w {
            let byte = x / 8;
            let bit = 7 - (x % 8);
            let mut idx = 0usize;
            for pl in 0..planes {
                let b = raw[(y * stored_planes + pl) * row_bytes + byte];
                idx |= (((b >> bit) & 1) as usize) << pl;
            }
            let c = if ham {
                // HAM6: i 2 bit alti dicono quale componente modificare.
                let ctl = idx >> (planes - 2);
                let v = ((idx & ((1 << (planes - 2)) - 1)) as u32) * 17;
                match ctl {
                    0 => color(idx),
                    1 => (prev & 0xFFFF00) | v,
                    2 => (prev & 0x00FFFF) | (v << 16),
                    _ => (prev & 0xFF00FF) | (v << 8),
                }
            } else if ehb && idx >= 32 {
                (color(idx - 32) >> 1) & 0x7F7F7F
            } else {
                color(idx)
            };
            pixels[y * w + x] = c;
            prev = c;
        }
    }
    Ok(Image {
        width: w,
        height: h,
        pixels,
    })
}

/// Decompressione ByteRun1 (PackBits): n >= 0 copia n+1 byte; n < 0 ripete il
/// byte successivo 1-n volte; -128 non fa nulla.
fn byterun1(src: &[u8], dst: &mut [u8]) {
    let (mut i, mut o) = (0usize, 0usize);
    while i < src.len() && o < dst.len() {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let cnt = n as usize + 1;
            for _ in 0..cnt {
                if i < src.len() && o < dst.len() {
                    dst[o] = src[i];
                    o += 1;
                    i += 1;
                }
            }
        } else if n != -128 {
            let cnt = 1 - n as isize;
            let v = src.get(i).copied().unwrap_or(0);
            i += 1;
            for _ in 0..cnt {
                if o < dst.len() {
                    dst[o] = v;
                    o += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Costruisce un'immagine 16x2 a 1 bitplane: riga 0 = 0xF0F0, riga 1 = 0x0000,
    /// compressa con ByteRun1, palette nero/bianco.
    #[test]
    fn decodifica_ilbm() {
        let mut bmhd = vec![0u8; 20];
        bmhd[0..2].copy_from_slice(&16u16.to_be_bytes());
        bmhd[2..4].copy_from_slice(&2u16.to_be_bytes());
        bmhd[8] = 1; // bitplane
        bmhd[10] = 1; // ByteRun1
        let cmap = [0u8, 0, 0, 255, 255, 255];
        // riga 0: copia letterale di 2 byte (F0 F0); riga 1: ripeti 0x00 due volte
        let body = [1u8, 0xF0, 0xF0, 0xFF, 0x00];
        let mut d = Vec::new();
        let mut chunk = |id: &[u8], data: &[u8]| {
            d.extend_from_slice(id);
            d.extend_from_slice(&(data.len() as u32).to_be_bytes());
            d.extend_from_slice(data);
            if data.len() % 2 == 1 {
                d.push(0);
            }
        };
        chunk(b"BMHD", &bmhd);
        chunk(b"CMAP", &cmap);
        chunk(b"BODY", &body);
        let mut f = b"FORM".to_vec();
        f.extend_from_slice(&((d.len() + 4) as u32).to_be_bytes());
        f.extend_from_slice(b"ILBM");
        f.extend_from_slice(&d);
        let img = decode(&f).unwrap();
        assert_eq!((img.width, img.height), (16, 2));
        assert_eq!(img.pixels[0], 0xFFFFFF);
        assert_eq!(img.pixels[4], 0x000000);
        assert!(img.pixels[16..].iter().all(|&p| p == 0));
    }
}
