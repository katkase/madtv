//! Finestra di gioco (crate `minifb`), audio e input.
//!
//! La finestra resta aperta per tutta la sessione ([`Frontend`]) e può
//! eseguire in sequenza più programmi emulati (l'intro `rai`, poi il gioco) e
//! mostrare immagini (la schermata dei credits), come faceva la
//! startup-sequence del disco originale.
//!
//! Tasti speciali: F9 = evidenziazioni sì/no (solo con --dev), F10 = turbo automatico sì/no, F11 (tenuto premuto) = avanti
//! veloce, F12 = screenshot. Durante l'intro un click o un tasto la salta.
//!
//! Il gioco disegna da sé il puntatore del mouse, quindi il cursore di sistema
//! viene nascosto. I movimenti del mouse del PC vengono convertiti in conteggi
//! del mouse Amiga (vedi `input.rs`). Quando la posizione del puntatore del
//! gioco è leggibile (`Machine::game_mouse`) l'allineamento è assoluto.

use crate::audio::AudioOut;
use crate::enhance::Enhancer;
use crate::input;
use crate::machine::{Machine, StopReason};
use crate::overlay::{Canvas, Shape};
use crate::render::WIDTH;
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use std::path::PathBuf;

pub struct WindowOpts {
    pub scale: usize,
    pub shot_dir: PathBuf,
    /// Conteggi del mouse Amiga per pixel del gioco.
    pub mouse_ratio: f32,
    /// Uscita audio attiva.
    pub audio: bool,
    /// Righe del display mostrate (220 o 256): le 200 del gioco più quelle
    /// che finiscono nel margine inferiore.
    pub rows: usize,
    /// Margine intorno all'immagine, in pixel del gioco.
    pub margin: usize,
    /// Disegno su GPU (wgpu) invece che in software.
    pub gpu: bool,
}

/// Come è finita l'esecuzione di un programma nella finestra.
pub enum RunEnd {
    /// Il programma è terminato o si è fermato (con il motivo).
    Stopped(StopReason),
    /// L'utente ha saltato (intro).
    Skipped,
    /// La finestra è stata chiusa.
    Closed,
}

pub struct Frontend {
    /// Motore su GPU (`--gpu`). Dichiarato prima di `win` perché venga
    /// distrutto prima: la sua superficie usa la finestra.
    gpu: Option<crate::gpu::Gpu>,
    win: Window,
    audio: Option<AudioOut>,
    buf: Vec<u32>,
    scale: usize,
    rows: usize,
    margin: usize,
    shot_dir: PathBuf,
    mouse_ratio: f32,
    shots: u32,
    warned_rows: bool,
    /// Registrazione della partita (script riproducibile), se attiva.
    record: Option<std::io::BufWriter<std::fs::File>>,
    /// Stato dei tasti del mouse al frame precedente (per registrare i cambi).
    rec_buttons: (bool, bool),
    /// Miglioramenti dell'interfaccia disegnati nello strato di sovrapposizione.
    pub enhancer: Enhancer,
    /// Modalità sviluppo (`--dev`): strato di sovrapposizione ed evidenziazioni.
    pub dev: bool,
}

impl Frontend {
    pub fn new(o: &WindowOpts) -> Option<Frontend> {
        let s = o.scale.max(1);
        // Margine a sinistra, a destra e in alto; in basso il margine è occupato
        // dalle righe del display oltre la 200 (o.rows le comprende già).
        let (w, h) = ((WIDTH + 2 * o.margin) * s, (o.margin + o.rows) * s);
        let mut win = match Window::new("Mad TV", w, h, WindowOptions::default()) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("impossibile aprire la finestra: {e}");
                return None;
            }
        };
        win.set_cursor_visibility(false);
        let gpu = if o.gpu {
            let layout = crate::gpu::Layout {
                scale: s,
                margin: o.margin,
                rows: o.rows,
            };
            // SAFETY: `gpu` viene distrutto prima di `win` (ordine dei campi).
            match unsafe { crate::gpu::Gpu::for_window(&win, layout) } {
                Ok(g) => {
                    println!("disegno su GPU: {}", g.adapter_info);
                    Some(g)
                }
                Err(e) => {
                    eprintln!("GPU non disponibile ({e}): uso il disegno software");
                    None
                }
            }
        } else {
            None
        };
        // Audio: se c'è un dispositivo, è la scheda audio a dettare il ritmo dei
        // frame; altrimenti la finestra limita il ritmo a 50 frame al secondo.
        let audio = if o.audio { AudioOut::open() } else { None };
        match &audio {
            Some(a) => {
                win.set_target_fps(0);
                println!("audio: {} a {} Hz", a.device_name, a.rate);
            }
            None => {
                win.set_target_fps(50);
                if o.audio {
                    println!("audio: nessun dispositivo disponibile, gioco senza suono");
                }
            }
        }
        Some(Frontend {
            gpu,
            win,
            audio,
            buf: vec![0; w * h],
            scale: s,
            rows: o.rows,
            margin: o.margin,
            shot_dir: o.shot_dir.clone(),
            mouse_ratio: o.mouse_ratio,
            shots: 0,
            warned_rows: false,
            record: None,
            rec_buttons: (false, false),
            enhancer: Enhancer::new(false),
            dev: false,
        })
    }

    /// Attiva la registrazione della partita in `path` (vedi `--record`).
    pub fn start_recording(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        self.record = Some(std::io::BufWriter::new(std::fs::File::create(path)?));
        Ok(())
    }

    fn rec(&mut self, frame: u64, what: &str) {
        if let Some(w) = self.record.as_mut() {
            use std::io::Write;
            let _ = writeln!(w, "{frame}: {what}");
        }
    }

    /// Prepara una macchina per l'uscita audio di questa finestra.
    fn attach_audio(&self, m: &mut Machine) {
        if let Some(a) = &self.audio {
            m.bus.custom.paula.set_rate(a.rate);
            m.bus.custom.paula.enabled = true;
        }
    }

    /// Aspetta che la scheda audio abbia consumato abbastanza campioni
    /// (3 frame in coda, 60 ms, bastano a evitare buchi).
    fn pace(&self) {
        if let Some(a) = &self.audio {
            let target = a.rate as usize * 3 / 50;
            while a.queued_frames() > target {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }

    fn screenshot(&mut self, pixels: &[u32]) {
        std::fs::create_dir_all(&self.shot_dir).ok();
        self.shots += 1;
        let p = self
            .shot_dir
            .join(format!("screenshot{:03}.png", self.shots));
        // Lo screenshot contiene l'area del gioco (320×200), senza margini.
        let n = WIDTH * crate::render::GAME_HEIGHT.min(self.rows);
        if crate::render::save_png(&pixels[..n.min(pixels.len())], &p).is_ok() {
            println!("screenshot: {}", p.display());
        }
    }

    /// Ingrandisce un'immagine larga WIDTH pixel e la mostra dentro il margine.
    /// False se la finestra è chiusa.
    fn present(&mut self, src: &[u32], shapes: &[Shape]) -> bool {
        if let Some(g) = &mut self.gpu {
            let rows = self.rows;
            let src = &src[..src.len().min(rows * WIDTH)];
            match g.render(src, shapes) {
                Ok(()) => {
                    // Solo eventi e input: l'immagine l'ha già presentata wgpu.
                    self.win.update();
                    return self.win.is_open();
                }
                Err(e) => {
                    eprintln!("errore della GPU ({e}): passo al disegno software");
                    self.gpu = None;
                }
            }
        }
        let s = self.scale;
        let mg = self.margin;
        let w = (WIDTH + 2 * mg) * s;
        let h = (mg + self.rows) * s;
        // Margine superiore: nero.
        self.buf[..mg * s * w].fill(0);
        for y in 0..self.rows {
            let top = (mg + y) * s;
            let drow = &mut self.buf[top * w..(top + 1) * w];
            drow[..mg * s].fill(0);
            drow[(mg + WIDTH) * s..].fill(0);
            let dst = &mut drow[mg * s..(mg + WIDTH) * s];
            match src.get(y * WIDTH..(y + 1) * WIDTH) {
                Some(srow) => {
                    for (x, &p) in srow.iter().enumerate() {
                        dst[x * s..(x + 1) * s].fill(p);
                    }
                }
                None => dst.fill(0),
            }
            for k in 1..s {
                self.buf.copy_within(top * w..(top + 1) * w, (top + k) * w);
            }
        }
        if !shapes.is_empty() {
            let mut c = Canvas {
                buf: &mut self.buf,
                width: w,
                height: h,
                scale: s as f32,
                origin: (mg * s, mg * s),
            };
            c.draw(shapes);
        }
        self.win.update_with_buffer(&self.buf, w, h).is_ok() && self.win.is_open()
    }

    /// Mostra un'immagine con l'effetto "a tendina" dell'originale: le righe si
    /// scoprono dall'alto verso il basso in `wipe_frames` frame, poi l'immagine
    /// resta finché l'utente non clicca o preme un tasto. False se la finestra
    /// è stata chiusa.
    pub fn show_image(&mut self, pixels: &[u32], wipe_frames: u32) -> bool {
        let total_rows = self.rows;
        let mut shown = vec![0u32; WIDTH * total_rows];
        let mut frame = 0u32;
        // Ignora un tasto o un click già premuti all'ingresso (per esempio quello
        // che ha saltato la schermata precedente).
        let mut armed = false;
        loop {
            if !self.win.is_open() {
                return false;
            }
            // Tendina: righe visibili in questo frame.
            let visible = if wipe_frames == 0 {
                total_rows
            } else {
                ((frame as usize + 1) * total_rows / wipe_frames as usize).min(total_rows)
            };
            let n = (WIDTH * visible).min(pixels.len());
            shown[..n].copy_from_slice(&pixels[..n]);
            frame += 1;
            let revealed = visible >= total_rows;
            // L'attesa del click inizia a tendina completata, come nell'originale.
            let pressed = self.any_input();
            if revealed && armed && pressed {
                break;
            }
            if !pressed {
                armed = true;
            }
            if self.win.is_key_pressed(Key::F12, KeyRepeat::No) {
                let px = shown.clone();
                self.screenshot(&px);
            }
            if self.audio.is_some() {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let px = std::mem::take(&mut shown);
            let ok = self.present(&px, &[]);
            shown = px;
            if !ok {
                return false;
            }
        }
        true
    }

    fn any_input(&self) -> bool {
        self.win.get_mouse_down(MouseButton::Left)
            || self.win.get_mouse_down(MouseButton::Right)
            || !self
                .win
                .get_keys()
                .into_iter()
                .filter(|k| !matches!(k, Key::F12))
                .collect::<Vec<_>>()
                .is_empty()
    }

    /// Esegue una macchina emulata nella finestra. Con `skippable` (intro) un
    /// click o un tasto la interrompe.
    pub fn run(&mut self, m: &mut Machine, skippable: bool) -> RunEnd {
        self.attach_audio(m);
        if !skippable && self.record.is_some() {
            // Orologio fisso e modalità della CPU: la riproduzione sarà identica.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let clock = *m.os.clock_override.get_or_insert(now);
            let f = m.bus.custom.frame;
            if let Some(w) = self.record.as_mut() {
                use std::io::Write;
                let _ = writeln!(w, "# Registrazione di Mad TV: riproducila con");
                let _ = writeln!(w, "#   madtv --headless --script @questo_file --frames N");
                let _ = writeln!(w, "# Formato: FRAME: azione (vedi madtv --help)");
            }
            self.rec(f, &format!("clock {clock}"));
            let cpu = if m.auto_turbo {
                "auto".to_string()
            } else {
                m.cpu_speed.to_string()
            };
            self.rec(f, &format!("cpu {cpu}"));
        }
        let result = self.run_inner(m, skippable);
        if let Some(w) = self.record.as_mut() {
            use std::io::Write;
            let _ = writeln!(w, "{}: shot fine", m.bus.custom.frame);
            let _ = w.flush();
        }
        result
    }

    fn run_inner(&mut self, m: &mut Machine, skippable: bool) -> RunEnd {
        let s = self.scale as f32;
        let mut last_pos: Option<(f32, f32)> = None;
        let mut armed = false;
        while self.win.is_open() {
            // --- intro: un click o un tasto la saltano ---
            if skippable {
                let pressed = self.any_input();
                if armed && pressed {
                    if let Some(a) = &self.audio {
                        a.clear();
                    }
                    return RunEnd::Skipped;
                }
                if !pressed {
                    armed = true;
                }
            } else {
                self.game_input(m, s, &mut last_pos);
            }
            if self.win.is_key_pressed(Key::F12, KeyRepeat::No) {
                let px = m.bus.custom.display.front.clone();
                self.screenshot(&px);
            }
            if self.dev && self.win.is_key_pressed(Key::F9, KeyRepeat::No) {
                self.enhancer.highlights = !self.enhancer.highlights;
                println!(
                    "evidenziazioni {}",
                    if self.enhancer.highlights {
                        "attive"
                    } else {
                        "spente (gioco originale)"
                    }
                );
            }
            if self.win.is_key_pressed(Key::F10, KeyRepeat::No) {
                // Turbo automatico (default) oppure velocità sempre fissa.
                m.auto_turbo = !m.auto_turbo;
                let f = m.bus.custom.frame;
                let cpu = if m.auto_turbo {
                    "auto".to_string()
                } else {
                    m.cpu_speed.to_string()
                };
                self.rec(f, &format!("cpu {cpu}"));
                if m.auto_turbo {
                    println!("CPU: turbo automatico durante i caricamenti");
                } else {
                    println!("CPU: velocità fissa {}%", m.cpu_speed);
                }
            }

            // --- emulazione ---
            // Avanti veloce: automatico finché lo schermo è nero (caricamento
            // iniziale del gioco), oppure tenendo premuto F11.
            let black = !skippable && m.bus.custom.display.last_active_lines == 0;
            let frames = if black {
                25
            } else if self.win.is_key_down(Key::F11) {
                8
            } else {
                1
            };
            if frames == 1 {
                self.pace();
            }
            for _ in 0..frames {
                if let Some(stop) = m.run_frame() {
                    return RunEnd::Stopped(stop);
                }
            }
            let samples = m.bus.custom.paula.take();
            if let Some(a) = &self.audio {
                if frames == 1 {
                    a.push(&samples);
                } else {
                    a.clear(); // avanti veloce: niente audio accelerato
                }
            }

            // Se una schermata disegnasse oltre le righe mostrate, avvisa una volta.
            if !self.warned_rows && !skippable {
                if let Some((_, last)) = m.bus.custom.display.used_rows {
                    if last >= self.rows {
                        eprintln!(
                            "nota: il gioco ha disegnato oltre la riga {}; avvia con --full-height per vederla",
                            self.rows
                        );
                        self.warned_rows = true;
                    }
                }
            }

            // Fuori dalla modalità sviluppo lo strato non esiste: niente copie
            // della macchina, niente click di prova, l'immagine è solo il gioco.
            let shapes = if skippable || !self.dev {
                Vec::new()
            } else {
                self.enhancer.update(m)
            };
            let px = std::mem::take(&mut m.bus.custom.display.front);
            let ok = self.present(&px, &shapes);
            m.bus.custom.display.front = px;
            if !ok {
                break;
            }
        }
        RunEnd::Closed
    }

    /// Mouse e tastiera verso il gioco.
    fn game_input(&mut self, m: &mut Machine, s: f32, last_pos: &mut Option<(f32, f32)>) {
        if let Some((mx, my)) = self.win.get_mouse_pos(MouseMode::Clamp) {
            let mg = self.margin as f32;
            let (gx, gy) = (mx / s - mg, my / s - mg);
            match m.game_mouse() {
                // Allineamento assoluto con le variabili del gioco.
                Some(g) => {
                    let (dx, dy) = input::pointer_step(&g, (gx as i32, gy as i32));
                    if dx != 0 || dy != 0 {
                        input::mouse_move(&mut m.bus.custom, dx, dy);
                        self.rec(m.bus.custom.frame, &format!("move {dx} {dy}"));
                    }
                    *last_pos = Some((gx, gy));
                }
                // Movimento relativo, se la posizione del puntatore non è disponibile.
                None => {
                    if let Some((lx, ly)) = *last_pos {
                        let dx = ((gx - lx) * self.mouse_ratio).round() as i32;
                        let dy = ((gy - ly) * self.mouse_ratio).round() as i32;
                        if dx != 0 || dy != 0 {
                            input::mouse_move(&mut m.bus.custom, dx, dy);
                            *last_pos = Some((
                                lx + dx as f32 / self.mouse_ratio,
                                ly + dy as f32 / self.mouse_ratio,
                            ));
                        }
                    } else {
                        *last_pos = Some((gx, gy));
                    }
                }
            }
        }
        let buttons = (
            self.win.get_mouse_down(MouseButton::Left),
            self.win.get_mouse_down(MouseButton::Right),
        );
        input::mouse_buttons(&mut m.bus.custom, buttons.0, buttons.1);
        let f = m.bus.custom.frame;
        if buttons.0 != self.rec_buttons.0 {
            self.rec(f, if buttons.0 { "down" } else { "up" });
        }
        if buttons.1 != self.rec_buttons.1 {
            self.rec(f, if buttons.1 { "rdown" } else { "rup" });
        }
        self.rec_buttons = buttons;
        for k in self.win.get_keys_pressed(KeyRepeat::No) {
            if let Some(code) = amiga_key(k) {
                input::key(&mut m.bus.custom, code, true);
                self.rec(f, &format!("keydown ${code:02x}"));
            }
        }
        for k in self.win.get_keys_released() {
            if let Some(code) = amiga_key(k) {
                input::key(&mut m.bus.custom, code, false);
                self.rec(f, &format!("keyup ${code:02x}"));
            }
        }
    }
}

/// Tasto del PC → codice tasto Amiga.
fn amiga_key(k: Key) -> Option<u8> {
    use Key::*;
    let name = match k {
        A => "A",
        B => "B",
        C => "C",
        D => "D",
        E => "E",
        F => "F",
        G => "G",
        H => "H",
        I => "I",
        J => "J",
        K => "K",
        L => "L",
        M => "M",
        N => "N",
        O => "O",
        P => "P",
        Q => "Q",
        R => "R",
        S => "S",
        T => "T",
        U => "U",
        V => "V",
        W => "W",
        X => "X",
        Y => "Y",
        Z => "Z",
        Key0 => "0",
        Key1 => "1",
        Key2 => "2",
        Key3 => "3",
        Key4 => "4",
        Key5 => "5",
        Key6 => "6",
        Key7 => "7",
        Key8 => "8",
        Key9 => "9",
        Space => "SPACE",
        Enter => "ENTER",
        Backspace => "BACKSPACE",
        Tab => "TAB",
        Escape => "ESC",
        Delete => "DEL",
        Up => "UP",
        Down => "DOWN",
        Left => "LEFT",
        Right => "RIGHT",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        Insert => "HELP",
        LeftShift => "LSHIFT",
        RightShift => "RSHIFT",
        CapsLock => "CAPSLOCK",
        LeftCtrl | RightCtrl => "CTRL",
        LeftAlt => "LALT",
        RightAlt => "RALT",
        LeftSuper => "LAMIGA",
        RightSuper => "RAMIGA",
        Comma => "COMMA",
        Period => "PERIOD",
        Slash => "SLASH",
        Semicolon => "SEMICOLON",
        Apostrophe => "QUOTE",
        LeftBracket => "LBRACKET",
        RightBracket => "RBRACKET",
        Backslash => "BACKSLASH",
        Minus => "MINUS",
        Equal => "EQUAL",
        Backquote => "BACKQUOTE",
        _ => return None,
    };
    input::key_code(name)
}
