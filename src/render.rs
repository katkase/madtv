//! Display OCS (PAL), disegnato riga per riga durante l'emulazione.
//!
//! A ogni riga di scansione `Custom` esegue prima il copper e poi chiama
//! [`Display::line`], che:
//! - legge i bitplane dagli indirizzi correnti BPLxPT (i registri veri);
//! - fa avanzare i puntatori di (parole lette × 2 + modulo), come fa il DMA
//!   dei bitplane sull'hardware, così le bitmap interleaved e i cambi di
//!   puntatore a metà schermo funzionano senza casi speciali;
//! - converte i pixel con la palette in vigore in quel momento.
//!
//! L'immagine è 320×256 (risoluzione lores, righe PAL da $2C a $12B); di
//! default se ne mostrano le prime 200, le sole che Mad TV usa. Le righe
//! hires vengono ridotte prendendo un pixel ogni due (Mad TV è tutto lores).
//! Sprite: gli 8 sprite hardware sono letti via DMA (POS/CTL e coppie di
//! parole dati) e disegnati sopra il playfield, anche collegati a 15 colori.
//! Mancano ancora: priorità sprite/playfield (BPLCON2), scroll fine
//! (BPLCON1), dual playfield, HAM. L'Extra Half-Brite è supportato.

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 256;
/// Righe mostrate di default: Mad TV usa una finestra video di 200 righe
/// (DIWSTRT/DIWSTOP da $2C a $F4) in tutte le schermate; le altre sono nere.
pub const GAME_HEIGHT: usize = 200;
/// Prima riga PAL mostrata (il valore standard di DIWSTRT).
pub const FIRST_LINE: u32 = 0x2C;

/// Stato del DMA di uno sprite.
#[derive(Clone, Copy, Default)]
struct Sprite {
    /// Intestazione (POS/CTL) già letta per il blocco corrente.
    armed: bool,
    /// Lista finita per questo frame (POS = CTL = 0).
    done: bool,
    vstart: u32,
    vstop: u32,
    hstart: u32,
    attached: bool,
    /// Dati della riga corrente (A = bit 0 del colore, B = bit 1).
    data_a: u16,
    data_b: u16,
    visible_now: bool,
}

#[derive(Clone)]
pub struct Display {
    sprites: [Sprite; 8],
    /// Frame in costruzione e ultimo frame completo, in formato 0x00RRGGBB.
    back: Vec<u32>,
    pub front: Vec<u32>,
    pub frames: u64,
    /// Diagnostica dell'ultimo frame completo.
    pub last_active_lines: u32,
    pub last_planes: u8,
    active_lines: u32,
    planes: u8,
    pub ham_seen: bool,
    pub hires_seen: bool,
    /// Prima e ultima riga dell'immagine (0..256) in cui c'è stato qualcosa da
    /// disegnare (bitplane attivi), su tutta la sessione.
    pub used_rows: Option<(usize, usize)>,
    /// Righe da mostrare e salvare negli screenshot (200 o 256).
    pub visible_rows: usize,
}

impl Default for Display {
    fn default() -> Self {
        Self::new()
    }
}

impl Display {
    pub fn new() -> Display {
        Display {
            sprites: [Sprite::default(); 8],
            back: vec![0; WIDTH * HEIGHT],
            front: vec![0; WIDTH * HEIGHT],
            frames: 0,
            last_active_lines: 0,
            last_planes: 0,
            active_lines: 0,
            planes: 0,
            ham_seen: false,
            hires_seen: false,
            used_rows: None,
            visible_rows: GAME_HEIGHT,
        }
    }

    /// Chiamata all'inizio di ogni frame: il frame appena finito diventa visibile.
    pub fn end_frame(&mut self) {
        std::mem::swap(&mut self.front, &mut self.back);
        self.last_active_lines = self.active_lines;
        self.last_planes = self.planes;
        self.active_lines = 0;
        self.planes = 0;
        self.frames += 1;
    }

    /// Il programma (o il copper) ha scritto il puntatore dello sprite `n`:
    /// il DMA riparte leggendo una nuova intestazione.
    pub fn sprite_pointer_written(&mut self, n: usize) {
        self.sprites[n & 7] = Sprite::default();
    }

    /// DMA degli sprite per la riga `y`: legge intestazioni e dati e fa avanzare SPRxPT.
    fn sprite_dma(&mut self, y: u32, regs: &mut [u16; 256], ram: &[u8]) {
        let rd = |a: u32| -> u16 {
            let a = (a & 0x1F_FFFE) as usize;
            if a + 1 < ram.len() {
                u16::from_be_bytes([ram[a], ram[a + 1]])
            } else {
                0
            }
        };
        for (n, sp) in self.sprites.iter_mut().enumerate() {
            sp.visible_now = false;
            if sp.done {
                continue;
            }
            let reg = 0x120 / 2 + n * 2;
            let mut ptr = ((regs[reg] as u32) << 16) | regs[reg + 1] as u32;
            if ptr == 0 {
                continue;
            }
            if !sp.armed {
                // Il DMA degli sprite inizia dopo il vertical blank (riga $19 circa).
                if y < 0x19 {
                    continue;
                }
                let pos = rd(ptr);
                let ctl = rd(ptr + 2);
                ptr += 4;
                regs[0x140 / 2 + n * 4] = pos;
                regs[0x142 / 2 + n * 4] = ctl;
                if pos == 0 && ctl == 0 {
                    sp.done = true;
                } else {
                    sp.armed = true;
                    sp.vstart = (pos >> 8) as u32 | (((ctl >> 2) & 1) as u32) << 8;
                    sp.vstop = (ctl >> 8) as u32 | (((ctl >> 1) & 1) as u32) << 8;
                    sp.hstart = ((pos & 0xFF) as u32) << 1 | (ctl & 1) as u32;
                    sp.attached = ctl & 0x80 != 0;
                }
            } else if y >= sp.vstart && y < sp.vstop {
                sp.data_a = rd(ptr);
                sp.data_b = rd(ptr + 2);
                ptr += 4;
                sp.visible_now = true;
                if y + 1 == sp.vstop {
                    sp.armed = false; // la riga dopo si legge la prossima intestazione
                }
            } else if y >= sp.vstop {
                sp.armed = false;
            }
            regs[reg] = (ptr >> 16) as u16;
            regs[reg + 1] = ptr as u16;
        }
    }

    /// Disegna gli sprite visibili su questa riga sopra i pixel già disegnati.
    fn draw_sprites(&mut self, row: usize, regs: &[u16; 256]) {
        let diw_h = (regs[0x8E / 2] & 0xFF) as i32;
        let out = &mut self.back[row..row + WIDTH];
        // Dal più basso al più alto in priorità (lo sprite 0 vince su tutti).
        for n in (0..8).rev() {
            let sp = self.sprites[n];
            if !sp.visible_now {
                continue;
            }
            // Uno sprite dispari collegato viene disegnato insieme al pari precedente.
            if n % 2 == 1 && sp.attached {
                continue;
            }
            let partner =
                if n % 2 == 0 && self.sprites[n + 1].attached && self.sprites[n + 1].visible_now {
                    Some(self.sprites[n + 1])
                } else {
                    None
                };
            let x0 = sp.hstart as i32 - diw_h + 1;
            for bit in 0..16 {
                let x = x0 + bit;
                if x < 0 || x >= WIDTH as i32 {
                    continue;
                }
                let m = 15 - bit as u16;
                let lo = ((sp.data_a >> m) & 1) | (((sp.data_b >> m) & 1) << 1);
                let color = if let Some(o) = partner {
                    let hi = ((o.data_a >> m) & 1) | (((o.data_b >> m) & 1) << 1);
                    let idx = (hi << 2) | lo;
                    if idx == 0 {
                        continue;
                    }
                    16 + idx as usize
                } else {
                    if lo == 0 {
                        continue;
                    }
                    16 + (n / 2) * 4 + lo as usize
                };
                out[x as usize] = rgb12(regs[0x180 / 2 + color]);
            }
        }
    }

    /// Disegna la riga `y` (0..313) con lo stato attuale dei registri.
    /// `regs` è modificato: i puntatori dei bitplane avanzano.
    pub fn line(&mut self, y: u32, regs: &mut [u16; 256], dmacon: u16, ram: &[u8]) {
        let visible = (FIRST_LINE..FIRST_LINE + HEIGHT as u32).contains(&y);
        let con0 = regs[0x100 / 2];
        let planes = ((con0 >> 12) & 7) as usize;
        let hires = con0 & 0x8000 != 0;
        let ham = con0 & 0x0800 != 0;
        let diwstrt = regs[0x8E / 2];
        let diwstop = regs[0x90 / 2];
        let vstart = (diwstrt >> 8) as u32;
        let mut vstop = (diwstop >> 8) as u32;
        if vstop & 0x80 == 0 {
            vstop += 256;
        }
        let bpl_on = dmacon & 0x0300 == 0x0300;
        let in_window = y >= vstart && y < vstop;
        if y == 0 {
            for sp in self.sprites.iter_mut() {
                sp.armed = false;
                sp.done = false;
            }
        }
        let spr_on = dmacon & 0x0220 == 0x0220;
        if spr_on {
            self.sprite_dma(y, regs, ram);
        }

        let color0 = rgb12(regs[0x180 / 2]);
        let row = if visible {
            Some((y - FIRST_LINE) as usize * WIDTH)
        } else {
            None
        };
        if let Some(r) = row {
            self.back[r..r + WIDTH].fill(color0);
        }
        if !bpl_on || planes == 0 || !in_window {
            if let (Some(r), true) = (row, spr_on) {
                self.draw_sprites(r, regs);
            }
            return;
        }
        self.ham_seen |= ham;
        self.hires_seen |= hires;

        // Quante parole legge il DMA per riga (da DDFSTRT/DDFSTOP).
        let ddfstrt = regs[0x92 / 2] & 0xFC;
        let ddfstop = (regs[0x94 / 2] & 0xFC).max(ddfstrt);
        let words = if hires {
            ((ddfstop - ddfstrt) >> 2) as usize + 2
        } else {
            ((ddfstop - ddfstrt) >> 3) as usize + 1
        };
        let mod1 = regs[0x108 / 2] as i16 as i32;
        let mod2 = regs[0x10A / 2] as i16 as i32;

        // Lettura dei bitplane e avanzamento dei puntatori.
        let mut data = [[0u16; 48]; 6];
        let words = words.min(48);
        for (p, plane) in data.iter_mut().enumerate().take(planes) {
            let reg = 0xE0 / 2 + p * 2;
            let ptr = ((regs[reg] as u32) << 16) | regs[reg + 1] as u32;
            for (w, slot) in plane.iter_mut().enumerate().take(words) {
                let a = ((ptr + w as u32 * 2) & 0x1F_FFFE) as usize;
                *slot = if a + 1 < ram.len() {
                    u16::from_be_bytes([ram[a], ram[a + 1]])
                } else {
                    0
                };
            }
            let m = if p % 2 == 0 { mod1 } else { mod2 };
            let next = (ptr as i64 + words as i64 * 2 + m as i64) as u32 & 0x1F_FFFE;
            regs[reg] = (next >> 16) as u16;
            regs[reg + 1] = next as u16;
        }

        let Some(r) = row else { return };
        self.active_lines += 1;
        let ry = r / WIDTH;
        self.used_rows = Some(match self.used_rows {
            Some((a, b)) => (a.min(ry), b.max(ry)),
            None => (ry, ry),
        });
        self.planes = self.planes.max(planes as u8);

        let mut palette = [0u32; 64];
        for (i, c) in palette.iter_mut().enumerate().take(32) {
            *c = rgb12(regs[0x180 / 2 + i]);
        }
        for i in 32..64 {
            let c = palette[i - 32];
            palette[i] = (c >> 1) & 0x7F7F7F; // Extra Half-Brite: metà luminosità
        }

        // Allineamento orizzontale: il primo pixel della finestra (DIWSTRT) rispetto
        // al primo pixel letto (DDFSTRT). In lores un fetch a $38 con DIWSTRT $81
        // è allineato: pixel = 2*ddf + 17 (in unità lores).
        let hstart = (diwstrt & 0xFF) as i32;
        let first_px = if hires {
            (ddfstrt as i32 * 2 + 9) * 2
        } else {
            ddfstrt as i32 * 2 + 17
        };
        let skip = if hires {
            hstart * 2 - first_px
        } else {
            hstart - first_px
        };
        let total = words * 16;
        let step = if hires { 2 } else { 1 };
        let out = &mut self.back[r..r + WIDTH];
        let mut ox = 0usize;
        let mut px = skip.max(0) as usize;
        while px < total && ox < WIDTH {
            let w = px / 16;
            let bit = 15 - (px % 16);
            let mut idx = 0usize;
            for (p, plane) in data.iter().enumerate().take(planes) {
                idx |= (((plane[w] >> bit) & 1) as usize) << p;
            }
            out[ox] = if planes == 6 && !ham {
                palette[idx]
            } else {
                palette[idx & 31]
            };
            ox += 1;
            px += step;
        }
        if spr_on {
            self.draw_sprites(r, regs);
        }
    }
}

fn rgb12(v: u16) -> u32 {
    let r = ((v >> 8) & 0xF) as u32 * 17;
    let g = ((v >> 4) & 0xF) as u32 * 17;
    let b = (v & 0xF) as u32 * 17;
    (r << 16) | (g << 8) | b
}

impl Display {
    /// Salva l'ultimo frame completo in PNG (solo le righe visibili).
    pub fn save_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        save_png(&self.front[..WIDTH * self.visible_rows], path)
    }
}

/// Salva un'immagine larga WIDTH pixel (0x00RRGGBB) in PNG.
pub fn save_png(pixels: &[u32], path: &std::path::Path) -> std::io::Result<()> {
    let mut rgb = Vec::with_capacity(pixels.len() * 3);
    for p in pixels {
        rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    let file = std::fs::File::create(path)?;
    let w = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(w, WIDTH as u32, (pixels.len() / WIDTH) as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(std::io::Error::other)?;
    writer
        .write_image_data(&rgb)
        .map_err(std::io::Error::other)?;
    Ok(())
}
