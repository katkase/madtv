//! exec.library emulata: memoria, liste, task/segnali (semplificati), librerie,
//! interrupt.
//!
//! Il gioco gira come unico task: le primitive di multitasking (Wait, Signal,
//! messaggi) sono ridotte al minimo indispensabile e documentate una per una.

use super::{Ctx, LibId, Os};

/// Offset nella struttura ExecBase.
pub const EB_THISTASK: u32 = 0x114;
pub const EB_ATTNFLAGS: u32 = 0x128;
pub const EB_VBLANKFREQ: u32 = 0x212;
pub const EB_POWERFREQ: u32 = 0x213;
pub const EB_POSSIZE: u32 = 0x260;

const MEMF_CLEAR: u32 = 1 << 16;
const MEMF_LARGEST: u32 = 1 << 17;

/// Esegue la funzione `idx` di exec. Restituisce una descrizione per il log,
/// oppure None se la funzione non è implementata.
pub fn call(os: &mut Os, c: &mut Ctx, idx: u16) -> Option<String> {
    Some(match idx {
        // Supervisor(userFunc a5): esegue a5 in modo supervisore; la funzione termina con RTE.
        0 => {
            let ret = c.bus.peek32(c.a(7));
            c.cpu.set_a(7, c.a(7) + 4);
            let sr = c.cpu.get_sr();
            c.cpu.set_sr(sr | 0x2000);
            let ssp = c.cpu.a(7) - 6;
            c.cpu.set_a(7, ssp);
            c.bus.poke16(ssp, sr);
            c.bus.poke32(ssp + 2, ret);
            {
                c.cpu.pc = c.a(5);
                c.cpu.invalidate_prefetch();
            }
            format!("JUMP func={:06x}", c.a(5))
        }
        // Disable/Enable/Forbid/Permit: nessun altro task né interrupt "di sistema" da bloccare.
        15..=18 => String::new(),
        // SetSR(newSR d0, mask d1) -> vecchio SR
        19 => {
            let old = c.cpu.get_sr() as u32;
            let mask = c.d(1) & 0xFFFF;
            let new = (old & !mask) | (c.d(0) & mask);
            c.cpu.set_sr(new as u16);
            c.set_d0(old);
            format!("({:04x},{:04x})", c.d(0), mask)
        }
        // SuperState() -> vecchio SSP; il chiamante continua in modo supervisore.
        20 => {
            let ret = c.bus.peek32(c.a(7));
            let usp = c.a(7) + 4;
            let sr = c.cpu.get_sr();
            c.cpu.set_sr(sr | 0x2000);
            let ssp = c.cpu.a(7);
            c.cpu.set_a(7, usp);
            {
                c.cpu.pc = ret;
                c.cpu.invalidate_prefetch();
            }
            c.set_d0(ssp);
            "JUMP".to_string()
        }
        // UserState(sysStack d0): torna in modo utente.
        21 => {
            let ret = c.bus.peek32(c.a(7));
            let usp = c.a(7) + 4;
            let sr = c.cpu.get_sr();
            c.cpu.set_a(7, c.d(0));
            c.cpu.set_sr(sr & !0x2000);
            c.cpu.set_a(7, usp);
            {
                c.cpu.pc = ret;
                c.cpu.invalidate_prefetch();
            }
            "JUMP".to_string()
        }
        // SetIntVector(intNumber d0, interrupt a1) -> vecchio interrupt
        22 => {
            c.set_d0(0);
            format!("(int={}, is={:06x}) [ignorato]", c.d(0), c.a(1))
        }
        // AddIntServer(intNumber d0, interrupt a1)
        23 => {
            let n = c.d(0);
            let is = c.a(1);
            let data = c.bus.peek32(is + 14);
            let code = c.bus.peek32(is + 18);
            os.int_servers.entry(n).or_default().push((data, code, is));
            if n == 5 {
                c.bus.custom.intena |= 0x4020; // INTEN + VERTB
            }
            format!("(int={n}, is={is:06x}, code={code:06x}, data={data:06x})")
        }
        // RemIntServer(intNumber d0, interrupt a1)
        24 => {
            let (n, is) = (c.d(0), c.a(1));
            if let Some(v) = os.int_servers.get_mut(&n) {
                v.retain(|x| x.2 != is);
            }
            format!("(int={n}, is={is:06x})")
        }
        // AllocMem(byteSize d0, requirements d1) -> indirizzo
        28 => {
            let (size, req) = (c.d(0), c.d(1));
            let a = os.heap.alloc(size).unwrap_or(0);
            if a != 0 && req & MEMF_CLEAR != 0 {
                for i in 0..size {
                    c.bus.poke8(a + i, 0);
                }
            }
            c.set_d0(a);
            format!("(size={size}, req={req:x})")
        }
        // AllocAbs(byteSize d0, location a1)
        29 => {
            let a = os.heap.alloc_at(c.a(1), c.d(0)).unwrap_or(0);
            c.set_d0(a);
            format!("(size={}, at={:06x})", c.d(0), c.a(1))
        }
        // FreeMem(memoryBlock a1, byteSize d0)
        30 => {
            let ok = os.heap.free(c.a(1));
            format!(
                "({:06x}, {}){}",
                c.a(1),
                c.d(0),
                if ok { "" } else { " [blocco sconosciuto]" }
            )
        }
        // AvailMem(requirements d1)
        31 => {
            let v = if c.d(1) & MEMF_LARGEST != 0 {
                os.heap.largest()
            } else {
                os.heap.available()
            };
            c.set_d0(v);
            format!("(req={:x})", c.d(1))
        }
        // Insert(list a0, node a1, pred a2)
        34 => {
            let (list, node, pred) = (c.a(0), c.a(1), c.a(2));
            if pred == 0 {
                list_add_head(c, list, node);
            } else {
                let succ = c.bus.peek32(pred);
                c.bus.poke32(node, succ);
                c.bus.poke32(node + 4, pred);
                c.bus.poke32(succ + 4, node);
                c.bus.poke32(pred, node);
            }
            String::new()
        }
        35 => {
            list_add_head(c, c.a(0), c.a(1));
            String::new()
        }
        36 => {
            list_add_tail(c, c.a(0), c.a(1));
            String::new()
        }
        37 => {
            list_remove(c, c.a(1));
            String::new()
        }
        // RemHead(list a0) -> nodo o 0
        38 => {
            let list = c.a(0);
            let head = c.bus.peek32(list);
            if c.bus.peek32(head) == 0 {
                c.set_d0(0);
            } else {
                list_remove(c, head);
                c.set_d0(head);
            }
            String::new()
        }
        // RemTail(list a0)
        39 => {
            let list = c.a(0);
            let tail = c.bus.peek32(list + 8);
            if c.bus.peek32(tail + 4) == 0 {
                c.set_d0(0);
            } else {
                list_remove(c, tail);
                c.set_d0(tail);
            }
            String::new()
        }
        // Enqueue(list a0, node a1): inserimento per priorità (ln_Pri, byte all'offset 9)
        40 => {
            let (list, node) = (c.a(0), c.a(1));
            let pri = c.bus.peek8(node + 9) as i8;
            let mut n = c.bus.peek32(list);
            while c.bus.peek32(n) != 0 && (c.bus.peek8(n + 9) as i8) >= pri {
                n = c.bus.peek32(n);
            }
            let pred = c.bus.peek32(n + 4);
            c.bus.poke32(node, n);
            c.bus.poke32(node + 4, pred);
            c.bus.poke32(pred, node);
            c.bus.poke32(n + 4, node);
            String::new()
        }
        // FindName(list a0, name a1)
        41 => {
            let target = c.bus.cstr(c.a(1));
            let mut n = c.bus.peek32(c.a(0));
            let mut found = 0;
            while n != 0 && c.bus.peek32(n) != 0 {
                let np = c.bus.peek32(n + 10);
                if np != 0 && c.bus.cstr(np) == target {
                    found = n;
                    break;
                }
                n = c.bus.peek32(n);
            }
            c.set_d0(found);
            format!("(\"{target}\")")
        }
        // FindTask(name a1): con a1=0 restituisce il task corrente (il nostro processo).
        44 => {
            let name = if c.a(1) == 0 {
                None
            } else {
                Some(c.bus.cstr(c.a(1)))
            };
            c.set_d0(if name.is_none() { os.process } else { 0 });
            format!("({name:?})")
        }
        // SetTaskPri(task a1, pri d0) -> vecchia priorità
        45 => {
            let old = c.bus.peek8(c.a(1) + 9) as i8 as i32 as u32;
            c.bus.poke8(c.a(1) + 9, c.d(0) as u8);
            c.set_d0(old);
            String::new()
        }
        // SetSignal(newSignals d0, signalMask d1) -> segnali precedenti
        46 => {
            let task = os.process;
            let old = c.bus.peek32(task + 26);
            let mask = c.d(1);
            c.bus.poke32(task + 26, (old & !mask) | (c.d(0) & mask));
            c.set_d0(old);
            format!("({:x},{:x})", c.d(0), mask)
        }
        // Wait(signalSet d0): non esistendo altri task, restituisce subito i segnali richiesti.
        48 => {
            let set = c.d(0);
            format!("({set:x}) [ritorno immediato]")
        }
        // Signal(task a1, signals d0)
        49 => {
            let task = c.a(1);
            let v = c.bus.peek32(task + 26) | c.d(0);
            c.bus.poke32(task + 26, v);
            String::new()
        }
        // AllocSignal(signalNum d0) -> numero o -1
        50 => {
            let want = c.d(0) as i32;
            let n = if want >= 0 {
                if os.signals_alloc & (1 << want) == 0 {
                    want
                } else {
                    -1
                }
            } else {
                (16..32)
                    .find(|i| os.signals_alloc & (1 << i) == 0)
                    .unwrap_or(-1)
            };
            if n >= 0 {
                os.signals_alloc |= 1 << n;
            }
            c.set_d0(n as u32);
            format!("({want})")
        }
        // FreeSignal(signalNum d0)
        51 => {
            let n = c.d(0) as i32;
            if (0..32).contains(&n) {
                os.signals_alloc &= !(1u32 << n);
            }
            String::new()
        }
        // AddPort / RemPort / PutMsg / ReplyMsg: nessun destinatario reale.
        54 | 55 | 56 | 58 => format!("({:06x}) [ignorato]", c.a(1)),
        // GetMsg(port a0): nessun messaggio in coda.
        57 => {
            c.set_d0(0);
            format!("({:06x})", c.a(0))
        }
        // FindPort(name a1)
        60 => {
            c.set_d0(0);
            format!("(\"{}\")", c.bus.cstr(c.a(1)))
        }
        // OldOpenLibrary(name a1) / OpenLibrary(name a1, version d0)
        63 | 87 => {
            let name = c.bus.cstr(c.a(1));
            let id = match name.to_lowercase().as_str() {
                "exec.library" => Some(LibId::Exec),
                "dos.library" => Some(LibId::Dos),
                "intuition.library" => Some(LibId::Intuition),
                "graphics.library" => Some(LibId::Graphics),
                "icon.library" => Some(LibId::Icon),
                _ => None,
            };
            let base = id.map(|i| os.lib_base(i)).unwrap_or(0);
            c.set_d0(base);
            let ver = if idx == 87 { c.d(0) } else { 0 };
            format!(
                "(\"{name}\", v{ver}){}",
                if base == 0 {
                    " [libreria non disponibile]"
                } else {
                    ""
                }
            )
        }
        // CloseLibrary(library a1)
        64 => String::new(),
        // SetFunction(library a1, funcOffset a0, newFunction d0) -> vecchia funzione
        65 => {
            let off = c.a(0) as i16 as i32;
            let entry = (c.a(1) as i32 + off) as u32;
            let new = c.d(0);
            // La "vecchia funzione" è la voce stessa: chiamarla esegue ancora la trap originale.
            // Per poterla ancora chiamare dopo la sostituzione, copiamo la voce originale altrove.
            let saved = os.alloc_clear(c.bus, 8);
            for i in 0..6 {
                let b = c.bus.peek8(entry + i);
                c.bus.poke8(saved + i, b);
            }
            c.bus.poke16(entry, 0x4EF9); // jmp new.l
            c.bus.poke32(entry + 2, new);
            c.set_d0(saved);
            format!("(lib={:06x}, off={off}, new={new:06x})", c.a(1))
        }
        // OpenDevice(devName a0, unit d0, ioRequest a1, flags d1) -> errore (0 = ok)
        69 => {
            let name = c.bus.cstr(c.a(0));
            // Per ora nessun device: restituisce IOERR_OPENFAIL (-1).
            c.set_d0(0xFFFF_FFFF);
            format!("(\"{name}\", unit={}) [device non emulato]", c.d(0))
        }
        // CloseDevice(ioRequest a1)
        70 => String::new(),
        // TypeOfMem(address a1) -> MEMF_PUBLIC|MEMF_CHIP
        84 => {
            c.set_d0(if (c.a(1) as usize) < crate::bus::RAM_SIZE {
                3
            } else {
                0
            });
            String::new()
        }
        // Alert(alertNum d7)
        13 => {
            let n = c.d(7);
            let desc = format!("(${n:08x})");
            if n & 0x8000_0000 != 0 {
                return Some(format!("EXIT: 20 Alert{desc}"));
            }
            desc
        }
        _ => return None,
    })
}

fn list_add_head(c: &mut Ctx, list: u32, node: u32) {
    let head = c.bus.peek32(list);
    c.bus.poke32(node, head);
    c.bus.poke32(node + 4, list);
    c.bus.poke32(head + 4, node);
    c.bus.poke32(list, node);
}

fn list_add_tail(c: &mut Ctx, list: u32, node: u32) {
    let tailpred = c.bus.peek32(list + 8);
    c.bus.poke32(node, list + 4);
    c.bus.poke32(node + 4, tailpred);
    c.bus.poke32(tailpred, node);
    c.bus.poke32(list + 8, node);
}

fn list_remove(c: &mut Ctx, node: u32) {
    let succ = c.bus.peek32(node);
    let pred = c.bus.peek32(node + 4);
    c.bus.poke32(pred, succ);
    c.bus.poke32(succ + 4, pred);
}

/// Inizializza una struct List vuota.
pub fn new_list(bus: &mut crate::bus::Bus, list: u32) {
    bus.poke32(list, list + 4);
    bus.poke32(list + 4, 0);
    bus.poke32(list + 8, list);
}
