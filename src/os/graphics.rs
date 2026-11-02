//! graphics.library emulata.
//!
//! Dall'analisi risulta che Mad TV disegna quasi tutto da sé (blitter e
//! copper programmati direttamente), quindi qui servirà poco. Le funzioni
//! vengono aggiunte solo quando il gioco le chiama davvero: ogni chiamata non
//! implementata ferma l'emulazione con un messaggio chiaro.

use super::{Ctx, Os};

#[derive(Default, Clone)]
pub struct GfxState {
    /// Numero di WaitTOF eseguiti (sincronizzazioni sul vertical blank).
    pub wait_tof: u64,
}

pub fn call(os: &mut Os, c: &mut Ctx, idx: u16) -> Option<String> {
    Some(match idx {
        // WaitBlit(): il nostro blitter termina istantaneamente.
        33 => String::new(),
        // WaitTOF(): attende il prossimo vertical blank.
        40 => {
            os.gfx.wait_tof += 1;
            os.pending_frames += 1;
            String::new()
        }
        // OwnBlitter / DisownBlitter: nessun altro utente del blitter.
        71 | 72 => String::new(),
        _ => {
            let _ = c;
            return None;
        }
    })
}
