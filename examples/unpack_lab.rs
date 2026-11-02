//! Chiama il decompressore del gioco ($042542) su un file dei dischi e stampa
//! l'intestazione del risultato. Uso: cargo run --release --example unpack_lab -- 006.HES [flag]
use madtv::adf::Adf;
use madtv::machine::Machine;
use madtv::os::vfs::Vfs;

const PACKED: u32 = 0x140000;
const OUT: u32 = 0x160000;
const RET: u32 = 0x13FFF0;

pub fn unpack(base: &Machine, data: &[u8], flag: u16) -> Option<Machine> {
    let mut c = base.clone();
    c.bus.custom.paula.enabled = false;
    c.bus.write_bytes(PACKED, data);
    c.bus.write_bytes(OUT, &vec![0u8; 0x40000]);
    c.bus.poke16(RET, 0x4E71); // nop
    let sp = c.cpu.a(7) - 64;
    c.bus.poke32(sp, RET);
    c.bus.poke32(sp + 4, PACKED);
    c.bus.poke32(sp + 8, OUT);
    c.bus.poke16(sp + 12, flag);
    c.cpu.set_a(7, sp);
    c.cpu.pc = 0x042542;
    c.cpu.invalidate_prefetch();
    let limit = c.instructions + 50_000_000;
    while c.cpu.pc != RET {
        if c.instructions > limit {
            return None;
        }
        c.run(c.instructions + 1, u64::MAX);
    }
    Some(c)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let flag: u16 = a.get(2).map(|s| s.parse().unwrap()).unwrap_or(0);
    let mut disks: Vec<Adf> = std::fs::read_dir("adf").unwrap().flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok()).collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let data = disks.iter().find_map(|d| d.read_file(&a[1]).ok()).expect("file");
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "/tmp/unpacksave".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    while m.bus.custom.frame < 450 { m.run_frame(); }
    let t = std::time::Instant::now();
    let c = unpack(&m, &data, flag).expect("decompressione non terminata");
    println!("decompresso in {:?}, {} istruzioni", t.elapsed(), c.instructions - m.instructions);
    let n = c.bus.peek16(OUT);
    println!("conteggio {n}");
    for i in 0..(n as u32).min(8) {
        let e = OUT + 2 + i * 8;
        println!("  voce {i}: {:04X} {:04X} {:08X}", c.bus.peek16(e), c.bus.peek16(e + 2), c.bus.peek32(e + 4));
    }
    std::fs::write(format!("/tmp/{}.raw", a[1]), c.bus.read_bytes(OUT, 0x10000)).unwrap();
}
