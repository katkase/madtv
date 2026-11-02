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

/// Frame che la copia aspetta, dopo il click, perché il gioco ridisegni
/// l'oggetto al suo posto (nelle news servono ~40 frame).
const LANDING_FRAMES: u32 = 70;
/// Frame di immobilità del punto caldo prima di cercare il punto d'arrivo.
const LANDING_STILL: u64 = 5;

/// Pixel dell'oggetto trascinato presi dallo schermo, per ritrovarlo dopo il
/// rilascio. Campioni a griglia all'interno del rettangolo, lontano dal
/// puntatore (che è uno sprite sopra l'immagine).
pub fn panel_samples(m: &Machine, d: &Drag) -> Vec<(i32, i32, u32)> {
    let src = &m.bus.custom.display.front;
    let ptr = m.game_pointer().unwrap_or((-100, -100));
    let mut v = Vec::new();
    for yy in (2..d.size.1 - 2).step_by(3) {
        for xx in (2..d.size.0 - 2).step_by(3) {
            let (x, y) = (d.hot.0 + xx, d.hot.1 + yy);
            if !(0..320).contains(&x) || !(0..200).contains(&y) {
                continue;
            }
            if (x - ptr.0 + 4).abs() < 20 && (y - ptr.1 + 4).abs() < 20 {
                continue;
            }
            v.push((xx, yy, src[(y * 320 + x) as usize]));
        }
    }
    v
}

/// Cerca nello schermo la posizione in cui i campioni coincidono (≥ 95%).
pub fn find_panel(
    screen: &[u32],
    size: (i32, i32),
    samples: &[(i32, i32, u32)],
) -> Option<(i32, i32)> {
    if samples.len() < 20 {
        return None;
    }
    let max_miss = samples.len() / 20;
    for y in 0..=(200 - size.1).max(0) {
        'x: for x in 0..=(320 - size.0).max(0) {
            let mut miss = 0;
            for &(xx, yy, c) in samples {
                if screen[((y + yy) * 320 + x + xx) as usize] != c {
                    miss += 1;
                    if miss > max_miss {
                        continue 'x;
                    }
                }
            }
            return Some((x, y));
        }
    }
    None
}

/// Click di prova lungo: dove finirà l'oggetto se rilasciato adesso?
/// Restituisce il rettangolo in cui il gioco lo ridisegna, se lo ridisegna
/// uguale (news, agenzie); None se sparisce (valigetta) o cambia aspetto
/// (palinsesto). Da eseguire fuori dal ciclo della finestra: costa ~100 ms.
pub fn probe_landing(
    mut c: Machine,
    d: Drag,
    samples: Vec<(i32, i32, u32)>,
) -> Option<(i32, i32, i32, i32)> {
    c.bus.custom.paula.enabled = false;
    c.bus.custom.left_button = true;
    let mut released = false;
    for _ in 0..PROBE_FRAMES {
        if c.run_frame().is_some() {
            return None;
        }
        if drag_state(&c).map(|e| e.image) != Some(d.image) {
            released = true;
            break;
        }
    }
    if !released {
        return None;
    }
    c.bus.custom.left_button = false;
    for i in 1..=LANDING_FRAMES {
        if c.run_frame().is_some() {
            return None;
        }
        if i % 5 == 0 {
            if let Some((x, y)) = find_panel(&c.bus.custom.display.front, d.size, &samples) {
                return Some((x, y, d.size.0, d.size.1));
            }
        }
    }
    None
}

/// Lavoro per il thread dei click di prova lunghi.
struct LandingJob {
    machine: Machine,
    drag: Drag,
    samples: Vec<(i32, i32, u32)>,
}

/// Thread che esegue i click di prova lunghi senza rallentare la finestra.
struct LandingWorker {
    jobs: std::sync::mpsc::Sender<LandingJob>,
    results: std::sync::mpsc::Receiver<(Drag, Option<(i32, i32, i32, i32)>)>,
    busy: bool,
}

impl LandingWorker {
    fn start() -> LandingWorker {
        let (jt, jr) = std::sync::mpsc::channel::<LandingJob>();
        let (rt, rr) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for j in jr {
                let r = probe_landing(j.machine, j.drag, j.samples);
                if rt.send((j.drag, r)).is_err() {
                    break;
                }
            }
        });
        LandingWorker {
            jobs: jt,
            results: rr,
            busy: false,
        }
    }
}

/// Colori (0xAARRGGBB).
const ACCEPT: u32 = 0xFF38D870;
const REFUSE: u32 = 0xFFE84848;
const UNKNOWN: u32 = 0xFFF0C838;

#[derive(Default)]
pub struct Enhancer {
    /// Disegna le informazioni diagnostiche.
    pub debug: bool,
    /// Evidenziazioni attive (con `--dev`; F9 le alterna).
    pub highlights: bool,
    /// Ultimo click di prova: (immagine, punto caldo, esito, frame).
    probe: Option<(u32, (i32, i32), bool, u64)>,
    /// Statistiche: click di prova eseguiti e tempo totale.
    pub probes: u64,
    pub probe_time: std::time::Duration,
    /// Punto caldo attuale e frame da cui non si muove.
    still: Option<((i32, i32), u64)>,
    /// Punto d'arrivo trovato dal click di prova lungo: (immagine, punto caldo
    /// della prova, rettangolo).
    landing: Option<(u32, (i32, i32), (i32, i32, i32, i32))>,
    /// Ultimo punto caldo provato con il click lungo (per non ripeterlo).
    landing_tried: Option<(u32, (i32, i32))>,
    worker: Option<LandingWorker>,
    /// Click di prova lunghi eseguiti (statistica).
    pub landing_probes: u64,
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

    /// Area d'arrivo dell'oggetto (fuori dal palinsesto): la cerca in un thread
    /// quando il punto caldo resta fermo su un punto accettato, e la mostra
    /// finché il punto caldo ci resta dentro.
    fn landing(
        &mut self,
        m: &Machine,
        d: &Drag,
        verdict: Option<bool>,
    ) -> Option<(i32, i32, i32, i32)> {
        let frame = m.bus.custom.frame;
        match self.still {
            Some((h, _)) if h == d.hot => {}
            _ => self.still = Some((d.hot, frame)),
        }
        if let Some(w) = &mut self.worker {
            while let Ok((jd, r)) = w.results.try_recv() {
                w.busy = false;
                if let Some(rect) = r {
                    self.landing = Some((jd.image, jd.hot, rect));
                }
            }
        }
        if d.in_schedule() {
            return None;
        }
        let inside = |r: (i32, i32, i32, i32)| {
            d.hot.0 >= r.0 - 4 && d.hot.0 < r.0 + r.2 && d.hot.1 >= r.1 - 4 && d.hot.1 < r.1 + r.3
        };
        if let Some((img, _, r)) = self.landing {
            if img != d.image || !inside(r) {
                self.landing = None;
            }
        }
        let still_long = self.still.is_some_and(|(_, f)| frame >= f + LANDING_STILL);
        let tried = self.landing_tried == Some((d.image, d.hot));
        if verdict == Some(true) && still_long && !tried && self.landing.is_none() {
            let w = self.worker.get_or_insert_with(LandingWorker::start);
            if !w.busy {
                let job = LandingJob {
                    machine: m.clone(),
                    drag: *d,
                    samples: panel_samples(m, d),
                };
                if w.jobs.send(job).is_ok() {
                    w.busy = true;
                    self.landing_tried = Some((d.image, d.hot));
                    self.landing_probes += 1;
                }
            }
        }
        if verdict == Some(true) {
            self.landing.map(|l| l.2)
        } else {
            None
        }
    }

    /// Forme da disegnare sopra il frame corrente.
    pub fn update(&mut self, m: &Machine) -> Vec<Shape> {
        let mut out = Vec::new();
        if self.highlights {
            match drag_state(m) {
                Some(d) if d.drawn => {
                    let v = self.verdict(m, &d);
                    let target = self.landing(m, &d, v);
                    drag_shapes(&d, v, target, &mut out);
                }
                Some(_) => {}
                None => {
                    self.probe = None;
                    self.landing = None;
                    self.landing_tried = None;
                    self.still = None;
                }
            }
        }
        if self.debug {
            debug_shapes(m, &mut out);
        }
        out
    }
}

/// Evidenziazioni del trascinamento.
pub fn drag_shapes(
    d: &Drag,
    verdict: Option<bool>,
    target: Option<(i32, i32, i32, i32)>,
    out: &mut Vec<Shape>,
) {
    let color = match verdict {
        Some(true) => ACCEPT,
        Some(false) => REFUSE,
        None => UNKNOWN,
    };
    let (hx, hy) = (d.hot.0 as f32, d.hot.1 as f32);
    // Area che riceverebbe l'oggetto: casella del palinsesto o punto d'arrivo
    // trovato dal click di prova lungo. Riempimento e contorno saltano il
    // rettangolo dell'oggetto (maschera), che così resta "sopra".
    if verdict != Some(false) {
        if let Some((x, y, w, h)) = d.schedule_cell().or(target) {
            let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
            out.push(Shape::Mask {
                x: d.hot.0 as f32,
                y: d.hot.1 as f32,
                w: d.size.0 as f32,
                h: d.size.1 as f32,
            });
            out.push(Shape::Fill {
                x,
                y,
                w,
                h,
                color: (color & 0xFF_FFFF) | 0x50 << 24,
            });
            out.push(Shape::Rect {
                x,
                y,
                w,
                h,
                color,
                thickness: 2,
            });
            out.push(Shape::Unmask);
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

/// Parti di `a` fuori da `b` (al più 4 rettangoli), formato (x, y, w, h).
pub fn subtract(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> Vec<(i32, i32, i32, i32)> {
    let (ax1, ay1) = (a.0 + a.2, a.1 + a.3);
    let (bx0, by0) = (b.0.max(a.0), b.1.max(a.1));
    let (bx1, by1) = ((b.0 + b.2).min(ax1), (b.1 + b.3).min(ay1));
    if bx0 >= bx1 || by0 >= by1 {
        return vec![a];
    }
    let mut v = Vec::new();
    if by0 > a.1 {
        v.push((a.0, a.1, a.2, by0 - a.1));
    }
    if ay1 > by1 {
        v.push((a.0, by1, a.2, ay1 - by1));
    }
    if bx0 > a.0 {
        v.push((a.0, by0, bx0 - a.0, by1 - by0));
    }
    if ax1 > bx1 {
        v.push((bx1, by0, ax1 - bx1, by1 - by0));
    }
    v
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
    fn sottrazione_di_rettangoli() {
        let area = |v: &[(i32, i32, i32, i32)]| v.iter().map(|r| r.2 * r.3).sum::<i32>();
        // Pannello che copre in parte la casella: resta casella − intersezione.
        let r = subtract((72, 68, 144, 16), (83, 70, 144, 16));
        assert_eq!(area(&r), 144 * 16 - (216 - 83) * 14);
        // Nessuna intersezione: la casella intera.
        assert_eq!(
            subtract((0, 0, 10, 10), (20, 20, 5, 5)),
            vec![(0, 0, 10, 10)]
        );
        // Copertura totale: niente.
        assert!(subtract((5, 5, 10, 10), (0, 0, 30, 30)).is_empty());
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
