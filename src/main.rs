//! Programma di avvio: `madtv [opzioni]`.
//!
//! Esegui `madtv --help` per l'elenco delle opzioni.

use madtv::adf::Adf;
use madtv::machine::{Machine, StopReason};
use madtv::os::vfs::Vfs;
use madtv::render;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const HELP: &str = "\
Mad TV - porting Rust della versione Amiga (fase 1: avvio e prima schermata)

USO: madtv [opzioni]

  --adf-dir DIR      cartella con le immagini ADF originali (default: adf)
                     i dischi vengono riconosciuti dal nome di volume (MTV1, MTV2)
  --save-dir DIR     cartella per i salvataggi del gioco (default: save)
  --frames N         ferma l'emulazione dopo N frame PAL (default: 500)
  --max-instr N      ferma dopo N istruzioni 68000 (default: 400000000)
  --trace FILE       scrive ogni chiamata di sistema in FILE
  --shot-every N     salva uno screenshot PNG ogni N frame (0 = solo alla fine)
  --shot-dir DIR     cartella degli screenshot (default: shots)
  --dump FILE        salva la RAM emulata (2 MB) in FILE all'arresto
  --list             elenca i file dei dischi ed esce
  --help             questo messaggio
";

struct Opts {
    adf_dir: PathBuf,
    save_dir: PathBuf,
    frames: u64,
    max_instr: u64,
    trace: Option<PathBuf>,
    shot_every: u64,
    shot_dir: PathBuf,
    list: bool,
    dump: Option<PathBuf>,
}

fn parse_args() -> Result<Opts, String> {
    let mut o = Opts {
        adf_dir: "adf".into(),
        save_dir: "save".into(),
        frames: 500,
        max_instr: 400_000_000,
        trace: None,
        shot_every: 0,
        shot_dir: "shots".into(),
        list: false,
        dump: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("manca il valore per {a}"));
        match a.as_str() {
            "--adf-dir" => o.adf_dir = val()?.into(),
            "--save-dir" => o.save_dir = val()?.into(),
            "--frames" => o.frames = val()?.parse().map_err(|e| format!("{e}"))?,
            "--max-instr" => o.max_instr = val()?.parse().map_err(|e| format!("{e}"))?,
            "--trace" => o.trace = Some(val()?.into()),
            "--shot-every" => o.shot_every = val()?.parse().map_err(|e| format!("{e}"))?,
            "--shot-dir" => o.shot_dir = val()?.into(),
            "--list" => o.list = true,
            "--dump" => o.dump = Some(val()?.into()),
            "--help" | "-h" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            _ => return Err(format!("opzione sconosciuta: {a}")),
        }
    }
    Ok(o)
}

/// Carica tutti gli ADF della cartella e li ordina per nome di volume.
fn load_disks(dir: &Path) -> Result<Vec<Adf>, String> {
    let rd = std::fs::read_dir(dir)
        .map_err(|e| format!("impossibile leggere {}: {e}", dir.display()))?;
    let mut disks: Vec<Adf> = Vec::new();
    for de in rd.flatten() {
        let p = de.path();
        if p.extension()
            .map(|e| e.eq_ignore_ascii_case("adf"))
            .unwrap_or(false)
        {
            match Adf::from_file(&p) {
                Ok(a) => {
                    if disks.iter().any(|d| d.volume_name == a.volume_name) {
                        eprintln!(
                            "nota: {} ignorato (volume {} già presente)",
                            p.display(),
                            a.volume_name
                        );
                    } else {
                        disks.push(a);
                    }
                }
                Err(e) => eprintln!("nota: {} ignorato: {e}", p.display()),
            }
        }
    }
    disks.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    if disks.first().map(|d| d.volume_name.as_str()) != Some("MTV1") {
        return Err(format!(
            "non trovo il disco 1 di Mad TV (volume MTV1) in {}.\nCopia lì le immagini ADF originali.",
            dir.display()
        ));
    }
    Ok(disks)
}

fn main() {
    let o = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("errore: {e}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    let disks = match load_disks(&o.adf_dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("errore: {e}");
            std::process::exit(1);
        }
    };
    for d in &disks {
        println!("disco: {} ({} file)", d.volume_name, d.entries().count());
    }
    if o.list {
        for d in &disks {
            for e in d.entries() {
                println!(
                    "  {}:{}{}  {}",
                    d.volume_name,
                    e.name,
                    if e.is_dir { "/" } else { "" },
                    e.size
                );
            }
        }
        return;
    }

    let exe = disks[0]
        .read_file("mtv")
        .expect("eseguibile 'mtv' non trovato sul disco 1");
    let mut m = Machine::new(Vfs::new(disks, o.save_dir.clone()));
    if let Some(t) = &o.trace {
        let f = std::fs::File::create(t).expect("impossibile creare il file di trace");
        m.os.trace = Some(std::io::BufWriter::new(f));
    }
    m.boot_os();
    // La startup-sequence originale lancia "mtv s".
    m.load_executable(&exe, "s").expect("eseguibile non valido");
    println!("eseguibile caricato: punto di ingresso ${:06x}", m.entry);
    for (i, (a, size, kind)) in m.segments.iter().enumerate() {
        println!("  segmento {i}: {kind:4} ${a:06x} {size} byte");
    }

    std::fs::create_dir_all(&o.shot_dir).ok();
    let shots = Rc::new(RefCell::new(0u32));
    if o.shot_every > 0 {
        let dir = o.shot_dir.clone();
        let every = o.shot_every;
        let shots = shots.clone();
        m.frame_hook = Some(Box::new(move |bus, frame| {
            if frame % every == 0 {
                let f = render::render(bus);
                let p = dir.join(format!("frame{frame:05}.png"));
                if render::save_png(&f, &p).is_ok() {
                    *shots.borrow_mut() += 1;
                }
            }
        }));
    }

    let t0 = std::time::Instant::now();
    let reason = m.run(o.max_instr, o.frames);
    let dt = t0.elapsed();
    if let Some(t) = m.os.trace.as_mut() {
        use std::io::Write;
        let _ = t.flush();
    }

    println!();
    println!("=== arresto: {}", describe(&reason));
    println!(
        "istruzioni: {}  frame: {}  tempo: {:.2}s  PC=${:06x}",
        m.instructions,
        m.bus.custom.frame,
        dt.as_secs_f64(),
        m.cpu.pc
    );
    if matches!(
        reason,
        StopReason::Exception(_) | StopReason::Unimplemented(_)
    ) {
        let pcs: Vec<String> = m
            .recent_pcs()
            .iter()
            .rev()
            .take(24)
            .map(|p| format!("{p:06x}"))
            .collect();
        println!("ultimi PC (dal più recente): {}", pcs.join(" "));
        let r: Vec<String> = (0..8).map(|i| format!("d{i}={:08x}", m.cpu.d(i))).collect();
        println!("  {}", r.join(" "));
        let r: Vec<String> = (0..8).map(|i| format!("a{i}={:08x}", m.cpu.a(i))).collect();
        println!("  {}", r.join(" "));
    }
    report(&m);
    if let Some(d) = &o.dump {
        std::fs::write(d, &m.bus.ram).expect("impossibile scrivere il dump");
        println!("RAM salvata in {}", d.display());
    }
    let f = render::render(&m.bus);
    let p = o.shot_dir.join("final.png");
    match render::save_png(&f, &p) {
        Ok(()) => println!(
            "schermata finale: {} ({}x{}, {} bitplane{}, {} righe attive, {} MOVE copper)",
            p.display(),
            f.width,
            f.height,
            f.planes,
            if f.ham { ", HAM" } else { "" },
            f.active_lines,
            f.copper_moves
        ),
        Err(e) => println!("impossibile salvare lo screenshot: {e}"),
    }
    if *shots.borrow() > 0 {
        println!(
            "screenshot intermedi: {} in {}",
            shots.borrow(),
            o.shot_dir.display()
        );
    }
}

fn describe(r: &StopReason) -> String {
    match r {
        StopReason::Exited(c) => format!("il programma è terminato (codice {c})"),
        StopReason::Unimplemented(s) => format!("funzione non implementata: {s}"),
        StopReason::Exception(s) => s.clone(),
        StopReason::InstructionLimit => "raggiunto il limite di istruzioni".into(),
        StopReason::FrameLimit => "raggiunto il limite di frame".into(),
        StopReason::CpuHalted => "CPU bloccata (doppio errore di bus)".into(),
    }
}

fn report(m: &Machine) {
    println!("--- chiamate di sistema:");
    let mut calls: Vec<_> = m.os.call_counts.iter().collect();
    calls.sort_by(|a, b| b.1.cmp(a.1));
    for ((lib, idx), n) in calls.iter().take(40) {
        println!(
            "  {:>8}  {}.{}",
            n,
            lib.lib_name().trim_end_matches(".library"),
            lib.func_name(*idx as usize)
        );
    }
    println!("--- registri dei chip custom scritti dal programma:");
    let c = &m.bus.custom;
    let mut regs: Vec<(usize, u64)> = c
        .write_count
        .iter()
        .enumerate()
        .filter(|(_, n)| **n > 0)
        .map(|(i, n)| (i, *n))
        .collect();
    regs.sort_by(|a, b| b.1.cmp(&a.1));
    let line: Vec<String> = regs
        .iter()
        .take(30)
        .map(|(i, n)| format!("{}={}", madtv::custom::reg_name((*i * 2) as u16), n))
        .collect();
    println!("  {}", line.join(" "));
    println!(
        "  blittate: {} (di cui in modalità linea, non ancora emulate: {})  COP1LC=${:06x}  DMACON=${:04x}  INTENA=${:04x}",
        c.blits, c.line_blits, c.cop1lc, c.dmacon, c.intena
    );
    if m.bus.unmapped_reads + m.bus.unmapped_writes > 0 {
        let first: Vec<String> = m
            .bus
            .unmapped_log
            .iter()
            .take(8)
            .map(|(a, w)| format!("{}${a:06x}", if *w { "W" } else { "R" }))
            .collect();
        println!(
            "  accessi a indirizzi non mappati: {} letture, {} scritture (primi: {})",
            m.bus.unmapped_reads,
            m.bus.unmapped_writes,
            first.join(" ")
        );
    }
    if !m.os.console.is_empty() {
        println!(
            "--- output su console:\n{}",
            String::from_utf8_lossy(&m.os.console)
        );
    }
}
