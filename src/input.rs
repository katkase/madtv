//! Input dell'Amiga: mouse sulla porta 0 e tastiera.
//!
//! - **Mouse**: due contatori a 8 bit in JOY0DAT (X nel byte basso, Y in quello
//!   alto) che avanzano con il movimento; il gioco li legge a ogni frame e usa la
//!   differenza. Tasto sinistro = CIA-A PRA bit 6, tasto destro = POTGOR bit 10
//!   (entrambi attivi bassi, gestiti in `custom.rs`).
//! - **Tastiera**: il gioco legge direttamente il registro seriale della CIA-A
//!   (SDR, `$BFEC01`) nel suo interrupt e reagisce quando il valore cambia. La
//!   tastiera Amiga trasmette il codice tasto ruotato a sinistra di un bit e
//!   negato; il bit 7 del codice vale 1 al rilascio.

use crate::custom::Custom;

/// Sposta il mouse di (dx, dy) "conteggi" (un conteggio ≈ un pixel lores).
pub fn mouse_move(c: &mut Custom, dx: i32, dy: i32) {
    c.mouse_x = c.mouse_x.wrapping_add(dx as u8);
    c.mouse_y = c.mouse_y.wrapping_add(dy as u8);
}

pub fn mouse_buttons(c: &mut Custom, left: bool, right: bool) {
    c.left_button = left;
    c.right_button = right;
}

/// Pressione o rilascio di un tasto (codice tasto Amiga, 0x00..0x67).
pub fn key(c: &mut Custom, code: u8, pressed: bool) {
    let k = (code & 0x7F) | if pressed { 0 } else { 0x80 };
    c.cia[0][0xC] = !k.rotate_left(1);
}

/// Limiti predefiniti del puntatore (schermata principale), usati solo se il
/// gioco non li fornisce.
pub const POINTER_MAX_X: i32 = 303;
pub const POINTER_MAX_Y: i32 = 189;

/// Calcola i conteggi del mouse da inviare per portare il puntatore del gioco
/// verso `target`.
///
/// Il gioco accumula i movimenti nell'interrupt di frame e li applica al
/// puntatore nel ciclo principale, che quando è occupato (per esempio mentre
/// disegna un dialog) può saltare qualche frame. La posizione "prevista" è
/// quindi puntatore + movimento ancora da applicare, entrambi letti dalle
/// variabili del gioco: così non si reinvia mai un movimento già fatto (era la
/// causa dei salti) e non si spinge oltre i limiti della schermata corrente.
pub fn pointer_step(g: &crate::machine::GameMouse, target: (i32, i32)) -> (i32, i32) {
    let (minx, miny) = g.min;
    let (maxx, maxy) = if g.max.0 > g.min.0 && g.max.1 > g.min.1 {
        g.max
    } else {
        (POINTER_MAX_X, POINTER_MAX_Y)
    };
    let tx = target.0.clamp(minx, maxx);
    let ty = target.1.clamp(miny, maxy);
    let predicted = (g.pos.0 + g.pending.0, g.pos.1 + g.pending.1);
    (
        (tx - predicted.0).clamp(-100, 100),
        (ty - predicted.1).clamp(-100, 100),
    )
}

/// Codici tasto Amiga per nome (usati dagli script e dalla finestra).
pub fn key_code(name: &str) -> Option<u8> {
    let n = name.to_uppercase();
    let letters = [
        ("Q", 0x10),
        ("W", 0x11),
        ("E", 0x12),
        ("R", 0x13),
        ("T", 0x14),
        ("Y", 0x15),
        ("U", 0x16),
        ("I", 0x17),
        ("O", 0x18),
        ("P", 0x19),
        ("A", 0x20),
        ("S", 0x21),
        ("D", 0x22),
        ("F", 0x23),
        ("G", 0x24),
        ("H", 0x25),
        ("J", 0x26),
        ("K", 0x27),
        ("L", 0x28),
        ("Z", 0x31),
        ("X", 0x32),
        ("C", 0x33),
        ("V", 0x34),
        ("B", 0x35),
        ("N", 0x36),
        ("M", 0x37),
    ];
    if let Some((_, c)) = letters.iter().find(|(l, _)| *l == n) {
        return Some(*c);
    }
    Some(match n.as_str() {
        "1" => 0x01,
        "2" => 0x02,
        "3" => 0x03,
        "4" => 0x04,
        "5" => 0x05,
        "6" => 0x06,
        "7" => 0x07,
        "8" => 0x08,
        "9" => 0x09,
        "0" => 0x0A,
        "MINUS" => 0x0B,
        "EQUAL" => 0x0C,
        "BACKSLASH" => 0x0D,
        "SPACE" => 0x40,
        "BACKSPACE" => 0x41,
        "TAB" => 0x42,
        "ENTER" | "RETURN" => 0x44,
        "ESC" => 0x45,
        "DEL" => 0x46,
        "UP" => 0x4C,
        "DOWN" => 0x4D,
        "RIGHT" => 0x4E,
        "LEFT" => 0x4F,
        "F1" => 0x50,
        "F2" => 0x51,
        "F3" => 0x52,
        "F4" => 0x53,
        "F5" => 0x54,
        "F6" => 0x55,
        "F7" => 0x56,
        "F8" => 0x57,
        "F9" => 0x58,
        "F10" => 0x59,
        "HELP" => 0x5F,
        "LSHIFT" => 0x60,
        "RSHIFT" => 0x61,
        "CAPSLOCK" => 0x62,
        "CTRL" => 0x63,
        "LALT" => 0x64,
        "RALT" => 0x65,
        "LAMIGA" => 0x66,
        "RAMIGA" => 0x67,
        "COMMA" => 0x38,
        "PERIOD" => 0x39,
        "SLASH" => 0x3A,
        "SEMICOLON" => 0x29,
        "QUOTE" => 0x2A,
        "LBRACKET" => 0x1A,
        "RBRACKET" => 0x1B,
        "BACKQUOTE" => 0x00,
        _ => return None,
    })
}

/// Un'azione programmata per un certo frame (vedi `madtv --help`, opzione --script).
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Move(i32, i32),
    /// Porta il puntatore del gioco alle coordinate (x, y) dello schermo.
    Goto(i32, i32),
    LeftDown,
    LeftUp,
    RightDown,
    RightUp,
    /// Pressione e rilascio (il rilascio avviene 3 frame dopo).
    Click,
    KeyDown(u8),
    KeyUp(u8),
    /// Pressione e rilascio di un tasto.
    Key(u8),
    /// Scrive un testo carattere per carattere.
    Type(String),
    Shot(String),
}

/// Analizza uno script: voci "FRAME:azione argomenti" separate da ';' o a capo.
/// Esempio: `1200:move 40 -10; 1205:click; 1300:shot menu`
pub fn parse_script(src: &str) -> Result<Vec<(u64, Action)>, String> {
    let mut out = Vec::new();
    for raw in src.split([';', '\n']) {
        let item = raw.trim();
        if item.is_empty() || item.starts_with('#') {
            continue;
        }
        let (f, rest) = item
            .split_once(':')
            .ok_or(format!("manca 'frame:' in \"{item}\""))?;
        let frame: u64 = f
            .trim()
            .parse()
            .map_err(|_| format!("frame non valido in \"{item}\""))?;
        let mut parts = rest.split_whitespace();
        let cmd = parts
            .next()
            .ok_or(format!("azione mancante in \"{item}\""))?
            .to_lowercase();
        let args: Vec<&str> = parts.collect();
        let num = |i: usize| -> Result<i32, String> {
            args.get(i)
                .and_then(|s| s.parse().ok())
                .ok_or(format!("argomento numerico mancante in \"{item}\""))
        };
        let keyarg = || -> Result<u8, String> {
            let n = args
                .first()
                .ok_or(format!("tasto mancante in \"{item}\""))?;
            key_code(n).ok_or(format!("tasto sconosciuto: {n}"))
        };
        let a = match cmd.as_str() {
            "move" => Action::Move(num(0)?, num(1)?),
            "goto" => Action::Goto(num(0)?, num(1)?),
            "down" => Action::LeftDown,
            "up" => Action::LeftUp,
            "rdown" => Action::RightDown,
            "rup" => Action::RightUp,
            "click" => Action::Click,
            "keydown" => Action::KeyDown(keyarg()?),
            "keyup" => Action::KeyUp(keyarg()?),
            "key" => Action::Key(keyarg()?),
            "type" => Action::Type(args.join(" ")),
            "shot" => Action::Shot(args.first().unwrap_or(&"shot").to_string()),
            _ => return Err(format!("azione sconosciuta: {cmd}")),
        };
        out.push((frame, a));
    }
    out.sort_by_key(|(f, _)| *f);
    Ok(out)
}

/// Espande le azioni composte (click, key, type) in pressioni e rilasci distribuiti sui frame.
pub fn expand(script: Vec<(u64, Action)>) -> Vec<(u64, Action)> {
    let mut out = Vec::new();
    for (f, a) in script {
        match a {
            Action::Click => {
                out.push((f, Action::LeftDown));
                out.push((f + 3, Action::LeftUp));
            }
            Action::Key(k) => {
                out.push((f, Action::KeyDown(k)));
                out.push((f + 3, Action::KeyUp(k)));
            }
            Action::Type(text) => {
                let mut t = f;
                for ch in text.chars() {
                    let name = if ch == ' ' {
                        "SPACE".to_string()
                    } else {
                        ch.to_string()
                    };
                    if let Some(k) = key_code(&name) {
                        let shift = ch.is_ascii_uppercase();
                        if shift {
                            out.push((t, Action::KeyDown(0x60)));
                        }
                        out.push((t + 1, Action::KeyDown(k)));
                        out.push((t + 4, Action::KeyUp(k)));
                        if shift {
                            out.push((t + 5, Action::KeyUp(0x60)));
                        }
                        t += 8;
                    }
                }
            }
            other => out.push((f, other)),
        }
    }
    out.sort_by_key(|(f, _)| *f);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codifica_tastiera() {
        let mut c = Custom::new();
        key(&mut c, 0x20, true); // A premuto
        assert_eq!(c.cia[0][0xC], !0x40u8);
        key(&mut c, 0x20, false); // A rilasciato: codice 0xA0
        assert_eq!(c.cia[0][0xC], !0x41u8);
    }

    /// Simula il gioco: l'interrupt accumula i movimenti, il ciclo principale
    /// li applica solo ogni tanto (occupato), con limiti. L'inseguimento non
    /// deve mai superare il bersaglio.
    #[test]
    fn inseguimento_con_ciclo_occupato() {
        use crate::machine::GameMouse;
        let mut g = GameMouse {
            pos: (160, 100),
            pending: (0, 0),
            min: (0, 0),
            max: (303, 189),
        };
        for frame in 0..80 {
            let target = if frame < 40 { (200, 150) } else { (500, -50) };
            let d = pointer_step(&g, target);
            g.pending.0 += d.0; // interrupt: accumula
            g.pending.1 += d.1;
            if frame % 7 == 0 {
                // ciclo principale: applica tutto, poi limita
                g.pos = (
                    (g.pos.0 + g.pending.0).clamp(0, 303),
                    (g.pos.1 + g.pending.1).clamp(0, 189),
                );
                g.pending = (0, 0);
            }
            if frame < 40 {
                assert!(
                    g.pos.0 <= 200 && g.pos.1 <= 150,
                    "superato il bersaglio: {:?}",
                    g.pos
                );
            }
        }
        assert_eq!(g.pos, (303, 0));
    }

    #[test]
    fn script() {
        let s = parse_script("10:move 5 -3; 5:click\n20:shot menu").unwrap();
        assert_eq!(s[0], (5, Action::Click));
        let e = expand(s);
        assert_eq!(e[0], (5, Action::LeftDown));
        assert_eq!(e[1], (8, Action::LeftUp));
    }
}
