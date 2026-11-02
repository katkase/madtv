//! Estrae la grafica di Mad TV dai dischi originali, usando il decompressore
//! del gioco stesso ($042542) dentro l'emulatore: risultato identico a quello
//! che il gioco ha in memoria.
//!
//! Per ogni file prova la decompressione; lo considera grafica se il risultato
//! corrisponde all'intestazione (numero di immagini e dimensioni). Salva:
//! - `OUT/NNN/NN.png`: ogni immagine, con trasparenza dalla maschera;
//! - `OUT/NNN.png`: foglio con tutte le immagini del file;
//! - `OUT/index.md`: indice (file, immagini, dimensioni).
//! Uso: cargo run --release --example extract_assets -- [cartella] (default assets)
//!
//! La grafica appartiene ai titolari dei diritti: `assets/` è esclusa da git.
use madtv::adf::Adf;
use madtv::machine::Machine;
use madtv::os::vfs::Vfs;

const PACKED: u32 = 0x140000;
const OUT: u32 = 0x160000;
const RET: u32 = 0x13FFF0;
const UNPACK: u32 = 0x042542;

/// Chiama il decompressore come farebbe il gioco: unpack(dati, uscita, maschera).
fn unpack(base: &Machine, data: &[u8], mask: bool) -> Option<Machine> {
    let mut c = base.clone();
    c.bus.custom.paula.enabled = false;
    c.bus.write_bytes(PACKED, data);
    c.bus.write_bytes(OUT, &vec![0u8; 0x60000]);
    c.bus.poke16(RET, 0x4E71);
    let sp = c.cpu.a(7) - 64;
    c.bus.poke32(sp, RET);
    c.bus.poke32(sp + 4, PACKED);
    c.bus.poke32(sp + 8, OUT);
    c.bus.poke16(sp + 12, mask as u16);
    c.cpu.set_a(7, sp);
    c.cpu.pc = UNPACK;
    c.cpu.invalidate_prefetch();
    let limit = c.instructions + 30_000_000;
    while c.cpu.pc != RET {
        if c.instructions > limit || !matches!(c.run(c.instructions + 1, u64::MAX), madtv::machine::StopReason::InstructionLimit) {
            return None;
        }
    }
    Some(c)
}

struct Img {
    w: usize,
    h: usize,
    /// Indice di colore per pixel; None = trasparente (maschera a 0).
    px: Vec<Option<u8>>,
}

fn decode(c: &Machine, n: usize) -> Vec<Img> {
    (0..n as u32)
        .map(|i| {
            let e = OUT + 2 + i * 8;
            let (wb, h, p) = (c.bus.peek16(e) as usize, c.bus.peek16(e + 2) as usize, c.bus.peek32(e + 4));
            let mut px = Vec::with_capacity(wb * 8 * h);
            for y in 0..h {
                for x in 0..wb * 8 {
                    // Con la maschera: prima la maschera (wb × h byte), poi i 5
                    // bitplane interleaved (riga per riga, un piano dopo l'altro).
                    let at = |o: usize| c.bus.peek8(p + o as u32) >> (7 - x % 8) & 1;
                    let mask = at(y * wb + x / 8);
                    let col = (0..5).map(|pl| at(wb * h + (y * 5 + pl) * wb + x / 8) << pl).sum::<u8>();
                    px.push(if mask == 1 { Some(col) } else { None });
                }
            }
            Img { w: wb * 8, h, px }
        })
        .collect()
}

fn save_png(path: &std::path::Path, w: usize, h: usize, rgba: &[u8]) {
    let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut e = png::Encoder::new(f, w as u32, h as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(rgba).unwrap();
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or("assets".into()));
    std::fs::create_dir_all(&out).unwrap();
    let mut disks: Vec<Adf> = std::fs::read_dir("adf").unwrap().flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok()).collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    // Tavolozza globale del gioco: MTV1:001.HES, 32 colori a 12 bit.
    let pal_raw = disks[0].read_file("001.HES").unwrap();
    let pal: Vec<[u8; 3]> = (0..32)
        .map(|i| {
            let v = u16::from_be_bytes([pal_raw[i * 2], pal_raw[i * 2 + 1]]);
            [((v >> 8) & 15) as u8 * 17, ((v >> 4) & 15) as u8 * 17, (v & 15) as u8 * 17]
        })
        .collect();
    let mut files: Vec<(String, String, Vec<u8>)> = Vec::new();
    for d in &disks {
        for e in d.entries().filter(|e| !e.is_dir) {
            if e.name.to_ascii_uppercase().ends_with(".HES") {
                files.push((d.volume_name.clone(), e.name.clone(), d.read_file(&e.name).unwrap()));
            }
        }
    }
    files.sort_by(|a, b| a.1.to_ascii_uppercase().cmp(&b.1.to_ascii_uppercase()));
    let exe = disks[0].read_file("mtv").unwrap();
    let mut m = Machine::new(Vfs::new(disks, "/tmp/extractsave".into()));
    m.boot_os();
    m.load_executable(&exe, "s").unwrap();
    while m.bus.custom.frame < 450 {
        m.run_frame();
    }
    let mut index = String::from("| file | disk | bytes | images | size (pixels) |\n|---|---|---|---|---|\n");
    let mut total = 0;
    for (vol, name, data) in &files {
        let n = data[0] as usize;
        if n == 0 || data.len() < 1 + 2 * n + 6 {
            continue;
        }
        let dims: Vec<(usize, usize)> = (0..n).map(|i| (data[1 + 2 * i] as usize, data[2 + 2 * i] as usize)).collect();
        if dims.iter().any(|&(w, h)| w == 0 || h == 0) {
            continue;
        }
        let Some(c) = unpack(&m, data, true) else { continue };
        let ok = c.bus.peek16(OUT) as usize == n
            && dims.iter().enumerate().all(|(i, &(w, h))| {
                let e = OUT + 2 + i as u32 * 8;
                c.bus.peek16(e) as usize == w.div_ceil(8) && c.bus.peek16(e + 2) as usize == h
            });
        if !ok {
            continue;
        }
        let imgs = decode(&c, n);
        let stem = name.split('.').next().unwrap().to_string();
        let dir = out.join(&stem);
        std::fs::create_dir_all(&dir).unwrap();
        let rgba = |img: &Img, x: usize, y: usize| match img.px[y * img.w + x] {
            Some(ci) => [pal[ci as usize][0], pal[ci as usize][1], pal[ci as usize][2], 255],
            None => [0, 0, 0, 0],
        };
        for (i, img) in imgs.iter().enumerate() {
            let mut buf = Vec::with_capacity(img.w * img.h * 4);
            for y in 0..img.h {
                for x in 0..img.w {
                    buf.extend_from_slice(&rgba(img, x, y));
                }
            }
            save_png(&dir.join(format!("{i:02}.png")), img.w, img.h, &buf);
        }
        // Foglio: immagini affiancate su righe larghe al più 320 pixel, sfondo a scacchi.
        let (mut x, mut y, mut rowh, mut sw) = (0usize, 0usize, 0usize, 0usize);
        let mut place = Vec::new();
        for img in &imgs {
            if x > 0 && x + img.w > 320 {
                x = 0;
                y += rowh + 2;
                rowh = 0;
            }
            place.push((x, y));
            x += img.w + 2;
            sw = sw.max(x);
            rowh = rowh.max(img.h);
        }
        let sh = y + rowh;
        let mut sheet = vec![0u8; sw * sh * 4];
        for yy in 0..sh {
            for xx in 0..sw {
                let g = if (xx / 4 + yy / 4) % 2 == 0 { 0x30 } else { 0x48 };
                sheet[(yy * sw + xx) * 4..][..4].copy_from_slice(&[g, g, g, 255]);
            }
        }
        for (img, &(px, py)) in imgs.iter().zip(&place) {
            for yy in 0..img.h {
                for xx in 0..img.w {
                    let c = rgba(img, xx, yy);
                    if c[3] == 255 {
                        sheet[((py + yy) * sw + px + xx) * 4..][..4].copy_from_slice(&c);
                    }
                }
            }
        }
        save_png(&out.join(format!("{stem}.png")), sw, sh, &sheet);
        let mut ds: Vec<String> = Vec::new();
        for &(w, h) in &dims {
            let s = format!("{w}×{h}");
            if ds.last().map(|l: &String| l.ends_with(&format!(" {s}")) || *l == s).unwrap_or(false) {
                let last = ds.pop().unwrap();
                let (k, _) = last.split_once(" ").map(|(a, b)| (a.trim_end_matches('×').parse::<usize>().unwrap_or(1), b)).unwrap_or((1, ""));
                let k = if last.contains(' ') { k + 1 } else { 2 };
                ds.push(format!("{k}× {s}"));
            } else {
                ds.push(s);
            }
        }
        index += &format!("| [{name}]({stem}.png) | {vol} | {} | {n} | {} |\n", data.len(), ds.join(", "));
        total += 1;
        println!("{vol}:{name}: {n} immagini");
    }
    std::fs::write(out.join("index.md"), &index).unwrap();
    println!("{total} file di grafica estratti in {}", out.display());
}
