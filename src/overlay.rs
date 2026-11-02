//! Strato di sovrapposizione: forme disegnate sopra l'immagine del gioco.
//!
//! Le coordinate sono in pixel del gioco (320×200), anche frazionarie, ma il
//! disegno avviene alla risoluzione della finestra, dopo l'ingrandimento: una
//! linea spessa 1 pixel dello schermo resta sottile e nitida anche a scala 3 o
//! 4. È la base per le evidenziazioni dell'interfaccia e, in futuro, per lo
//! strato HD.

/// Colore con trasparenza: 0xAARRGGBB (AA = 255 opaco).
pub type Argb = u32;

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// Contorno di un rettangolo; `thickness` in pixel della finestra.
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Argb,
        thickness: u32,
    },
    /// Rettangolo pieno (con trasparenza).
    Fill {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Argb,
    },
    /// Linea orizzontale o verticale (per mirini e guide).
    Line {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        color: Argb,
        thickness: u32,
    },
}

/// Area di disegno: buffer della finestra, sua larghezza, scala e origine
/// (in pixel della finestra) del pixel (0,0) del gioco.
pub struct Canvas<'a> {
    pub buf: &'a mut [u32],
    pub width: usize,
    pub height: usize,
    pub scale: f32,
    pub origin: (usize, usize),
}

impl Canvas<'_> {
    fn blend(&mut self, x: i64, y: i64, color: Argb) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let i = y as usize * self.width + x as usize;
        let a = (color >> 24) & 0xFF;
        if a == 255 {
            self.buf[i] = color & 0xFF_FFFF;
            return;
        }
        let d = self.buf[i];
        let mix = |sh: u32| -> u32 {
            let s = (color >> sh) & 0xFF;
            let t = (d >> sh) & 0xFF;
            ((s * a + t * (255 - a)) / 255) << sh
        };
        self.buf[i] = mix(16) | mix(8) | mix(0);
    }

    fn to_px(&self, x: f32, y: f32) -> (i64, i64) {
        (
            (self.origin.0 as f32 + x * self.scale).round() as i64,
            (self.origin.1 as f32 + y * self.scale).round() as i64,
        )
    }

    fn fill_px(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, color: Argb) {
        for y in y0.max(0)..y1.min(self.height as i64) {
            for x in x0.max(0)..x1.min(self.width as i64) {
                self.blend(x, y, color);
            }
        }
    }

    pub fn draw(&mut self, shapes: &[Shape]) {
        for s in shapes {
            match *s {
                Shape::Fill { x, y, w, h, color } => {
                    let (x0, y0) = self.to_px(x, y);
                    let (x1, y1) = self.to_px(x + w, y + h);
                    self.fill_px(x0, y0, x1, y1, color);
                }
                Shape::Rect {
                    x,
                    y,
                    w,
                    h,
                    color,
                    thickness,
                } => {
                    let t = thickness.max(1) as i64;
                    let (x0, y0) = self.to_px(x, y);
                    let (x1, y1) = self.to_px(x + w, y + h);
                    self.fill_px(x0, y0, x1, y0 + t, color); // sopra
                    self.fill_px(x0, y1 - t, x1, y1, color); // sotto
                    self.fill_px(x0, y0 + t, x0 + t, y1 - t, color); // sinistra
                    self.fill_px(x1 - t, y0 + t, x1, y1 - t, color); // destra
                }
                Shape::Line {
                    x0,
                    y0,
                    x1,
                    y1,
                    color,
                    thickness,
                } => {
                    let t = thickness.max(1) as i64;
                    let (a0, b0) = self.to_px(x0, y0);
                    let (a1, b1) = self.to_px(x1, y1);
                    if b0 == b1 {
                        self.fill_px(a0.min(a1), b0 - t / 2, a0.max(a1), b0 - t / 2 + t, color);
                    } else {
                        self.fill_px(a0 - t / 2, b0.min(b1), a0 - t / 2 + t, b0.max(b1), color);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contorno_e_trasparenza() {
        let mut buf = vec![0u32; 30 * 30];
        let mut c = Canvas {
            buf: &mut buf,
            width: 30,
            height: 30,
            scale: 3.0,
            origin: (0, 0),
        };
        c.draw(&[Shape::Rect {
            x: 1.0,
            y: 1.0,
            w: 5.0,
            h: 5.0,
            color: 0xFFFF0000,
            thickness: 1,
        }]);
        assert_eq!(
            buf[3 * 30 + 3],
            0xFF0000,
            "angolo in alto a sinistra a (3,3)"
        );
        assert_eq!(buf[10 * 30 + 10], 0, "interno vuoto");
        let mut c = Canvas {
            buf: &mut buf,
            width: 30,
            height: 30,
            scale: 3.0,
            origin: (0, 0),
        };
        c.draw(&[Shape::Fill {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            color: 0x80FFFFFF,
        }]);
        assert_eq!(buf[0] & 0xFF, 128, "bianco al 50% sul nero");
    }
}
