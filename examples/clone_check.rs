//! Verifica che `Machine::clone` produca una copia che evolve identica
//! all'originale con gli stessi input. Uso: cargo run --release --example clone_check -- rec/palinsesto.txt 2000 500
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
    let at: u64 = a[2].parse().unwrap();
    let span: u64 = a[3].parse().unwrap();
    let mut disks: Vec<Adf> = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok())
        .collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "/tmp/clonesave".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    let mut next = 0;
    while m.bus.custom.frame < at {
        lab::apply(&mut m, &script, &mut next);
        m.run_frame();
    }
    let t = std::time::Instant::now();
    let mut c = m.clone();
    println!("clone in {:?}", t.elapsed());
    let mut next_c = next;
    for _ in 0..span {
        lab::apply(&mut m, &script, &mut next);
        m.run_frame();
        lab::apply(&mut c, &script, &mut next_c);
        c.run_frame();
    }
    let diff = m
        .bus
        .ram
        .iter()
        .zip(&c.bus.ram)
        .filter(|(x, y)| x != y)
        .count();
    let scr = m.bus.custom.display.front == c.bus.custom.display.front;
    println!(
        "frame {} byte di RAM diversi: {diff}, schermo identico: {scr}, pc {:06X}/{:06X}",
        m.bus.custom.frame, m.cpu.pc, c.cpu.pc
    );
}
