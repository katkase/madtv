//! Funzioni del gioco riscritte in Rust ("agganci").
//!
//! Quando la CPU emulata sta per eseguire l'indirizzo di un aggancio, invece
//! dell'istruzione originale viene chiamata la funzione Rust corrispondente,
//! che può:
//! - fare il lavoro e restituire i cicli da contare: la macchina emula poi
//!   l'`rts` e il gioco prosegue dal chiamante;
//! - rinunciare (`None`): si esegue il codice originale, come se l'aggancio non
//!   ci fosse (casi limite, versione diversa del gioco).
//!
//! Ogni aggancio controlla i primi byte del codice originale prima di agire:
//! l'eseguibile si decomprime da solo all'avvio (Imploder), quindi il codice
//! esiste solo da quel momento, e su un'altra versione del gioco gli indirizzi
//! potrebbero non corrispondere.
//!
//! È il primo passo della riscrittura graduale del gioco in Rust: si parte dalle
//! funzioni in cui il 68000 a 7 MHz è lento, con risultato in memoria identico
//! all'originale. `--originale` disattiva tutti gli agganci.

use crate::bus::Bus;
use m68k::CpuCore;

/// Funzione Rust al posto di una routine del gioco. Restituisce i cicli di
/// CPU da contare, oppure None per eseguire la routine originale.
pub type HookFn = fn(&mut CpuCore, &mut Bus) -> Option<u64>;

#[derive(Clone, Copy)]
pub struct Hook {
    /// Offset dell'ingresso della routine dall'inizio del codice del gioco
    /// (primo segmento dopo il decompressore: `$003198` nel nostro caricamento).
    pub offset: u32,
    /// Indirizzo assoluto (calcolato al caricamento).
    pub addr: u32,
    /// Primi byte attesi all'ingresso (controllo della versione).
    pub signature: &'static [u8],
    pub name: &'static str,
    pub f: HookFn,
}

/// Agganci noti, con l'indirizzo calcolato dalla base del codice del gioco.
pub fn game_hooks(code_base: u32) -> Vec<Hook> {
    let mut v = vec![Hook {
        offset: 0x3DC7A,
        addr: 0,
        // move.l d4,-(a7) ; move.l a1,d4 ; btst #0,d4
        signature: &[0x2F, 0x04, 0x28, 0x09, 0x08, 0x04, 0x00, 0x00],
        name: "copia di un rettangolo di schermo ($040E12)",
        f: copy_rect,
    }];
    for h in &mut v {
        h.addr = code_base + h.offset;
    }
    v
}

/// Controlla che all'indirizzo ci sia il codice atteso.
pub fn signature_ok(bus: &Bus, h: &Hook) -> bool {
    h.signature
        .iter()
        .enumerate()
        .all(|(i, &b)| bus.peek8(h.addr + i as u32) == b)
}

/// `$040E12`: copia un rettangolo di memoria dello schermo (salvataggio e
/// ripristino dello sfondo sotto pannelli, etichette e finestre di dettaglio).
///
/// Ingresso: a1 sorgente, a2 destinazione, d0.w gruppi di 5 righe (una per
/// bitplane, bitmap interleaved), d1.w byte per riga, d2.w/d3.w modulo di
/// sorgente e destinazione (byte da saltare a fine riga).
///
/// L'originale, se indirizzi, larghezza e moduli sono pari, usa il blitter
/// (istantaneo nel nostro emulatore: lì lasciamo l'originale); altrimenti copia
/// **un byte alla volta** con la CPU, ed è ciò che rende lente le finestre di
/// dettaglio grandi (fino a ~1 secondo). Qui facciamo la stessa copia, nello
/// stesso ordine, e lasciamo i registri come li lascerebbe il ciclo originale.
pub fn copy_rect(cpu: &mut CpuCore, bus: &mut Bus) -> Option<u64> {
    let a1 = cpu.a(1);
    let a2 = cpu.a(2);
    let rows = cpu.d(0) as u16;
    let width = cpu.d(1) as u16;
    let d2 = cpu.d(2) as u16 as i16 as i32;
    let d3 = cpu.d(3) as u16 as i16 as i32;
    let even = a1 & 1 == 0 && a2 & 1 == 0 && width & 1 == 0 && d2 & 1 == 0 && d3 & 1 == 0;
    if even || rows == 0 || width == 0 {
        // Percorso del blitter, o casi limite (0 righe = 65536 giri nel ciclo
        // originale): esegue la routine originale.
        return None;
    }
    let (mut src, mut dst) = (a1, a2);
    for _ in 0..rows as u32 * 5 {
        for _ in 0..width {
            let b = bus.peek8(src);
            bus.poke8(dst, b);
            src = src.wrapping_add(1);
            dst = dst.wrapping_add(1);
        }
        src = src.wrapping_add_signed(d2);
        dst = dst.wrapping_add_signed(d3);
    }
    // L'originale salva d4 sulla pila e lo ripristina: il valore resta nella
    // memoria sotto il puntatore della pila. Lo scriviamo anche noi, così il
    // contenuto della RAM coincide byte per byte.
    let sp = cpu.a(7);
    bus.poke32(sp.wrapping_sub(4), cpu.d(4));
    // Stato di uscita del ciclo a byte: a1/a2 avanzati, d0.w = -1 (fine del
    // dbra), d1.w = larghezza − 1; d4 è salvato e ripristinato dall'originale.
    cpu.set_a(1, src);
    cpu.set_a(2, dst);
    cpu.set_d(0, (cpu.d(0) & 0xFFFF_0000) | 0xFFFF);
    cpu.set_d(1, (cpu.d(1) & 0xFFFF_0000) | (width - 1) as u32);
    // Costo simbolico: come una copia fatta dal blitter, non dalla CPU.
    Some(40 + rows as u64 * 5 * width as u64 / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copia_dispari_come_l_originale() {
        let mut bus = Bus::new();
        let mut cpu = CpuCore::new();
        for i in 0..2000u32 {
            bus.poke8(0x10000 + i, (i * 7) as u8);
        }
        // 2 gruppi di 5 righe da 3 byte, sorgente dispari, moduli 1 e 5.
        cpu.set_a(1, 0x10001);
        cpu.set_a(2, 0x20000);
        cpu.set_d(0, 2);
        cpu.set_d(1, 3);
        cpu.set_d(2, 1);
        cpu.set_d(3, 5);
        assert!(copy_rect(&mut cpu, &mut bus).is_some());
        for r in 0..10u32 {
            for b in 0..3u32 {
                let s = 0x10001 + r * 4 + b;
                let d = 0x20000 + r * 8 + b;
                assert_eq!(bus.peek8(d), bus.peek8(s), "riga {r} byte {b}");
            }
            // I byte saltati dal modulo restano intatti.
            assert_eq!(bus.peek8(0x20000 + r * 8 + 3), 0);
        }
        assert_eq!(cpu.a(1), 0x10001 + 10 * 4);
        assert_eq!(cpu.a(2), 0x20000 + 10 * 8);
        assert_eq!(cpu.d(0) & 0xFFFF, 0xFFFF);
        assert_eq!(cpu.d(1) & 0xFFFF, 2);
        // Caso pari: lasciato al blitter dell'originale.
        cpu.set_a(1, 0x10000);
        cpu.set_d(1, 4);
        cpu.set_d(2, 0);
        cpu.set_d(3, 4);
        assert!(copy_rect(&mut cpu, &mut bus).is_none());
    }
}
