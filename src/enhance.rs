//! Miglioramenti dell'interfaccia sopra il gioco originale.
//!
//! Qui nascono le evidenziazioni (casella dove finirà il pannello trascinato,
//! aree in cui si può rilasciare, ecc.). Ogni frame `update` legge lo stato del
//! gioco dalla memoria emulata e produce le forme da disegnare nello strato di
//! sovrapposizione. Le variabili del gioco necessarie si trovano con le
//! registrazioni delle partite (vedi NOTES.md, "Strada verso le evidenziazioni").
//!
//! Per ora c'è solo la modalità diagnostica (`--debug-overlay`), che segna la
//! punta del puntatore secondo il gioco: serve a verificare che le coordinate
//! dello strato coincidano con quelle della grafica.

use crate::machine::Machine;
use crate::overlay::Shape;

#[derive(Default)]
pub struct Enhancer {
    /// Disegna le informazioni diagnostiche.
    pub debug: bool,
}

impl Enhancer {
    /// Forme da disegnare sopra il frame corrente.
    pub fn update(&mut self, m: &Machine) -> Vec<Shape> {
        let mut out = Vec::new();
        if self.debug {
            if let Some(g) = m.game_mouse() {
                let (x, y) = (g.pos.0 as f32, g.pos.1 as f32);
                // Mirino sulla punta del puntatore (il pixel (x, y) del gioco).
                out.push(Shape::Rect {
                    x,
                    y,
                    w: 1.0,
                    h: 1.0,
                    color: 0xFF00FF00,
                    thickness: 1,
                });
                out.push(Shape::Line {
                    x0: x - 6.0,
                    y0: y + 0.5,
                    x1: x - 2.0,
                    y1: y + 0.5,
                    color: 0xC000FF00,
                    thickness: 1,
                });
                out.push(Shape::Line {
                    x0: x + 3.0,
                    y0: y + 0.5,
                    x1: x + 7.0,
                    y1: y + 0.5,
                    color: 0xC000FF00,
                    thickness: 1,
                });
                out.push(Shape::Line {
                    x0: x + 0.5,
                    y0: y - 6.0,
                    x1: x + 0.5,
                    y1: y - 2.0,
                    color: 0xC000FF00,
                    thickness: 1,
                });
                out.push(Shape::Line {
                    x0: x + 0.5,
                    y0: y + 3.0,
                    x1: x + 0.5,
                    y1: y + 7.0,
                    color: 0xC000FF00,
                    thickness: 1,
                });
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
        }
        out
    }
}
