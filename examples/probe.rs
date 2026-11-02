//! Rigioca una registrazione e stampa, a ogni cambiamento, il valore di alcune
//! variabili (longword) e la posizione del puntatore.
//! Uso: cargo run --release --example probe -- rec/x.txt FRAME_FINE $0448EC [$...] [dump=FRAME:$ADDR:LEN]...
use madtv::adf::Adf;
use madtv::input;
use madtv::machine::Machine;
use madtv::os::vfs::Vfs;
#[path = "drag_lab.rs"]
#[allow(dead_code)]
mod lab;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s.trim_start_matches('$'), 16).unwrap()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let script =
        input::expand(input::parse_script(&std::fs::read_to_string(&a[1]).unwrap()).unwrap());
    let last: u64 = a[2].parse().unwrap();
    let mut addrs = vec![];
    let mut dumps = vec![];
    let mut watch: Option<(u32, u32, u64, u64)> = None;
    let mut probes = vec![];
    for s in &a[3..] {
        if let Some(b) = s.strip_prefix("break=") {
            probes.push(hex(b));
        } else if let Some(w) = s.strip_prefix("watch=") {
            let p: Vec<&str> = w.split(':').collect();
            let (f0, f1) = p[2].split_once('-').unwrap();
            watch = Some((
                hex(p[0]),
                p[1].parse().unwrap(),
                f0.parse().unwrap(),
                f1.parse().unwrap(),
            ));
        } else if let Some(d) = s.strip_prefix("dump=") {
            let p: Vec<&str> = d.split(':').collect();
            dumps.push((
                p[0].parse::<u64>().unwrap(),
                hex(p[1]),
                p[2].parse::<u32>().unwrap(),
            ));
        } else {
            addrs.push(hex(s));
        }
    }
    let mut disks: Vec<Adf> = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok())
        .collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "/tmp/probesave".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    m.probe_pcs = probes.clone();
    let mut next = 0;
    let mut prev: Vec<u32> = vec![u32::MAX; addrs.len()];
    while m.bus.custom.frame <= last {
        lab::apply(&mut m, &script, &mut next);
        if let Some((wa, wl, f0, f1)) = watch {
            let f = m.bus.custom.frame;
            m.bus.watch = if f >= f0 && f <= f1 {
                (wa, wa + wl)
            } else {
                (0, 0)
            };
        }
        m.run_frame();
        let f = m.bus.custom.frame;
        let cur: Vec<u32> = addrs.iter().map(|&a| m.bus.peek32(a)).collect();
        if cur != prev {
            let s: Vec<String> = addrs
                .iter()
                .zip(&cur)
                .map(|(a, v)| format!("${a:06X}={v:08X}"))
                .collect();
            println!("{f:5} ptr {:?} {}", m.game_pointer(), s.join(" "));
            prev = cur;
        }
        for &(df, da, dl) in &dumps {
            if df == f {
                let a = if da == 0 { m.bus.peek32(addrs[0]) } else { da };
                println!("--- dump {f} ${a:06X}");
                for row in (0..dl).step_by(16) {
                    let b: Vec<String> = (0..16)
                        .map(|i| format!("{:02X}", m.bus.peek8(a + row + i)))
                        .collect();
                    let t: String = (0..16)
                        .map(|i| {
                            let c = m.bus.peek8(a + row + i);
                            if (32..127).contains(&c) {
                                c as char
                            } else {
                                '.'
                            }
                        })
                        .collect();
                    println!("{:06X}: {} {}", a + row, b.join(" "), t);
                }
            }
        }
    }
    for (f, pc, st) in &m.probe_log {
        let s: Vec<String> = st.iter().map(|v| format!("{v:08X}")).collect();
        println!("break ${pc:06X} frame {f} ptr? stack {}", s.join(" "));
    }
    if watch.is_some() {
        let mut c: std::collections::BTreeMap<(u32, u8), (u32, u64, u64)> = Default::default();
        for &(f, pc, b) in &m.watch_log {
            let e = c.entry((pc, b)).or_insert((0, f, f));
            e.0 += 1;
            e.2 = f;
        }
        println!("--- accessi osservati (PC, r=1 w=2, volte, primo frame, ultimo)");
        for ((pc, b), (n, f0, f1)) in c {
            println!("  ${pc:06X} {b} x{n} frame {f0}-{f1}");
        }
    }
}
