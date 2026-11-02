//! Prova del "click speculativo": durante un trascinamento clona la macchina,
//! simula il click su una copia e lo confronta con una copia senza click.
//! Uso: cargo run --release --example drop_lab -- rec/palinsesto.txt 2933 3094 [ogni]
use madtv::adf::Adf;
use madtv::input;
use madtv::machine::Machine;
use madtv::os::vfs::Vfs;
#[path = "drag_lab.rs"]
#[allow(dead_code)]
mod lab;

const HELD: u32 = 0x0448EC;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let script =
        input::expand(input::parse_script(&std::fs::read_to_string(&a[1]).unwrap()).unwrap());
    let f0: u64 = a[2].parse().unwrap();
    let f1: u64 = a[3].parse().unwrap();
    let every: u64 = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(5);
    let mut disks: Vec<Adf> = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok())
        .collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "/tmp/dropsave".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    let mut next = 0;
    while m.bus.custom.frame <= f1 {
        lab::apply(&mut m, &script, &mut next);
        m.run_frame();
        let f = m.bus.custom.frame;
        if f < f0 || (f - f0) % every != 0 || m.bus.peek32(HELD) == 0 {
            continue;
        }
        let t = std::time::Instant::now();
        let mut yes = m.clone();
        let mut no = m.clone();
        yes.bus.custom.left_button = true;
        let hold: u32 = std::env::var("HOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut n = 0;
        while (n < 6 && yes.bus.peek32(HELD) != 0) || n < hold {
            yes.run_frame();
            no.run_frame();
            n += 1;
        }
        yes.bus.custom.left_button = false;
        let settle: u32 = std::env::var("SETTLE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        for _ in 0..settle {
            yes.run_frame();
            no.run_frame();
        }
        // Tabella del palinsesto (giocatore 0): 45 word da $04AA60.
        let tab: Vec<String> = (0..90u32)
            .filter(|i| yes.bus.peek16(0x04AA60 + i * 2) != no.bus.peek16(0x04AA60 + i * 2))
            .map(|i| {
                format!(
                    "[{i}] {:04X}->{:04X}",
                    no.bus.peek16(0x04AA60 + i * 2),
                    yes.bus.peek16(0x04AA60 + i * 2)
                )
            })
            .collect();
        if !tab.is_empty() {
            println!("    palinsesto: {}", tab.join(" "));
        }
        if std::env::var("RAMDIFF").is_ok() {
            let mut out = vec![];
            let mut i = 0usize;
            while i < 0x100000 {
                if yes.bus.ram[i] != no.bus.ram[i] {
                    let st = i;
                    while i < 0x100000
                        && (yes.bus.ram[i] != no.bus.ram[i]
                            || yes.bus.ram[i..(i + 8).min(0x100000)]
                                != no.bus.ram[i..(i + 8).min(0x100000)])
                    {
                        i += 1;
                    }
                    out.push(format!("${st:06X}+{}", i - st));
                    if out.len() > 60 {
                        break;
                    }
                }
                i += 1;
            }
            println!("    ram: {}", out.join(" "));
        }
        let accepted = yes.bus.peek32(HELD) == 0;
        if std::env::var("SHOTS").is_ok() {
            std::fs::create_dir_all("/tmp/drop").ok();
            let _ = m
                .bus
                .custom
                .display
                .save_png(std::path::Path::new(&format!("/tmp/drop/{f}a.png")));
            let _ = yes
                .bus
                .custom
                .display
                .save_png(std::path::Path::new(&format!("/tmp/drop/{f}b.png")));
        }
        let (w, s) = (320usize, &yes.bus.custom.display.front);
        let r = &no.bus.custom.display.front;
        let (mut x0, mut y0, mut x1, mut y1, mut cnt) = (w, 999, 0, 0, 0);
        for y in 0..200 {
            for x in 0..w {
                if s[y * w + x] != r[y * w + x] {
                    cnt += 1;
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                }
            }
        }
        let p = m.game_pointer().unwrap();
        let w16 = |a: u32| m.bus.peek16(a) as i16;
        println!(
            "{f} ptr {p:?} disegnato ({},{}) off ({},{}) dim {}x{} -> {} in {n} frame, diff {cnt} px [{x0},{y0}]-[{x1},{y1}] ({:?})",
            w16(0x044656),
            w16(0x044658),
            w16(0x0448F4),
            w16(0x0448F6),
            w16(0x0448F0),
            w16(0x0448F2),
            if accepted { "ACCETTATO" } else { "rifiutato" },
            t.elapsed()
        );
    }
}
