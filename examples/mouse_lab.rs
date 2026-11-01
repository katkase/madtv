//! Laboratorio per il mouse: simula l'inseguimento del puntatore della finestra
//! e stampa, frame per frame, bersaglio, puntatore del gioco e conteggi inviati.
//! Uso: cargo run --release --example mouse_lab
use madtv::adf::Adf;
use madtv::input;
use madtv::machine::Machine;
use madtv::os::vfs::Vfs;

fn main() {
    let mut disks: Vec<Adf> = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok())
        .collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "save".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    while m.bus.custom.frame < 1290 {
        m.run_frame();
    }
    let targets: Vec<(i32, i32)> = vec![(200, 150); 12]
        .into_iter()
        .chain(vec![(330, 260); 15])
        .chain(vec![(-10, -10); 15])
        .collect();
    let mut sync = input::PointerSync::default();
    for (tx, ty) in targets {
        let p = m.game_pointer().unwrap();
        let (dx, dy) = sync.step(p, (tx, ty));
        input::mouse_move(&mut m.bus.custom, dx, dy);
        m.run_frame();
        let q = m.game_pointer().unwrap();
        println!(
            "frame {} bersaglio ({tx},{ty}) puntatore {:?} -> inviati ({dx},{dy}) -> dopo {:?}",
            m.bus.custom.frame, p, q
        );
    }
}
