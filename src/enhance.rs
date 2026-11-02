//! Miglioramenti dell'interfaccia sopra il gioco originale.
//!
//! Ogni frame `Enhancer::update` legge lo stato del gioco dalla memoria emulata
//! e produce le forme da disegnare nello strato di sovrapposizione (vedi
//! `overlay.rs`). Il gioco non viene modificato: le evidenziazioni si limitano
//! a mostrare meglio quello che il gioco farà.
//!
//! ## Trascinamento
//!
//! In tutte le stanze (palinsesto, Movie Agency, agenzia sceneggiature, news,
//! studio) il gioco usa la stessa routine per "agganciare" un oggetto al mouse
//! (`$030D5A`) e la stessa per sganciarlo (`$031016`). Le sue variabili, nel
//! segmento dati (segmento 2, base `$044610`):
//!
//! | offset | indirizzo | significato |
//! |---|---|---|
//! | `+0x2DC` | `$0448EC` | immagine dell'oggetto agganciato (0 = niente in mano) |
//! | `+0x2E0` | `$0448F0` | larghezza in pixel |
//! | `+0x2E2` | `$0448F2` | altezza in pixel |
//! | `+0x2E4` | `$0448F4` | distanza orizzontale puntatore − angolo dell'oggetto |
//! | `+0x2E6` | `$0448F6` | distanza verticale |
//!
//! Scoperta importante: mentre si trascina, per decidere dove si rilascia il
//! gioco **non usa la punta del puntatore ma l'angolo in alto a sinistra
//! dell'oggetto** (funzioni `$0179BC`/`$0179FC`: puntatore − distanza). È il
//! "punto caldo" che segniamo sullo schermo.
//!
//! ## Click di prova
//!
//! Le regole che decidono se un rilascio è accettato cambiano da stanza a
//! stanza (nel palinsesto contano l'ora corrente e i blocchi occupati, nella
//! Movie Agency la valigetta, ecc.). Invece di riscriverle, le chiediamo al
//! gioco stesso: cloniamo la macchina emulata (`Machine::clone`, ~2 ms),
//! premiamo il tasto sinistro sulla copia e guardiamo se dopo pochi frame
//! l'oggetto è stato sganciato. La copia ha il file system in sola lettura e
//! viene buttata subito: il gioco vero non si accorge di nulla.

use crate::machine::Machine;
use crate::overlay::Shape;

/// Offset delle variabili del trascinamento nel segmento dati del gioco.
const OFF_HELD: u32 = 0x2DC;
const OFF_W: u32 = 0x2E0;
const OFF_H: u32 = 0x2E2;
const OFF_DX: u32 = 0x2E4;
const OFF_DY: u32 = 0x2E6;
/// Posizione del puntatore secondo il gioco.
const OFF_PTR: u32 = 0x40;
/// Ascissa a cui il gioco ha disegnato l'oggetto (`$044656`); -1 finché non
/// l'ha ancora disegnato (i primi frame dopo l'aggancio).
const OFF_DRAWN_X: u32 = 0x46;
/// Zona dello schermo sotto il punto caldo (`$04E962`); 0x0F = lista del palinsesto.
const OFF_ZONE: u32 = 0xA352;
const ZONE_SCHEDULE_LIST: u16 = 0x0F;

/// Frame massimi che la copia aspetta perché il gioco reagisca al click.
const PROBE_FRAMES: u32 = 6;
/// Frame minimi tra due click di prova.
const PROBE_INTERVAL: u64 = 3;

/// Oggetto agganciato al mouse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    /// Indirizzo dell'immagine dell'oggetto (identifica il trascinamento).
    pub image: u32,
    /// Punto caldo = angolo in alto a sinistra dell'oggetto, in pixel del gioco.
    pub hot: (i32, i32),
    pub size: (i32, i32),
    /// Zona dello schermo sotto il punto caldo, secondo il gioco.
    pub zone: u16,
    /// Il gioco ha già disegnato l'oggetto sullo schermo.
    pub drawn: bool,
}

impl Drag {
    /// Siamo nel palinsesto (pannello 144×16 sopra la lista dei programmi)?
    pub fn in_schedule(&self) -> bool {
        self.size == (144, 16) && self.zone == ZONE_SCHEDULE_LIST
    }

    /// Casella del palinsesto che riceverebbe il pannello: stessa formula del
    /// gioco (`$006724`: riga = (y − 4) / 16), colonne dei programmi
    /// (x 56..215, casella da 72) e della pubblicità (x 216..279).
    pub fn schedule_cell(&self) -> Option<(i32, i32, i32, i32)> {
        if !self.in_schedule() {
            return None;
        }
        let (x, y) = self.hot;
        if y < 4 {
            return None;
        }
        let row = (y - 4) / 16;
        let y0 = 4 + row * 16;
        if y0 + 16 > 116 {
            return None;
        }
        match x {
            56..=215 => Some((72, y0, 144, 16)),
            216..=279 => Some((216, y0, 64, 16)),
            _ => None,
        }
    }
}

/// Legge lo stato del trascinamento; None se non c'è niente in mano.
pub fn drag_state(m: &Machine) -> Option<Drag> {
    let (base, size, _) = *m.segments.get(2)?;
    if size < OFF_ZONE + 2 {
        return None;
    }
    let image = m.bus.peek32(base + OFF_HELD);
    if image == 0 {
        return None;
    }
    let w = |o: u32| m.bus.peek16(base + o) as i16 as i32;
    let (px, py) = (w(OFF_PTR), w(OFF_PTR + 2));
    Some(Drag {
        image,
        hot: (px - w(OFF_DX), py - w(OFF_DY)),
        size: (w(OFF_W), w(OFF_H)),
        zone: m.bus.peek16(base + OFF_ZONE),
        drawn: w(OFF_DRAWN_X) != -1,
    })
}

/// Click di prova: il gioco accetterebbe il rilascio adesso?
pub fn probe_drop(m: &Machine) -> bool {
    let Some(d) = drag_state(m) else {
        return false;
    };
    let mut c = m.clone();
    c.bus.custom.paula.enabled = false;
    c.bus.custom.left_button = true;
    for _ in 0..PROBE_FRAMES {
        if c.run_frame().is_some() {
            return false;
        }
        match drag_state(&c) {
            None => return true,
            Some(e) if e.image != d.image => return true,
            _ => {}
        }
    }
    false
}

/// Colori (0xAARRGGBB).
const ACCEPT: u32 = 0xFF38D870;
const REFUSE: u32 = 0xFFE84848;
const UNKNOWN: u32 = 0xFFF0C838;

#[derive(Default)]
pub struct Enhancer {
    /// Disegna le informazioni diagnostiche.
    pub debug: bool,
    /// Evidenziazioni attive (disattivate da `--originale` o con F9).
    pub highlights: bool,
    /// Ultimo click di prova: (immagine, punto caldo, esito, frame).
    probe: Option<(u32, (i32, i32), bool, u64)>,
    /// Statistiche: click di prova eseguiti e tempo totale.
    pub probes: u64,
    pub probe_time: std::time::Duration,
}

impl Enhancer {
    pub fn new(highlights: bool) -> Enhancer {
        Enhancer {
            highlights,
            ..Default::default()
        }
    }

    /// Esito del rilascio nel punto attuale: Some(true/false) se noto.
    fn verdict(&mut self, m: &Machine, d: &Drag) -> Option<bool> {
        let frame = m.bus.custom.frame;
        let stale = match self.probe {
            Some((img, hot, _, _)) => img != d.image || hot != d.hot,
            None => true,
        };
        let due = match self.probe {
            Some((img, _, _, f)) => img != d.image || frame >= f + PROBE_INTERVAL,
            None => true,
        };
        if stale && due {
            let t = std::time::Instant::now();
            let ok = probe_drop(m);
            self.probe_time += t.elapsed();
            self.probes += 1;
            self.probe = Some((d.image, d.hot, ok, frame));
        }
        match self.probe {
            Some((img, _, ok, _)) if img == d.image => Some(ok),
            _ => None,
        }
    }

    /// Forme da disegnare sopra il frame corrente.
    pub fn update(&mut self, m: &Machine) -> Vec<Shape> {
        let mut out = Vec::new();
        if self.highlights {
            match drag_state(m) {
                Some(d) if d.drawn => {
                    let v = self.verdict(m, &d);
                    drag_shapes(&d, v, &mut out);
                }
                Some(_) => {}
                None => self.probe = None,
            }
        }
        if self.debug {
            debug_shapes(m, &mut out);
        }
        out
    }
}

/// Evidenziazioni del trascinamento.
pub fn drag_shapes(d: &Drag, verdict: Option<bool>, out: &mut Vec<Shape>) {
    let color = match verdict {
        Some(true) => ACCEPT,
        Some(false) => REFUSE,
        None => UNKNOWN,
    };
    let (hx, hy) = (d.hot.0 as f32, d.hot.1 as f32);
    // Casella del palinsesto che riceverebbe il pannello.
    if verdict != Some(false) {
        if let Some((x, y, w, h)) = d.schedule_cell() {
            let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
            out.push(Shape::Fill {
                x,
                y,
                w,
                h,
                color: (color & 0xFF_FFFF) | 0x40 << 24,
            });
            out.push(Shape::Rect {
                x,
                y,
                w,
                h,
                color,
                thickness: 2,
            });
        }
    }
    // Contorno dell'oggetto trascinato.
    out.push(Shape::Rect {
        x: hx - 1.0,
        y: hy - 1.0,
        w: d.size.0 as f32 + 2.0,
        h: d.size.1 as f32 + 2.0,
        color: (color & 0xFF_FFFF) | 0xE0 << 24,
        thickness: 2,
    });
    // Punto caldo: il pixel che il gioco usa per decidere dove si rilascia.
    out.push(Shape::Fill {
        x: hx - 2.0,
        y: hy - 2.0,
        w: 4.0,
        h: 4.0,
        color: 0xFF000000,
    });
    out.push(Shape::Fill {
        x: hx - 1.5,
        y: hy - 1.5,
        w: 3.0,
        h: 3.0,
        color,
    });
}

fn debug_shapes(m: &Machine, out: &mut Vec<Shape>) {
    let Some(g) = m.game_mouse() else {
        return;
    };
    let (x, y) = (g.pos.0 as f32, g.pos.1 as f32);
    let green = 0xC000FF00;
    // Mirino sulla punta del puntatore (il pixel (x, y) del gioco).
    out.push(Shape::Rect {
        x,
        y,
        w: 1.0,
        h: 1.0,
        color: 0xFF00FF00,
        thickness: 1,
    });
    for (x0, y0, x1, y1) in [
        (x - 6.0, y + 0.5, x - 2.0, y + 0.5),
        (x + 3.0, y + 0.5, x + 7.0, y + 0.5),
        (x + 0.5, y - 6.0, x + 0.5, y - 2.0),
        (x + 0.5, y + 3.0, x + 0.5, y + 7.0),
    ] {
        out.push(Shape::Line {
            x0,
            y0,
            x1,
            y1,
            color: green,
            thickness: 1,
        });
    }
    // Area in cui il gioco lascia stare il puntatore in questa schermata.
    out.push(Shape::Rect {
        x: g.min.0 as f32,
        y: g.min.1 as f32,
        w: (g.max.0 - g.min.0 + 1) as f32,
        h: (g.max.1 - g.min.1 + 1) as f32,
        color: 0x6000FFFF,
        thickness: 1,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(hot: (i32, i32), size: (i32, i32), zone: u16) -> Drag {
        Drag {
            image: 1,
            hot,
            size,
            zone,
            drawn: true,
        }
    }

    #[test]
    fn casella_del_palinsesto() {
        // Rilascio reale della registrazione: angolo (83,70) → riga delle 22:00.
        assert_eq!(
            drag((83, 70), (144, 16), 0x0F).schedule_cell(),
            Some((72, 68, 144, 16))
        );
        assert_eq!(
            drag((150, 4), (144, 16), 0x0F).schedule_cell(),
            Some((72, 4, 144, 16))
        );
        assert_eq!(
            drag((230, 20), (144, 16), 0x0F).schedule_cell(),
            Some((216, 20, 64, 16))
        );
        // Fuori dalla lista o in un'altra stanza: niente casella.
        assert_eq!(drag((30, 70), (144, 16), 0x0F).schedule_cell(), None);
        assert_eq!(drag((83, 70), (144, 16), 0x18).schedule_cell(), None);
        assert_eq!(drag((83, 70), (16, 29), 0x0F).schedule_cell(), None);
    }
}
