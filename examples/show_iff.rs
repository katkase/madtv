//! Decodifica un'immagine IFF dal disco 1 e la salva in PNG.
//! Uso: cargo run --release --example show_iff -- u3.lbm /tmp/u3.png
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let disk = std::fs::read_dir("adf")
        .unwrap()
        .flatten()
        .filter_map(|e| madtv::adf::Adf::from_file(&e.path()).ok())
        .find(|d| d.volume_name == "MTV1")
        .unwrap();
    let img = madtv::iff::decode(&disk.read_file(&a[1]).unwrap()).unwrap();
    println!("{}x{}", img.width, img.height);
    let mut px = img.pixels.clone();
    px.truncate(320 * img.height.min(256));
    madtv::render::save_png(&px, std::path::Path::new(&a[2])).unwrap();
}
