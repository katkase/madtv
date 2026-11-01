//! Finestra di gioco (crate `minifb`): 50 frame al secondo, mouse e tastiera.
//!
//! Tasti speciali: F10 = CPU originale/veloce, F11 (tenuto premuto) = avanti
//! veloce, F12 = screenshot.
//!
//! Il gioco disegna da sé il puntatore del mouse, quindi il cursore di sistema
//! viene nascosto. I movimenti del mouse del PC vengono convertiti in conteggi
//! del mouse Amiga (vedi `input.rs`). Quando la posizione del puntatore del
//! gioco è leggibile (`Machine::game_pointer`) l'allineamento è assoluto: a ogni
//! frame inviamo la differenza tra il cursore del PC e il puntatore del gioco.

use crate::input;
use crate::machine::{Machine, StopReason};
use crate::render::{self, HEIGHT, WIDTH};
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use std::path::PathBuf;

pub struct WindowOpts {
    pub scale: usize,
    pub shot_dir: PathBuf,
    /// Conteggi del mouse Amiga per pixel del gioco.
    pub mouse_ratio: f32,
}

/// Esegue il gioco in una finestra finché non viene chiusa o il programma termina.
pub fn run(m: &mut Machine, o: &WindowOpts) -> Option<StopReason> {
    let s = o.scale.max(1);
    let (w, h) = (WIDTH * s, HEIGHT * s);
    let mut win = match Window::new("Mad TV", w, h, WindowOptions::default()) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("impossibile aprire la finestra: {e}");
            return None;
        }
    };
    win.set_target_fps(50);
    win.set_cursor_visibility(false);
    let mut buf = vec![0u32; w * h];
    let mut last_pos: Option<(f32, f32)> = None;
    let fast_speed = if m.cpu_speed > 100 { m.cpu_speed } else { 300 };
    let mut shots = 0u32;

    while win.is_open() {
        // --- input ---
        if let Some((mx, my)) = win.get_mouse_pos(MouseMode::Clamp) {
            let (gx, gy) = (mx / s as f32, my / s as f32);
            match m.game_mouse() {
                // Allineamento assoluto: spostiamo il mouse Amiga della differenza
                // tra il cursore del PC e il puntatore del gioco.
                Some(g) => {
                    let (dx, dy) = input::pointer_step(&g, (gx as i32, gy as i32));
                    if dx != 0 || dy != 0 {
                        input::mouse_move(&mut m.bus.custom, dx, dy);
                    }
                    last_pos = Some((gx, gy));
                }
                // Movimento relativo, se la posizione del puntatore non è disponibile.
                None => {
                    if let Some((lx, ly)) = last_pos {
                        let dx = ((gx - lx) * o.mouse_ratio).round() as i32;
                        let dy = ((gy - ly) * o.mouse_ratio).round() as i32;
                        if dx != 0 || dy != 0 {
                            input::mouse_move(&mut m.bus.custom, dx, dy);
                            last_pos = Some((
                                lx + dx as f32 / o.mouse_ratio,
                                ly + dy as f32 / o.mouse_ratio,
                            ));
                        }
                    } else {
                        last_pos = Some((gx, gy));
                    }
                }
            }
        }
        input::mouse_buttons(
            &mut m.bus.custom,
            win.get_mouse_down(MouseButton::Left),
            win.get_mouse_down(MouseButton::Right),
        );
        for k in win.get_keys_pressed(KeyRepeat::No) {
            if k == Key::F10 {
                // CPU originale (A500) o veloce: cambia solo la rapidità di calcolo e disegno.
                m.cpu_speed = if m.cpu_speed > 100 { 100 } else { fast_speed };
                println!("velocità CPU: {}%", m.cpu_speed);
                continue;
            }
            if k == Key::F12 {
                std::fs::create_dir_all(&o.shot_dir).ok();
                shots += 1;
                let p = o.shot_dir.join(format!("screenshot{shots:03}.png"));
                if render::save_png(&m.bus.custom.display.front, &p).is_ok() {
                    println!("screenshot: {}", p.display());
                }
                continue;
            }
            if let Some(code) = amiga_key(k) {
                input::key(&mut m.bus.custom, code, true);
            }
        }
        for k in win.get_keys_released() {
            if let Some(code) = amiga_key(k) {
                input::key(&mut m.bus.custom, code, false);
            }
        }

        // --- emulazione ---
        // Avanti veloce: automatico finché lo schermo è nero (caricamento
        // iniziale), oppure tenendo premuto F11.
        let black = m.bus.custom.display.last_active_lines == 0;
        let frames = if black {
            25
        } else if win.is_key_down(Key::F11) {
            8
        } else {
            1
        };
        for _ in 0..frames {
            if let Some(stop) = m.run_frame() {
                return Some(stop);
            }
        }

        // --- presentazione (ingrandimento a pixel interi) ---
        let src = &m.bus.custom.display.front;
        for y in 0..HEIGHT {
            let srow = &src[y * WIDTH..(y + 1) * WIDTH];
            let drow = &mut buf[y * s * w..(y * s + 1) * w];
            for (x, &p) in srow.iter().enumerate() {
                drow[x * s..(x + 1) * s].fill(p);
            }
            for k in 1..s {
                buf.copy_within(y * s * w..(y * s + 1) * w, (y * s + k) * w);
            }
        }
        if win.update_with_buffer(&buf, w, h).is_err() {
            break;
        }
    }
    None
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
        F10 => "F10",
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
