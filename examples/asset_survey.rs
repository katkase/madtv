//! Inventario dei file dei dischi: dimensione, primi byte, tipo riconosciuto.
//! Uso: cargo run --release --example asset_survey [cartella_estrazione]
use madtv::adf::Adf;

fn kind(d: &[u8]) -> &'static str {
    if d.starts_with(b"FORM") { "IFF" }
    else if d.starts_with(b"TFMX") { "musica TFMX" }
    else if d.starts_with(&[0x00, 0x00, 0x03, 0xF3]) { "eseguibile" }
    else if d.starts_with(&[0x01, 0xA0, 0x6D]) { "grafica compressa (01 A0 6D)" }
    else if d.iter().take(64).all(|&b| b == 10 || b == 13 || b == 9 || (32..127).contains(&b) || b >= 0xA0) { "testo" }
    else { "?" }
}

fn main() {
    let out = std::env::args().nth(1);
    let mut disks: Vec<Adf> = std::fs::read_dir("adf").unwrap().flatten()
        .filter_map(|e| Adf::from_file(&e.path()).ok()).collect();
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    let mut counts = std::collections::BTreeMap::new();
    for d in &disks {
        let mut names: Vec<String> = d.entries().filter(|e| !e.is_dir).map(|e| e.name.clone()).collect();
        names.sort();
        for name in names {
            let data = d.read_file(&name).unwrap_or_default();
            let k = kind(&data);
            *counts.entry(k).or_insert(0) += 1;
            let head: Vec<String> = data.iter().take(16).map(|b| format!("{b:02X}")).collect();
            println!("{}:{name:<14} {:>7}  {:<30} {}", d.volume_name, data.len(), k, head.join(" "));
            if let Some(o) = &out {
                let p = std::path::Path::new(o).join(&d.volume_name);
                std::fs::create_dir_all(&p).unwrap();
                let f = p.join(&name);
                std::fs::create_dir_all(f.parent().unwrap()).unwrap();
                std::fs::write(f, &data).unwrap();
            }
        }
    }
    println!("{counts:?}");
}
