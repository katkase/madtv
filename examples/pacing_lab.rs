//! Rigioca una registrazione e stampa, per ogni frame, quante volte il gioco ha
//! chiamato la sua funzione di sincronizzazione sul contatore dei frame
//! (`Machine::pacing_pc`) e i tratti in cui non l'ha chiamata (gioco "in
//! ritardo": sta calcolando o disegnando).
//! Uso: cargo run --release --example pacing_lab -- rec.txt [DA A]
use madtv::adf::Adf;
use madtv::input;
use madtv::machine::Machine;
use madtv::os::vfs::Vfs;
#[path = "drag_lab.rs"]
#[allow(dead_code)]
mod lab;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let script =
        input::expand(input::parse_script(&std::fs::read_to_string(&a[1]).unwrap()).unwrap());
    let from: u64 = a.get(2).map(|s| s.parse().unwrap()).unwrap_or(0);
    let to: u64 = a.get(3).map(|s| s.parse().unwrap()).unwrap_or(u64::MAX);
    let last = script.last().map(|x| x.0).unwrap_or(0).min(to);
    let mut disks: Vec<Adf> = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok())
        .collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "/tmp/pacesave".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    let mut next = 0;
    let mut run_start: Option<u64> = None;
    let mut hist = std::collections::BTreeMap::new();
    while m.bus.custom.frame < last {
        lab::apply(&mut m, &script, &mut next);
        let before = m.pacing_calls;
        let turbo = m.turbo_active;
        m.run_frame();
        let f = m.bus.custom.frame;
        let calls = m.pacing_calls - before;
        if f < from || f < 450 {
            continue;
        }
        match (calls, run_start) {
            (0, None) => run_start = Some(f),
            (c, Some(s)) if c > 0 => {
                let len = f - s;
                *hist.entry(len.min(50)).or_insert(0) += 1;
                if len >= 5 {
                    println!(
                        "frame {s}-{f}: {len} frame senza sincronizzarsi{} ptr {:?}",
                        if turbo { " (turbo)" } else { "" },
                        m.game_pointer()
                    );
                }
                run_start = None;
            }
            _ => {}
        }
    }
    println!("lunghezza dei tratti senza sincronizzazione -> quante volte: {hist:?}");
}
