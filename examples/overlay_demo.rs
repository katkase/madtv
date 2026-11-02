//! Compone offline un frame come lo mostrerebbe la finestra (scala 3, margine
//! 20) con lo strato diagnostico, e lo salva in PNG.
//! Uso: cargo run --release --example overlay_demo -- out.png
use madtv::adf::Adf;
use madtv::enhance::Enhancer;
use madtv::input;
use madtv::machine::Machine;
use madtv::os::vfs::Vfs;
use madtv::overlay::Canvas;
use madtv::render::WIDTH;

fn main() {
    let out = std::env::args().nth(1).unwrap_or("overlay.png".into());
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
    for _ in 0..40 {
        if let Some(g) = m.game_mouse() {
            let (dx, dy) = input::pointer_step(&g, (55, 101));
            input::mouse_move(&mut m.bus.custom, dx, dy);
        }
        m.run_frame();
    }
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
    let shapes = Enhancer { debug: true }.update(&m);
    Canvas {
        buf: &mut buf,
        width: w,
        height: h,
        scale: s as f32,
        origin: (mg * s, mg * s),
    }
    .draw(&shapes);
    let mut rgb = Vec::new();
    for p in &buf {
        rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    let f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut e = png::Encoder::new(f, w as u32, h as u32);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&rgb).unwrap();
    println!("{out}: {w}x{h}, puntatore {:?}", m.game_pointer());
}
