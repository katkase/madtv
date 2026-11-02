//! Prova il click di prova lungo (punto d'arrivo) in un frame di una registrazione.
//! Uso: cargo run --release --example landing_check -- rec.txt FRAME
use madtv::adf::Adf;
use madtv::enhance;
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
    let at: u64 = a[2].parse().unwrap();
    let mut disks: Vec<Adf> = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok())
        .collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "/tmp/lcsave".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    let mut next = 0;
    while m.bus.custom.frame < at {
        lab::apply(&mut m, &script, &mut next);
        m.run_frame();
    }
    let d = enhance::drag_state(&m).expect("niente in mano");
    let s = enhance::panel_samples(&m, &d);
    let here = enhance::find_panel(&m.bus.custom.display.front, d.size, &s);
    let t = std::time::Instant::now();
    let r = enhance::probe_landing_on(m.clone(), d, s.clone());
    println!(
        "{d:?}\ncampioni {} ritrovato qui {here:?}\narrivo {r:?} in {:?}",
        s.len(),
        t.elapsed()
    );
}
