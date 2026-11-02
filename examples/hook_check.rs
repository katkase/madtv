//! Verifica degli agganci: a ogni chiamata reale di una routine agganciata,
//! esegue l'originale 68000 su una copia della macchina e la versione Rust su
//! un'altra, poi confronta tutta la RAM e i registri.
//! Uso: cargo run --release --example hook_check -- rec.txt DA A [max_confronti]
use madtv::adf::Adf;
use madtv::hooks;
use madtv::input;
use madtv::machine::Machine;
use madtv::os::vfs::Vfs;
#[path = "drag_lab.rs"]
#[allow(dead_code)]
mod lab;

fn regs(m: &Machine) -> Vec<u32> {
    (0..8)
        .map(|i| m.cpu.d(i))
        .chain((0..8).map(|i| m.cpu.a(i)))
        .chain([m.cpu.pc])
        .collect()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let script =
        input::expand(input::parse_script(&std::fs::read_to_string(&a[1]).unwrap()).unwrap());
    let from: u64 = a[2].parse().unwrap();
    let to: u64 = a[3].parse().unwrap();
    let max: usize = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(20);
    let mut disks: Vec<Adf> = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok())
        .collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "/tmp/hooksave".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    let mut next = 0;
    while m.bus.custom.frame < from {
        lab::apply(&mut m, &script, &mut next);
        m.run_frame();
    }
    let (mut done, mut skipped) = (0, 0);
    while m.bus.custom.frame < to && done < max {
        lab::apply(&mut m, &script, &mut next);
        let frame_end = m.bus.custom.frame + 1;
        while m.bus.custom.frame < frame_end && done < max {
            let pc = m.cpu.pc;
            if let Some(h) = m.hooks.iter().find(|h| h.addr == pc).copied() {
                if hooks::signature_ok(&m.bus, &h) {
                    let mut rust = m.clone();
                    if (h.f)(&mut rust.cpu, &mut rust.bus).is_some() {
                        let sp = rust.cpu.a(7);
                        rust.cpu.pc = rust.bus.peek32(sp);
                        rust.cpu.set_a(7, sp + 4);
                        // Originale: esegue finché la routine non torna al chiamante.
                        let mut orig = m.clone();
                        let (ret, sp0) = (orig.bus.peek32(orig.cpu.a(7)), orig.cpu.a(7));
                        let mut n = 0u64;
                        while !(orig.cpu.pc == ret && orig.cpu.a(7) == sp0 + 4) {
                            orig.run(orig.instructions + 1, u64::MAX);
                            n += 1;
                        }
                        // Gli interrupt serviti durante l'originale cambiano solo
                        // contatori e registri custom: confrontiamo la RAM del gioco
                        // escludendo il contatore dei frame e gli accumulatori del mouse.
                        // Fuori da vettori e pila del supervisore (quadri degli
                        // interrupt presi durante l'originale) se SOLO_GIOCO è impostata.
                        let skip_low = std::env::var("SOLO_GIOCO").is_ok();
                        let diff: Vec<usize> = (0..0x100000usize)
                            .filter(|&i| !(skip_low && i < 0x2000))
                            .filter(|&i| orig.bus.ram[i] != rust.bus.ram[i])
                            .collect();
                        let rd: Vec<usize> = (0..17)
                            .filter(|&i| regs(&orig)[i] != regs(&rust)[i])
                            .collect();
                        println!(
                            "frame {} {}: originale {n} istruzioni, {} byte di RAM diversi {:X?} (a7 = {:06X}), registri diversi {rd:?}",
                            m.bus.custom.frame,
                            h.name,
                            diff.len(),
                            &diff[..diff.len().min(8)],
                            m.cpu.a(7)
                        );
                        done += 1;
                    } else {
                        skipped += 1;
                    }
                }
            }
            m.run(m.instructions + 1, u64::MAX);
        }
    }
    println!("{done} confronti, {skipped} chiamate lasciate all'originale (percorso del blitter)");
}
