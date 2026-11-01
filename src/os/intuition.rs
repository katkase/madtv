//! intuition.library emulata.
//!
//! Come per graphics, le funzioni vengono implementate man mano che il gioco
//! le chiama. Il gioco apre intuition.library ma, fino alla schermata di
//! selezione dei giocatori, non la usa mai: disegna tutto da sé.

use super::{Ctx, Os};

pub fn call(_os: &mut Os, _c: &mut Ctx, _idx: u16) -> Option<String> {
    None
}
