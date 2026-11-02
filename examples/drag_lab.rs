//! Laboratorio del trascinamento: rigioca una registrazione e cerca nella memoria
//! le variabili legate al pannello trascinato.
//! Uso: cargo run --release --example drag_lab -- rec/palinsesto.txt "2060-2290,2960-3090" "1950-2030,2320-2880,3110-3250"
//! Primo intervallo: frame con pannello agganciato; secondo: frame senza.
use madtv::adf::Adf;
use madtv::input::{self, Action};
use madtv::machine::Machine;
use madtv::os::vfs::Vfs;

const MEM: usize = 0x100000;

fn ranges(s: &str) -> Vec<(u64, u64)> {
    s.split(',')
        .map(|r| {
            let (a, b) = r.split_once('-').unwrap();
            (a.parse().unwrap(), b.parse().unwrap())
        })
        .collect()
}
fn inside(r: &[(u64, u64)], f: u64) -> bool {
    r.iter().any(|&(a, b)| f >= a && f <= b)
}

pub fn apply(m: &mut Machine, script: &[(u64, Action)], next: &mut usize) {
    let frame = m.bus.custom.frame;
    while *next < script.len() && script[*next].0 <= frame {
        let c = &mut m.bus.custom;
        match &script[*next].1 {
            Action::Move(dx, dy) => input::mouse_move(c, *dx, *dy),
            Action::Clock(t) => m.os.clock_override = Some(*t),
            Action::Cpu(None) => {
                m.cpu_speed = 100;
                m.auto_turbo = true;
            }
            Action::Cpu(Some(p)) => {
                m.cpu_speed = *p;
                m.auto_turbo = false;
            }
            Action::LeftDown => c.left_button = true,
            Action::LeftUp => c.left_button = false,
            Action::RightDown => c.right_button = true,
            Action::RightUp => c.right_button = false,
            Action::KeyDown(k) => input::key(c, *k, true),
            Action::KeyUp(k) => input::key(c, *k, false),
            _ => {}
        }
        *next += 1;
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let script =
        input::expand(input::parse_script(&std::fs::read_to_string(&a[1]).unwrap()).unwrap());
    let drag = ranges(&a[2]);
    let idle = ranges(&a[3]);
    let last = drag.iter().chain(idle.iter()).map(|r| r.1).max().unwrap();
    let mut disks: Vec<Adf> = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok())
        .collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "/tmp/dragsave".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    let n = MEM / 2;
    // Correlazione con il puntatore: offset costante durante il trascinamento.
    let mut offx: Vec<Option<i32>> = vec![None; n];
    let mut okx = vec![true; n];
    let mut offy: Vec<Option<i32>> = vec![None; n];
    let mut oky = vec![true; n];
    // Bandiere: valore costante nel trascinamento, diverso a riposo.
    let mut vd: Vec<Option<u16>> = vec![None; n];
    let mut okd = vec![true; n];
    let mut idle_vals: Vec<Vec<u16>> = vec![Vec::new(); n];
    let mut next = 0;
    let mut pxs = std::collections::BTreeSet::new();
    while m.bus.custom.frame <= last {
        apply(&mut m, &script, &mut next);
        m.run_frame();
        let f = m.bus.custom.frame;
        let (px, py) = m.game_pointer().unwrap_or((-1, -1));
        let w = |i: usize| m.bus.peek16((i * 2) as u32);
        if inside(&drag, f) {
            pxs.insert(px);
            for i in 0..n {
                let v = w(i) as i16 as i32;
                if okx[i] {
                    match offx[i] {
                        None => offx[i] = Some(v - px),
                        Some(o) => {
                            if o != v - px {
                                okx[i] = false
                            }
                        }
                    }
                }
                if oky[i] {
                    match offy[i] {
                        None => offy[i] = Some(v - py),
                        Some(o) => {
                            if o != v - py {
                                oky[i] = false
                            }
                        }
                    }
                }
                if okd[i] {
                    match vd[i] {
                        None => vd[i] = Some(w(i)),
                        Some(o) => {
                            if o != w(i) {
                                okd[i] = false
                            }
                        }
                    }
                }
            }
        } else if inside(&idle, f) {
            for i in 0..n {
                if okd[i] {
                    let v = w(i);
                    if Some(v) == vd[i] {
                        okd[i] = false
                    } else if idle_vals[i].len() < 4 && !idle_vals[i].contains(&v) {
                        idle_vals[i].push(v)
                    }
                }
            }
        }
    }
    println!(
        "valori distinti di x durante il trascinamento: {}",
        pxs.len()
    );
    for i in 0..n {
        if okx[i] {
            println!("X  ${:06X} = px{:+}", i * 2, offx[i].unwrap());
        }
        if oky[i] {
            println!("Y  ${:06X} = py{:+}", i * 2, offy[i].unwrap());
        }
    }
    for i in 0..n {
        if okd[i] && !idle_vals[i].is_empty() {
            println!(
                "F  ${:06X} trascinamento={:04X} riposo={:04X?}",
                i * 2,
                vd[i].unwrap(),
                idle_vals[i]
            );
        }
    }
}
