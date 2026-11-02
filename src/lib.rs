//! Porting di Mad TV (Rainbow Arts, 1991) dalla versione Amiga.
//!
//! Architettura (vedi `docs/architettura.md`):
//! - [`adf`]: lettura delle immagini disco originali;
//! - [`hunk`]: caricamento degli eseguibili AmigaOS;
//! - [`bus`] e [`custom`]: memoria e chip custom dell'"Amiga minimo";
//! - [`os`]: AmigaOS emulato ad alto livello (exec, dos, intuition, graphics);
//! - [`machine`]: CPU 68000 (crate `m68k`) + tutto il resto;
//! - [`render`]: conversione di copper list e bitplane in immagine.

pub mod adf;
pub mod audio;
pub mod bus;
pub mod custom;
pub mod enhance;
pub mod gpu;
pub mod hunk;
pub mod iff;
pub mod input;
pub mod machine;
pub mod os;
pub mod overlay;
pub mod paula;
pub mod render;
pub mod window;
