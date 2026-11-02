//! Rigioca una registrazione e, alla fine di ogni frame nell'intervallo dato,
//! stampa il PC e la catena delle chiamate (frame `link a5` di Aztec C).
//! Uso: cargo run --release --example stack_lab -- rec.txt DA A
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
    let from: u64 = a[2].parse().unwrap();
    let to: u64 = a[3].parse().unwrap();
    let mut disks: Vec<Adf> = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok())
        .collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "/tmp/stacksave".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    let mut next = 0;
    let mut count: std::collections::BTreeMap<u32, u32> = Default::default();
    while m.bus.custom.frame < to {
        lab::apply(&mut m, &script, &mut next);
        m.run_frame();
        let f = m.bus.custom.frame;
        if f < from {
            continue;
        }
        let mut chain = vec![];
        let mut a5 = m.cpu.a(5);
        for _ in 0..12 {
            if a5 < 0x1000 || a5 >= 0x200000 {
                break;
            }
            let ret = m.bus.peek32(a5 + 4);
            if !(0x3000..0x45000).contains(&ret) {
                break;
            }
            chain.push(ret);
            *count.entry(ret).or_default() += 1;
            a5 = m.bus.peek32(a5);
        }
        let c: Vec<String> = chain.iter().map(|r| format!("{r:06X}")).collect();
        println!(
            "{f} pc {:06X} ptr {:?} chiamanti {}",
            m.cpu.pc,
            m.game_pointer(),
            c.join(" < ")
        );
    }
    let mut v: Vec<_> = count.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    println!(
        "indirizzi di ritorno più frequenti: {:?}",
        v.iter()
            .take(12)
            .map(|(a, n)| format!("{a:06X}x{n}"))
            .collect::<Vec<_>>()
    );
}
