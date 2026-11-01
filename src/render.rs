//! Generazione dell'immagine a schermo (OCS, PAL) a partire da copper list e
//! bitplane.
//!
//! Per ogni riga di scansione eseguiamo le istruzioni copper fino alla prima
//! WAIT non ancora soddisfatta, poi convertiamo i bitplane della riga in pixel.
//! Semplificazioni attuali (fase 1): niente sprite, niente scroll fine
//! (BPLCON1), niente dual playfield; i cambi di registro a metà riga valgono
//! per tutta la riga. HAM non è ancora supportato; l'Extra Half-Brite sì.

use crate::bus::Bus;

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    /// Diagnostica: righe visibili con bitplane attivi, istruzioni copper eseguite.
    pub active_lines: u32,
    pub copper_moves: u32,
    pub planes: u8,
    pub hires: bool,
    pub ham: bool,
}

/// Prima riga PAL visualizzata (nell'immagine) e numero di righe.
const FIRST_LINE: u32 = 0x2C;
const LINES: u32 = 256;

pub fn render(bus: &Bus) -> Frame {
    let c = &bus.custom;
    let mut regs = c.regs;
    let copper_on = c.dmacon & 0x0280 == 0x0280;
    let bpl_on = c.dmacon & 0x0300 == 0x0300;

    let hires_any = regs[0x100 / 2] & 0x8000 != 0;
    let width: u32 = if hires_any { 640 } else { 320 };
    let mut rgb = vec![0u8; (width * LINES * 3) as usize];

    let mut cop = c.cop1lc;
    let mut cop_done = !copper_on || cop == 0;
    let mut moves = 0u32;
    let mut waiting: Option<(u16, u16)> = None; // (vp, maschera verticale)
    let mut active = 0u32;
    let mut ptrs = [0u32; 6];
    for (i, p) in ptrs.iter_mut().enumerate() {
        *p = long(&regs, 0xE0 + i as u16 * 4);
    }
    let mut max_planes = 0u8;
    let mut ham_seen = false;

    for y in 0..313u32 {
        // --- copper ---
        let mut guard = 0;
        while !cop_done && guard < 2000 {
            guard += 1;
            if let Some((vp, mask)) = waiting {
                let beam = (y & 0xFF) as u16;
                if (beam & mask) >= (vp & mask) {
                    waiting = None;
                } else {
                    break;
                }
            }
            let w1 = bus.peek16(cop);
            let w2 = bus.peek16(cop + 2);
            cop += 4;
            if w1 & 1 == 0 {
                // MOVE
                let reg = w1 & 0x1FE;
                regs[(reg / 2) as usize] = w2;
                moves += 1;
                if (0xE0..0xF8).contains(&reg) {
                    let i = ((reg - 0xE0) / 4) as usize;
                    ptrs[i] = long(&regs, 0xE0 + i as u16 * 4);
                }
                if reg == 0x88 {
                    cop = long(&regs, 0x80); // COPJMP1
                } else if reg == 0x8A {
                    cop = long(&regs, 0x84); // COPJMP2
                }
            } else if w2 & 1 == 0 {
                // WAIT
                if w1 == 0xFFFF && w2 == 0xFFFE {
                    cop_done = true;
                } else {
                    let vp = w1 >> 8;
                    let vmask = ((w2 >> 8) & 0x7F) | 0x80;
                    waiting = Some((vp, vmask));
                }
            } else {
                // SKIP: ignorato (raro)
            }
        }

        // --- bitplane ---
        if !(FIRST_LINE..FIRST_LINE + LINES).contains(&y) {
            continue;
        }
        let out_y = y - FIRST_LINE;
        let con0 = regs[0x100 / 2];
        let planes = ((con0 >> 12) & 7) as usize;
        let hires = con0 & 0x8000 != 0;
        let ham = con0 & 0x0800 != 0;
        ham_seen |= ham && planes > 0;
        let diwstrt = regs[0x8E / 2];
        let diwstop = regs[0x90 / 2];
        let vstart = (diwstrt >> 8) as u32;
        let mut vstop = (diwstop >> 8) as u32;
        if vstop & 0x80 == 0 {
            vstop += 256;
        }
        let colors: Vec<[u8; 3]> = (0..32)
            .map(|i| rgb12(regs[(0x180 / 2 + i) as usize]))
            .collect();
        let row = (out_y * width * 3) as usize;
        let in_window = y >= vstart && y < vstop;
        // Sfondo: COLOR00
        for x in 0..width as usize {
            rgb[row + x * 3..row + x * 3 + 3].copy_from_slice(&colors[0]);
        }
        if !bpl_on || planes == 0 || !in_window {
            continue;
        }
        active += 1;
        max_planes = max_planes.max(planes as u8);
        let ddfstrt = regs[0x92 / 2] & 0xFC;
        let ddfstop = regs[0x94 / 2] & 0xFC;
        let words = if hires {
            ((ddfstop.saturating_sub(ddfstrt)) >> 2) as u32 + 2
        } else {
            ((ddfstop.saturating_sub(ddfstrt)) >> 3) as u32 + 1
        };
        let mod1 = regs[0x108 / 2] as i16 as i32;
        let mod2 = regs[0x10A / 2] as i16 as i32;
        // Posizione orizzontale di inizio della finestra rispetto al primo fetch.
        let hstart = (diwstrt & 0xFF) as i32;
        let fetch_start_px = if hires {
            (ddfstrt as i32 * 2 + 17) * 2
        } else {
            ddfstrt as i32 * 2 + 17
        };
        let skip = if hires {
            (hstart * 2) - fetch_start_px
        } else {
            hstart - fetch_start_px
        };
        let px_per_line = words * 16;
        let scale = if hires || !hires_any { 1 } else { 2 };
        for px in 0..px_per_line {
            let w = px / 16;
            let bit = 15 - (px % 16);
            let mut idx = 0usize;
            for p in 0..planes {
                let word = bus.peek16(ptrs[p] + w * 2);
                if word & (1 << bit) != 0 {
                    idx |= 1 << p;
                }
            }
            let ox = px as i32 - skip.max(0);
            if ox < 0 {
                continue;
            }
            let color = if planes == 6 && !ham && idx >= 32 {
                let c0 = colors[idx - 32];
                [c0[0] / 2, c0[1] / 2, c0[2] / 2]
            } else {
                colors[idx & 31]
            };
            for s in 0..scale {
                let x = (ox as u32) * scale + s;
                if x < width {
                    let o = row + x as usize * 3;
                    rgb[o..o + 3].copy_from_slice(&color);
                }
            }
        }
        for p in 0..planes {
            let m = if p % 2 == 0 { mod1 } else { mod2 };
            ptrs[p] = (ptrs[p] as i64 + (words * 2) as i64 + m as i64) as u32;
        }
    }

    Frame {
        width,
        height: LINES,
        rgb,
        active_lines: active,
        copper_moves: moves,
        planes: max_planes,
        hires: hires_any,
        ham: ham_seen,
    }
}

fn long(regs: &[u16; 256], off: u16) -> u32 {
    ((regs[(off / 2) as usize] as u32) << 16) | regs[(off / 2 + 1) as usize] as u32
}

fn rgb12(v: u16) -> [u8; 3] {
    [
        ((v >> 8) & 0xF) as u8 * 17,
        ((v >> 4) & 0xF) as u8 * 17,
        (v & 0xF) as u8 * 17,
    ]
}

pub fn save_png(frame: &Frame, path: &std::path::Path) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let w = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(w, frame.width, frame.height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(std::io::Error::other)?;
    writer
        .write_image_data(&frame.rgb)
        .map_err(std::io::Error::other)?;
    Ok(())
}
