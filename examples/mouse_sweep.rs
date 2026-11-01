//! Prova di scorrimento lento del puntatore sopra l'edificio (zone con dialog
//! al passaggio del mouse). Conta i "salti": movimenti contrari alla direzione
//! o scatti troppo ampi. Uso: cargo run --release --example mouse_sweep
use madtv::adf::Adf;
use madtv::input;
use madtv::machine::Machine;
use madtv::os::vfs::Vfs;

fn setup(speed: u32) -> Machine {
    let mut disks: Vec<Adf> = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok())
        .collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "save".into()));
    m.cpu_speed = speed;
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    m
}

/// Avanza `n` frame con un bersaglio fisso usando la logica nuova.
fn go(m: &mut Machine, t: (i32, i32), n: u32) {
    for _ in 0..n {
        if let Some(g) = m.game_mouse() {
            let (dx, dy) = input::pointer_step(&g, t);
            input::mouse_move(&mut m.bus.custom, dx, dy);
        }
        m.run_frame();
    }
}

fn click(m: &mut Machine) {
    m.bus.custom.left_button = true;
    m.run_frame();
    m.run_frame();
    m.run_frame();
    m.bus.custom.left_button = false;
}

fn main() {
    for (label, naive) in [
        ("vecchia logica (ripete dopo 3 frame fermi)", true),
        ("nuova logica (accumulatori del gioco)", false),
    ] {
        let cpu: u32 = std::env::var("CPU")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);
        let mut m = setup(cpu);
        while m.bus.custom.frame < 500 {
            m.run_frame();
        }
        go(&mut m, (55, 101), 20);
        click(&mut m);
        // dialoghi del capo: qualche click per arrivare all'edificio
        let clicks: usize = std::env::var("CLICKS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(6);
        for i in 0..clicks {
            go(&mut m, (150, 60), 120);
            if !naive {
                let p = format!("/tmp/seq{i}.png");
                m.bus
                    .custom
                    .display
                    .save_png(std::path::Path::new(&p))
                    .unwrap();
            }
            click(&mut m);
        }
        go(&mut m, (10, 40), 60);
        if !naive {
            m.bus
                .custom
                .display
                .save_png(std::path::Path::new("/tmp/sweep_start.png"))
                .unwrap();
        }
        let (mut jumps, mut maxover) = (0, 0);
        let (mut busy, mut busy_run) = (0, 0);
        let mut last = m.game_pointer().unwrap();
        let mut pending = (0i32, 0i32);
        let mut still = 0;
        let mut lastp = last;
        let speed: i32 = std::env::var("SPEED")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);
        let half = 290 / speed;
        for f in 0..(half * 8) {
            // scorrimento avanti e indietro tra x=10 e x=300, su righe diverse
            let phase = f % (half * 2);
            let x = if phase < half {
                10 + phase * speed
            } else {
                300 - (phase - half) * speed
            };
            let y = 40 + ((f / (half * 2)) % 3) * 30;
            let g = m.game_mouse().unwrap();
            if g.pending != (0, 0) {
                busy_run += 1;
                if busy_run == 3 {
                    busy += 1;
                }
            } else {
                busy_run = 0;
            }
            if !naive && (f == 60 || f == 130 || f == 400) {
                let p = format!("/tmp/sweep{f}.png");
                m.bus
                    .custom
                    .display
                    .save_png(std::path::Path::new(&p))
                    .unwrap();
            }
            let d = if naive {
                let moved = (g.pos.0 - lastp.0, g.pos.1 - lastp.1);
                pending.0 -= moved.0;
                pending.1 -= moved.1;
                if moved == (0, 0) {
                    still += 1;
                    if still >= 3 {
                        pending = (0, 0);
                    }
                } else {
                    still = 0;
                }
                lastp = g.pos;
                let d = (
                    (x - g.pos.0 - pending.0).clamp(-100, 100),
                    (y - g.pos.1 - pending.1).clamp(-100, 100),
                );
                pending.0 += d.0;
                pending.1 += d.1;
                d
            } else {
                input::pointer_step(&g, (x, y))
            };
            input::mouse_move(&mut m.bus.custom, d.0, d.1);
            m.run_frame();
            let p = m.game_pointer().unwrap();
            let dir = if phase < half { 1 } else { -1 };
            let dx = p.0 - last.0;
            // Subito dopo un'inversione il puntatore, in ritardo, prosegue ancora:
            // non è un salto.
            let near_turn = phase % half < 12;
            if !near_turn && (dx * dir < -2 || dx.abs() > speed * 4 + 4) {
                jumps += 1;
            }
            maxover = maxover.max((p.0 - x) * dir);
            last = p;
        }
        println!(
            "{label}: salti {jumps}, massimo anticipo sul bersaglio {maxover} px, episodi di ciclo occupato {busy}"
        );
    }
}
