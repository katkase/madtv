//! Programma di avvio: `madtv [opzioni]`.
//!
//! Esegui `madtv --help` per l'elenco delle opzioni.

use madtv::adf::Adf;
use madtv::input::{self, Action};
use madtv::machine::{Machine, StopReason};
use madtv::os::vfs::Vfs;
use madtv::render;
use madtv::window::{self, WindowOpts};
use std::path::{Path, PathBuf};

const HELP: &str = "\
Mad TV - porting Rust della versione Amiga

USO: madtv [opzioni]

Di default apre la finestra di gioco (F10 = turbo automatico sì/no,
F11 tenuto premuto = avanti veloce, F12 = screenshot).

  --adf-dir DIR      cartella con le immagini ADF originali (default: adf)
                     i dischi vengono riconosciuti dal nome di volume (MTV1, MTV2)
  --save-dir DIR     cartella per i salvataggi del gioco (default: save)
  --scale N          ingrandimento della finestra (default: 3)
  --full-height      mostra tutte le 256 righe PAL (default: le 200 usate dal gioco)
  --mouse-ratio R    conteggi del mouse Amiga per pixel (default: 1.0)
  --cpu-speed P      velocità della CPU in % di un A500, oppure auto
                     (default: auto con la finestra, 100 in headless).
                     auto = velocità originale, con turbo durante caricamenti e
                     decompressioni dal disco.
  --headless         nessuna finestra: esegue --frames frame e salva screenshot
  --frames N         (headless) frame PAL da eseguire, 50 = 1 secondo (default: 500)
  --max-instr N      (headless) limite di istruzioni 68000
  --shot-every N     (headless) uno screenshot PNG ogni N frame (0 = solo alla fine)
  --shot-dir DIR     cartella degli screenshot (default: shots)
  --script TESTO     azioni automatiche: \"FRAME:azione; FRAME:azione...\"
                     (oppure @file per leggerle da un file). Azioni:
                       move DX DY   sposta il mouse (conteggi Amiga)
                       goto X Y     porta il puntatore del gioco in (X, Y)
                       click | down | up | rdown | rup   tasti del mouse
                       key NOME | keydown NOME | keyup NOME   tasti (A, ENTER, F1...)
                       type TESTO   scrive un testo
                       shot NOME    salva shots/NOME.png
  --trace FILE       scrive ogni chiamata di sistema in FILE
  --dump FILE        salva la RAM emulata (2 MB) in FILE all'arresto
  --list             elenca i file dei dischi ed esce
  --help             questo messaggio
";

struct Opts {
    adf_dir: PathBuf,
    save_dir: PathBuf,
    scale: usize,
    full_height: bool,
    mouse_ratio: f32,
    cpu_speed: Option<String>,
    headless: bool,
    frames: u64,
    max_instr: u64,
    trace: Option<PathBuf>,
    shot_every: u64,
    shot_dir: PathBuf,
    script: Vec<(u64, Action)>,
    list: bool,
    dump: Option<PathBuf>,
}

fn parse_args() -> Result<Opts, String> {
    let mut o = Opts {
        adf_dir: "adf".into(),
        save_dir: "save".into(),
        scale: 3,
        full_height: false,
        mouse_ratio: 1.0,
        cpu_speed: None,
        headless: false,
        frames: 500,
        max_instr: u64::MAX,
        trace: None,
        shot_every: 0,
        shot_dir: "shots".into(),
        script: Vec::new(),
        list: false,
        dump: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("manca il valore per {a}"));
        match a.as_str() {
            "--adf-dir" => o.adf_dir = val()?.into(),
            "--save-dir" => o.save_dir = val()?.into(),
            "--scale" => o.scale = val()?.parse().map_err(|e| format!("{e}"))?,
            "--mouse-ratio" => o.mouse_ratio = val()?.parse().map_err(|e| format!("{e}"))?,
            "--headless" => o.headless = true,
            "--full-height" => o.full_height = true,
            "--cpu-speed" => o.cpu_speed = Some(val()?),
            "--frames" => o.frames = val()?.parse().map_err(|e| format!("{e}"))?,
            "--max-instr" => o.max_instr = val()?.parse().map_err(|e| format!("{e}"))?,
            "--trace" => o.trace = Some(val()?.into()),
            "--shot-every" => o.shot_every = val()?.parse().map_err(|e| format!("{e}"))?,
            "--shot-dir" => o.shot_dir = val()?.into(),
            "--script" => {
                let v = val()?;
                let src = match v.strip_prefix('@') {
                    Some(f) => std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?,
                    None => v,
                };
                o.script = input::expand(input::parse_script(&src)?);
            }
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
    let speed = o.cpu_speed.clone().unwrap_or_else(|| {
        if o.headless {
            "100".into()
        } else {
            "auto".into()
        }
    });
    if speed.eq_ignore_ascii_case("auto") {
        m.cpu_speed = 100;
        m.auto_turbo = true;
    } else {
        match speed.parse::<u32>() {
            Ok(v) => m.cpu_speed = v.clamp(25, 2000),
            Err(_) => {
                eprintln!("errore: --cpu-speed vuole un numero o auto");
                std::process::exit(2);
            }
        }
    }
    if o.full_height {
        m.bus.custom.display.visible_rows = render::HEIGHT;
    }
    m.boot_os();
    // La startup-sequence originale lancia "mtv s".
    m.load_executable(&exe, "s").expect("eseguibile non valido");

    let t0 = std::time::Instant::now();
    let reason = if o.headless {
        run_headless(&mut m, &o)
    } else {
        // Lo script, se c'è, viene applicato prima di aprire la finestra
        // fino all'ultimo frame indicato (utile per saltare il caricamento).
        let pre = run_script_until(&mut m, &o, o.script.last().map(|x| x.0).unwrap_or(0));
        match pre {
            Some(r) => r,
            None => {
                let wo = WindowOpts {
                    scale: o.scale,
                    shot_dir: o.shot_dir.clone(),
                    mouse_ratio: o.mouse_ratio,
                };
                window::run(&mut m, &wo).unwrap_or(StopReason::FrameLimit)
            }
        }
    };
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
    if m.auto_turbo {
        println!(
            "turbo automatico: {} frame in turbo, {} chiamate al contatore dei frame",
            m.turbo_frames, m.pacing_calls
        );
    }
    report(&m);
    if let Some(d) = &o.dump {
        std::fs::write(d, &m.bus.ram).expect("impossibile scrivere il dump");
        println!("RAM salvata in {}", d.display());
    }
    if o.headless {
        std::fs::create_dir_all(&o.shot_dir).ok();
        let p = o.shot_dir.join("final.png");
        let d = &m.bus.custom.display;
        match d.save_png(&p) {
            Ok(()) => println!(
                "schermata finale: {} ({} bitplane, {} righe attive; righe usate nella sessione: {:?})",
                p.display(),
                d.last_planes,
                d.last_active_lines,
                d.used_rows
            ),
            Err(e) => println!("impossibile salvare lo screenshot: {e}"),
        }
    }
}

/// Avvicina il puntatore del gioco al bersaglio (al massimo 100 conteggi per frame).
fn step_goto(m: &mut Machine, target: &mut Option<(i32, i32)>) {
    let Some(t) = *target else { return };
    let Some(g) = m.game_mouse() else { return };
    let (dx, dy) = input::pointer_step(&g, t);
    if dx == 0 && dy == 0 && g.pending == (0, 0) {
        *target = None; // arrivato (o bloccato dai limiti della schermata)
    } else if dx != 0 || dy != 0 {
        input::mouse_move(&mut m.bus.custom, dx, dy);
    }
}

/// Applica le azioni dello script previste per il frame corrente.
fn apply_actions(m: &mut Machine, o: &Opts, next: &mut usize, target: &mut Option<(i32, i32)>) {
    step_goto(m, target);
    let frame = m.bus.custom.frame;
    while *next < o.script.len() && o.script[*next].0 <= frame {
        let c = &mut m.bus.custom;
        match &o.script[*next].1 {
            Action::Move(dx, dy) => input::mouse_move(c, *dx, *dy),
            Action::Goto(x, y) => *target = Some((*x, *y)),
            Action::LeftDown => c.left_button = true,
            Action::LeftUp => c.left_button = false,
            Action::RightDown => c.right_button = true,
            Action::RightUp => c.right_button = false,
            Action::KeyDown(k) => input::key(c, *k, true),
            Action::KeyUp(k) => input::key(c, *k, false),
            Action::Shot(name) => {
                std::fs::create_dir_all(&o.shot_dir).ok();
                let p = o.shot_dir.join(format!("{name}.png"));
                let _ = c.display.save_png(&p);
                println!("[frame {frame}] screenshot {}", p.display());
            }
            Action::Click | Action::Key(_) | Action::Type(_) => {} // già espanse
        }
        *next += 1;
    }
}

fn run_script_until(m: &mut Machine, o: &Opts, last: u64) -> Option<StopReason> {
    let mut next = 0usize;
    let mut target = None;
    while m.bus.custom.frame <= last && next < o.script.len() {
        apply_actions(m, o, &mut next, &mut target);
        if let Some(r) = m.run_frame() {
            return Some(r);
        }
    }
    None
}

fn run_headless(m: &mut Machine, o: &Opts) -> StopReason {
    let mut next = 0usize;
    let mut target = None;
    loop {
        let frame = m.bus.custom.frame;
        if frame >= o.frames {
            return StopReason::FrameLimit;
        }
        if m.instructions >= o.max_instr {
            return StopReason::InstructionLimit;
        }
        apply_actions(m, o, &mut next, &mut target);
        if let Some(r) = m.run_frame() {
            return r;
        }
        let f = m.bus.custom.frame;
        if o.shot_every > 0 && f.is_multiple_of(o.shot_every) {
            std::fs::create_dir_all(&o.shot_dir).ok();
            let _ = m
                .bus
                .custom
                .display
                .save_png(&o.shot_dir.join(format!("frame{f:05}.png")));
        }
    }
}

fn describe(r: &StopReason) -> String {
    match r {
        StopReason::Exited(c) => format!("il programma è terminato (codice {c})"),
        StopReason::Unimplemented(s) => format!("funzione non implementata: {s}"),
        StopReason::Exception(s) => s.clone(),
        StopReason::InstructionLimit => "raggiunto il limite di istruzioni".into(),
        StopReason::FrameLimit => "fine (limite di frame o finestra chiusa)".into(),
        StopReason::CpuHalted => "CPU bloccata (doppio errore di bus)".into(),
    }
}

fn report(m: &Machine) {
    println!("--- chiamate di sistema:");
    let mut calls: Vec<_> = m.os.call_counts.iter().collect();
    calls.sort_by_key(|x| std::cmp::Reverse(*x.1));
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
    regs.sort_by_key(|x| std::cmp::Reverse(x.1));
    let line: Vec<String> = regs
        .iter()
        .take(30)
        .map(|(i, n)| format!("{}={}", madtv::custom::reg_name((*i * 2) as u16), n))
        .collect();
    println!("  {}", line.join(" "));
    println!(
        "  blittate: {} (in modalità linea, non ancora emulate: {})  COP1LC=${:06x}  DMACON=${:04x}  INTENA=${:04x}",
        c.blits, c.line_blits, c.cop1lc, c.dmacon, c.intena
    );
    if std::env::var("MADTV_ALLREGS").is_ok() {
        let all: Vec<String> = regs
            .iter()
            .map(|(i, n)| format!("{}={}", madtv::custom::reg_name((*i * 2) as u16), n))
            .collect();
        println!("  tutti: {}", all.join(" "));
    }
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
