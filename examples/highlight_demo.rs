//! Rigioca una registrazione passando ogni frame alle evidenziazioni (come la
//! finestra) e salva in PNG, alla risoluzione della finestra (scala 3, margine
//! 20), i frame richiesti.
//! Uso: cargo run --release --example highlight_demo -- rec/palinsesto.txt cartella 2950 3000 ...
use madtv::adf::Adf;
use madtv::enhance::Enhancer;
use madtv::input;
use madtv::machine::Machine;
use madtv::os::vfs::Vfs;
use madtv::overlay::Canvas;
use madtv::render::WIDTH;
#[path = "drag_lab.rs"]
#[allow(dead_code)]
mod lab;

fn save(m: &Machine, shapes: &[madtv::overlay::Shape], path: &str) {
    let (s, mg, rows) = (3usize, 20usize, 220usize);
    let (w, h) = ((WIDTH + 2 * mg) * s, (mg + rows) * s);
    let mut buf = vec![0u32; w * h];
    let src = &m.bus.custom.display.front;
    for y in 0..rows {
        for x in 0..WIDTH {
            let p = src[y * WIDTH + x];
            for dy in 0..s {
                for dx in 0..s {
                    buf[((mg + y) * s + dy) * w + (mg + x) * s + dx] = p;
                }
            }
        }
    }
    Canvas {
        buf: &mut buf,
        width: w,
        height: h,
        scale: s as f32,
        origin: (mg * s, mg * s),
    }
    .draw(shapes);
    let mut rgb = Vec::with_capacity(w * h * 3);
    for p in &buf {
        rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut e = png::Encoder::new(f, w as u32, h as u32);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&rgb).unwrap();
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let script =
        input::expand(input::parse_script(&std::fs::read_to_string(&a[1]).unwrap()).unwrap());
    let dir = &a[2];
    std::fs::create_dir_all(dir).unwrap();
    let frames: Vec<u64> = a[3..].iter().map(|s| s.parse().unwrap()).collect();
    let last = *frames.iter().max().unwrap();
    let mut disks: Vec<Adf> = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok())
        .collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "/tmp/hlsave".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    let mut en = Enhancer::new(true);
    let mut next = 0;
    let t = std::time::Instant::now();
    while m.bus.custom.frame <= last {
        lab::apply(&mut m, &script, &mut next);
        m.run_frame();
        // Vicino ai frame richiesti va a velocità reale (50 fps), come la
        // finestra: i click di prova lunghi girano in un thread.
        if frames
            .iter()
            .any(|&t| t >= m.bus.custom.frame && t < m.bus.custom.frame + 60)
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let shapes = en.update(&m);
        let f = m.bus.custom.frame;
        if frames.contains(&f) {
            let p = format!("{dir}/{f:05}.png");
            save(&m, &shapes, &p);
            println!("{p}: {:?}", madtv::enhance::drag_state(&m));
        }
    }
    println!(
        "{} click di prova in {:?} (media {:?}), {} lunghi, tempo totale {:?}",
        en.probes,
        en.probe_time,
        en.probe_time
            .checked_div(en.probes.max(1) as u32)
            .unwrap_or_default(),
        en.landing_probes,
        t.elapsed()
    );
}
