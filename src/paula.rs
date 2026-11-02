//! Paula: i 4 canali audio dell'Amiga.
//!
//! Ogni canale legge via DMA parole di 16 bit (due campioni da 8 bit con segno)
//! dalla Chip RAM a partire da AUDxLC, per AUDxLEN parole; ogni campione dura
//! AUDxPER cicli di clock audio (3,546895 MHz in PAL, metà del clock della
//! CPU); il volume va da 0 a 64. Finito il blocco, il canale ricarica
//! puntatore e lunghezza dai registri e ricomincia: per questo genera un
//! interrupt (AUD0-3, livello 4) quando ha letto i registri, così il
//! programma può già scrivere l'indirizzo del blocco successivo.
//!
//! Semplificazioni: niente modulazione (ADKCON), temporizzazione degli slot DMA
//! ignorata (i campioni arrivano al momento giusto, non qualche ciclo prima).
//!
//! L'uscita è mixata come sull'hardware (canali 0 e 3 a sinistra, 1 e 2 a
//! destra), ricampionata alla frequenza della scheda audio del PC con un filtro
//! a media mobile, poi filtrata come un A500: passa-basso fisso a circa 4,4 kHz
//! e, se acceso dal programma (bit 1 della CIA-A PRA a 0), il filtro "LED" a
//! circa 3,3 kHz.

/// Clock audio PAL in Hz (color clock).
pub const AUDIO_CLOCK: f64 = 3_546_895.0;

#[derive(Default, Clone, Copy)]
struct Channel {
    // registri
    lc: u32,
    len: u16,
    per: u16,
    vol: u8,
    dat: u16,
    // stato
    dma_on: bool,
    ptr: u32,
    remaining: u32,
    word: u16,
    /// Prossimo byte da emettere della parola corrente: 0 = alto, 1 = basso, 2 = parola finita.
    byte: u8,
    counter: u32,
    /// Campione in uscita (8 bit con segno) e se il canale sta suonando.
    sample: i8,
    playing: bool,
    /// Modo manuale: parola scritta dalla CPU in AUDxDAT, in attesa di essere suonata.
    manual_pending: bool,
}

#[derive(Clone)]
pub struct Paula {
    ch: [Channel; 4],
    /// Frequenza di uscita (Hz) e clock audio per campione in uscita.
    pub rate: u32,
    clocks_per_out: f64,
    to_next_out: f64,
    acc_l: f64,
    acc_r: f64,
    acc_clocks: f64,
    /// Filtri: stato dei passa-basso (sinistra, destra).
    lp_fixed: [f32; 2],
    lp_led1: [f32; 2],
    lp_led2: [f32; 2],
    a_fixed: f32,
    a_led: f32,
    pub led_filter: bool,
    /// Campioni stereo interlacciati prodotti e non ancora prelevati.
    pub out: Vec<f32>,
    /// Se false i campioni non vengono accumulati (nessuna uscita audio).
    pub enabled: bool,
    /// Statistiche.
    pub samples_played: [u64; 4],
}

impl Default for Paula {
    fn default() -> Self {
        Self::new(48_000)
    }
}

fn one_pole(fc: f64, fs: f64) -> f32 {
    (1.0 - (-2.0 * std::f64::consts::PI * fc / fs).exp()) as f32
}

impl Paula {
    pub fn new(rate: u32) -> Paula {
        let mut p = Paula {
            ch: [Channel::default(); 4],
            rate,
            clocks_per_out: 0.0,
            to_next_out: 0.0,
            acc_l: 0.0,
            acc_r: 0.0,
            acc_clocks: 0.0,
            lp_fixed: [0.0; 2],
            lp_led1: [0.0; 2],
            lp_led2: [0.0; 2],
            a_fixed: 0.0,
            a_led: 0.0,
            led_filter: false,
            out: Vec::new(),
            enabled: false,
            samples_played: [0; 4],
        };
        p.set_rate(rate);
        p
    }

    pub fn set_rate(&mut self, rate: u32) {
        self.rate = rate.max(8000);
        self.clocks_per_out = AUDIO_CLOCK / self.rate as f64;
        self.to_next_out = self.clocks_per_out;
        self.a_fixed = one_pole(4400.0, self.rate as f64);
        self.a_led = one_pole(3300.0, self.rate as f64);
    }

    /// Scrittura di un registro audio ($0A0-$0DE). Restituisce i bit di INTREQ da attivare.
    pub fn write(&mut self, off: u16, val: u16) -> u16 {
        let n = ((off - 0xA0) / 0x10) as usize;
        let c = &mut self.ch[n];
        match off & 0x0F {
            0x0 => c.lc = (c.lc & 0xFFFF) | ((val as u32 & 0x1F) << 16),
            0x2 => c.lc = (c.lc & 0x1F_0000) | (val as u32 & 0xFFFE),
            0x4 => c.len = val,
            0x6 => c.per = val,
            0x8 => c.vol = (val & 0x7F).min(64) as u8,
            0xA => {
                c.dat = val;
                if !c.dma_on {
                    c.manual_pending = true;
                    if !c.playing {
                        // Avvio in modo manuale: la parola parte subito e si chiede la prossima.
                        c.word = val;
                        c.byte = 0;
                        c.manual_pending = false;
                        c.playing = true;
                        c.counter = 0;
                        return 1 << (7 + n);
                    }
                }
            }
            _ => {}
        }
        0
    }

    /// Aggiorna lo stato del DMA audio da DMACON. Restituisce i bit di INTREQ da attivare.
    pub fn set_dmacon(&mut self, dmacon: u16, ram: &[u8]) -> u16 {
        let mut irq = 0;
        let master = dmacon & 0x0200 != 0;
        for n in 0..4 {
            let on = master && dmacon & (1 << n) != 0;
            let c = &mut self.ch[n];
            if on && !c.dma_on {
                // Avvio del DMA: carica puntatore e lunghezza, legge la prima parola
                // e segnala con l'interrupt che i registri si possono già riscrivere.
                c.dma_on = true;
                c.ptr = c.lc;
                c.remaining = if c.len == 0 { 0x10000 } else { c.len as u32 };
                c.word = read_word(ram, c.ptr);
                c.ptr = c.ptr.wrapping_add(2);
                c.remaining -= 1;
                c.byte = 0;
                c.counter = 0;
                c.playing = true;
                irq |= 1 << (7 + n);
            } else if !on && c.dma_on {
                c.dma_on = false;
                c.playing = false;
                c.sample = 0;
            }
        }
        irq
    }

    /// Fa avanzare l'audio di `clocks` cicli di clock audio. Restituisce i bit di
    /// INTREQ da attivare (fine blocco DMA o parola manuale consumata).
    pub fn run(&mut self, mut clocks: u64, ram: &[u8]) -> u16 {
        let mut irq = 0u16;
        while clocks > 0 {
            // Passo: fino al prossimo evento (cambio di campione o campione in uscita).
            let mut step = clocks.min(self.to_next_out.ceil().max(1.0) as u64);
            for c in self.ch.iter() {
                if c.playing && c.counter > 0 {
                    step = step.min(c.counter as u64);
                }
            }
            // Integrazione dell'uscita sul passo (filtro a media mobile).
            let (l, r) = self.mix();
            self.acc_l += l * step as f64;
            self.acc_r += r * step as f64;
            self.acc_clocks += step as f64;
            for n in 0..4 {
                let c = &mut self.ch[n];
                if !c.playing {
                    continue;
                }
                if c.counter as u64 > step {
                    c.counter -= step as u32;
                    continue;
                }
                // Campione successivo.
                if c.byte >= 2 {
                    // Parola finita: la prossima dal DMA, o dal registro in modo manuale.
                    if c.dma_on {
                        if c.remaining == 0 {
                            c.ptr = c.lc;
                            c.remaining = if c.len == 0 { 0x10000 } else { c.len as u32 };
                            irq |= 1 << (7 + n);
                        }
                        c.word = read_word(ram, c.ptr);
                        c.ptr = c.ptr.wrapping_add(2);
                        c.remaining -= 1;
                    } else if c.manual_pending {
                        c.word = c.dat;
                        c.manual_pending = false;
                        irq |= 1 << (7 + n);
                    } else {
                        c.playing = false;
                        c.sample = 0;
                        continue;
                    }
                    c.byte = 0;
                }
                c.sample = if c.byte == 0 {
                    (c.word >> 8) as u8 as i8
                } else {
                    c.word as u8 as i8
                };
                c.byte += 1;
                self.samples_played[n] += 1;
                // Il DMA non riesce a servire periodi sotto 124 clock.
                let per = if c.per < 124 && c.dma_on {
                    124
                } else {
                    c.per.max(1)
                };
                c.counter = per as u32;
            }
            self.to_next_out -= step as f64;
            if self.to_next_out <= 0.0 {
                self.to_next_out += self.clocks_per_out;
                if self.enabled {
                    let l = (self.acc_l / self.acc_clocks.max(1.0)) as f32;
                    let r = (self.acc_r / self.acc_clocks.max(1.0)) as f32;
                    let (l, r) = self.filter(l, r);
                    self.out.push(l);
                    self.out.push(r);
                }
                self.acc_l = 0.0;
                self.acc_r = 0.0;
                self.acc_clocks = 0.0;
            }
            clocks -= step;
        }
        irq
    }

    /// Uscita istantanea (-1..1) dei canali mixati: 0 e 3 a sinistra, 1 e 2 a destra.
    fn mix(&self) -> (f64, f64) {
        let v = |c: &Channel| {
            if c.playing {
                c.sample as f64 * c.vol as f64
            } else {
                0.0
            }
        };
        let scale = 1.0 / (128.0 * 64.0 * 2.0);
        (
            (v(&self.ch[0]) + v(&self.ch[3])) * scale,
            (v(&self.ch[1]) + v(&self.ch[2])) * scale,
        )
    }

    fn filter(&mut self, l: f32, r: f32) -> (f32, f32) {
        let mut s = [l, r];
        for i in 0..2 {
            self.lp_fixed[i] += self.a_fixed * (s[i] - self.lp_fixed[i]);
            s[i] = self.lp_fixed[i];
            if self.led_filter {
                self.lp_led1[i] += self.a_led * (s[i] - self.lp_led1[i]);
                self.lp_led2[i] += self.a_led * (self.lp_led1[i] - self.lp_led2[i]);
                s[i] = self.lp_led2[i];
            } else {
                self.lp_led1[i] = s[i];
                self.lp_led2[i] = s[i];
            }
        }
        (s[0], s[1])
    }

    /// Descrizione dello stato dei canali, per il rapporto diagnostico.
    pub fn describe(&self) -> String {
        self.ch
            .iter()
            .enumerate()
            .map(|(n, c)| {
                format!(
                    "canale {n}: {} per={} vol={} len={} lc=${:06x} campioni={}",
                    if c.dma_on {
                        "DMA"
                    } else if c.playing {
                        "manuale"
                    } else {
                        "fermo"
                    },
                    c.per,
                    c.vol,
                    c.len,
                    c.lc,
                    self.samples_played[n]
                )
            })
            .collect::<Vec<_>>()
            .join("\n  ")
    }

    /// Preleva i campioni prodotti (stereo interlacciati).
    pub fn take(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.out)
    }
}

fn read_word(ram: &[u8], a: u32) -> u16 {
    let a = (a & 0x1F_FFFE) as usize;
    if a + 1 < ram.len() {
        u16::from_be_bytes([ram[a], ram[a + 1]])
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un'onda quadra di 2 campioni (+100, -100) con periodo 400: frequenza
    /// attesa 3546895 / (400 * 2) ≈ 4434 Hz → contiamo i passaggi per zero.
    #[test]
    fn frequenza_e_interrupt() {
        let mut ram = vec![0u8; 0x1000];
        ram[0x100] = 100;
        ram[0x101] = (-100i8) as u8;
        let mut p = Paula::new(48_000);
        p.enabled = true;
        p.write(0xA0, 0);
        p.write(0xA2, 0x100);
        p.write(0xA4, 1); // una parola
        p.write(0xA6, 400);
        p.write(0xA8, 64);
        let irq = p.set_dmacon(0x0201, &ram);
        assert_eq!(irq, 1 << 7, "interrupt all'avvio del DMA");
        let irq = p.run(AUDIO_CLOCK as u64, &ram); // un secondo
        assert_ne!(irq & (1 << 7), 0, "interrupt a fine blocco");
        // il primo campione esce all'istante zero
        assert_eq!(
            p.samples_played[0] as f64,
            (AUDIO_CLOCK / 400.0).floor() + 1.0
        );
        let out = p.take();
        // ~48000 campioni stereo (l'arrotondamento dei passi può perderne uno o due)
        assert!(
            (2 * 47_990..=2 * 48_000).contains(&out.len()),
            "{} campioni",
            out.len()
        );
        // Canale 0 → solo a sinistra.
        assert!(out.iter().skip(1).step_by(2).all(|&r| r == 0.0));
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let crossings = left
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        assert!(
            (4300..4560).contains(&crossings),
            "frequenza {crossings} Hz"
        );
    }

    #[test]
    fn dma_spento_silenzio() {
        let ram = vec![0u8; 0x1000];
        let mut p = Paula::new(48_000);
        p.enabled = true;
        p.run(100_000, &ram);
        assert!(p.take().iter().all(|&s| s == 0.0));
    }
}
