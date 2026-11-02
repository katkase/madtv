//! Misura quanti frame impiega una routine del gioco (dall'ingresso al ritorno)
//! con e senza le funzioni riscritte in Rust, alle sue chiamate reali in una
//! registrazione. Uso: cargo run --release --example hook_speed -- rec.txt $INDIRIZZO [max]
use madtv::adf::Adf;
use madtv::input;
use madtv::machine::Machine;
use madtv::os::vfs::Vfs;
#[path = "drag_lab.rs"]
#[allow(dead_code)]
mod lab;

fn frames_to_return(mut c: Machine, hooks: bool) -> (u64, u64) {
    c.hooks_enabled = hooks;
    c.bus.custom.paula.enabled = false;
    let (ret, sp0, f0, i0) = (
        c.bus.peek32(c.cpu.a(7)),
        c.cpu.a(7),
        c.bus.custom.frame,
        c.instructions,
    );
    let mut hist: std::collections::HashMap<u32, u64> = Default::default();
    c.run(c.instructions + 1, u64::MAX);
    while !(c.cpu.pc == ret && c.cpu.a(7) == sp0 + 4) {
        *hist.entry(c.cpu.pc & !0x3F).or_default() += 1;
        c.run(c.instructions + 1, u64::MAX);
    }
    if !hooks && std::env::var("PROFILO").is_ok() {
        let mut v: Vec<_> = hist.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        let tot: u64 = v.iter().map(|x| x.1).sum();
        for (pc, n) in v.iter().take(12) {
            println!(
                "   ${pc:06X}-${:06X}: {:.1}%",
                pc + 0x3F,
                *n as f64 * 100.0 / tot as f64
            );
        }
    }
    (c.bus.custom.frame - f0, c.instructions - i0)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let script =
        input::expand(input::parse_script(&std::fs::read_to_string(&a[1]).unwrap()).unwrap());
    let target = u32::from_str_radix(a[2].trim_start_matches('$'), 16).unwrap();
    let max: usize = a.get(3).map(|s| s.parse().unwrap()).unwrap_or(5);
    let last = script.last().unwrap().0;
    let mut disks: Vec<Adf> = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok())
        .collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "/tmp/speedsave".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    m.probe_pcs = vec![target];
    let mut next = 0;
    let mut done = 0;
    while m.bus.custom.frame < last && done < max {
        lab::apply(&mut m, &script, &mut next);
        let seen = m.probe_log.len();
        let mut snap = None;
        // Esegue il frame un'istruzione alla volta solo se la routine viene chiamata.
        let frame_end = m.bus.custom.frame + 1;
        let probe = m.clone();
        m.run_frame();
        if m.probe_log.len() > seen {
            let mut p = probe;
            p.probe_pcs.clear();
            while p.cpu.pc != target {
                p.run(p.instructions + 1, u64::MAX);
            }
            snap = Some(p);
        }
        let _ = frame_end;
        if let Some(s) = snap {
            let (fo, io) = frames_to_return(s.clone(), false);
            let (fr, ir) = frames_to_return(s, true);
            println!(
                "frame {}: originale {fo} frame ({io} istruzioni), con Rust {fr} frame ({ir} istruzioni)",
                m.bus.custom.frame
            );
            done += 1;
        }
    }
}
