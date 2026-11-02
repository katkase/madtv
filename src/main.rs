//! Programma di avvio: `madtv [opzioni]`.
//!
//! Esegui `madtv --help` per l'elenco delle opzioni.

use madtv::adf::Adf;
use madtv::input::{self, Action};
use madtv::machine::{Machine, StopReason};
use madtv::os::vfs::Vfs;
use madtv::render;
use madtv::window::{Frontend, RunEnd, WindowOpts};
use std::path::{Path, PathBuf};

const HELP: &str = "\
Mad TV - porting Rust della versione Amiga

USO: madtv [opzioni]

Di default apre la finestra di gioco (F9 = evidenziazioni sì/no con --dev, F10 = turbo automatico sì/no,
F11 tenuto premuto = avanti veloce, F12 = screenshot).

  --adf-dir DIR      cartella con le immagini ADF originali (default: adf, cercata
                     nella cartella corrente, accanto all'eseguibile e nella
                     cartella del progetto); i dischi vengono riconosciuti dal
                     nome di volume (MTV1, MTV2)
  --save-dir DIR     cartella per i salvataggi del gioco (default: save, accanto ad adf)
  --scale N          ingrandimento della finestra (default: 3)
  --no-audio         nessun suono
  --intro            mostra prima del gioco il logo Rainbow Arts e i credits
  --margin N         margine intorno all'immagine in pixel (default: 20); in
                     basso mostra le righe del display oltre la 200
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
  --shot-dir DIR     cartella degli screenshot (default: shots, accanto ad adf)
  --script TESTO     azioni automatiche: \"FRAME:azione; FRAME:azione...\"
                     (oppure @file per leggerle da un file). Azioni:
                       move DX DY   sposta il mouse (conteggi Amiga)
                       goto X Y     porta il puntatore del gioco in (X, Y)
                       click | down | up | rdown | rup   tasti del mouse
                       key NOME | keydown NOME | keyup NOME   tasti (A, ENTER, F1...)
                       type TESTO   scrive un testo
                       shot NOME    salva shots/NOME.png
                       clock SECONDI   orologio di sistema fisso (riproducibilità)
                       cpu auto|P   velocità della CPU
                     Le registrazioni fatte con --record sono script validi.
  --wav FILE         (headless) registra l'audio del gioco in un file WAV
  --gpu              disegna con la GPU (wgpu) invece che in software
                     (sperimentale; se la GPU non parte torna al software)
  --dev              modalità sviluppo: strato di sovrapposizione con le
                     evidenziazioni del trascinamento (F9 le alterna). Senza
                     --dev la finestra mostra solo il gioco emulato.
  --debug-overlay    mostra il mirino sulla punta del puntatore e l'area del
                     puntatore (verifica dello strato di sovrapposizione)
  --record FILE      registra la partita nella finestra (mouse, tasti, orologio):
                     il file è uno script che la riproduce identica
  --trace FILE       scrive ogni chiamata di sistema in FILE
  --dump FILE        salva la RAM emulata (2 MB) in FILE all'arresto
  --program NOME     esegue un altro programma del disco 1 invece di mtv (diagnostica)
  --list             elenca i file dei dischi ed esce
  --help             questo messaggio
";

struct Opts {
    /// Cartelle indicate esplicitamente sulla riga di comando.
    explicit_adf: bool,
    explicit_save: bool,
    explicit_shots: bool,
    adf_dir: PathBuf,
    save_dir: PathBuf,
    scale: usize,
    no_audio: bool,
    intro: bool,
    margin: usize,
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
    wav: Option<PathBuf>,
    program: Option<String>,
    record: Option<PathBuf>,
    debug_overlay: bool,
    dev: bool,
    gpu: bool,
}

fn parse_args() -> Result<Opts, String> {
    let mut o = Opts {
        explicit_adf: false,
        explicit_save: false,
        explicit_shots: false,
        adf_dir: "adf".into(),
        save_dir: "save".into(),
        scale: 3,
        no_audio: false,
        intro: false,
        margin: 20,
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
        wav: None,
        program: None,
        record: None,
        debug_overlay: false,
        dev: false,
        gpu: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("manca il valore per {a}"));
        match a.as_str() {
            "--adf-dir" => {
                o.adf_dir = val()?.into();
                o.explicit_adf = true;
            }
            "--save-dir" => {
                o.save_dir = val()?.into();
                o.explicit_save = true;
            }
            "--scale" => o.scale = val()?.parse().map_err(|e| format!("{e}"))?,
            "--mouse-ratio" => o.mouse_ratio = val()?.parse().map_err(|e| format!("{e}"))?,
            "--headless" => o.headless = true,
            "--full-height" => o.full_height = true,
            "--no-audio" => o.no_audio = true,
            "--intro" => o.intro = true,
            "--no-intro" => o.intro = false,
            "--margin" => o.margin = val()?.parse().map_err(|e| format!("{e}"))?,
            "--cpu-speed" => o.cpu_speed = Some(val()?),
            "--frames" => o.frames = val()?.parse().map_err(|e| format!("{e}"))?,
            "--max-instr" => o.max_instr = val()?.parse().map_err(|e| format!("{e}"))?,
            "--trace" => o.trace = Some(val()?.into()),
            "--shot-every" => o.shot_every = val()?.parse().map_err(|e| format!("{e}"))?,
            "--shot-dir" => {
                o.shot_dir = val()?.into();
                o.explicit_shots = true;
            }
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
            "--wav" => o.wav = Some(val()?.into()),
            "--program" => o.program = Some(val()?),
            "--record" => o.record = Some(val()?.into()),
            "--debug-overlay" => o.debug_overlay = true,
            "--dev" => o.dev = true,
            "--gpu" => o.gpu = true,
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
    let rd = std::fs::read_dir(dir).map_err(|e| {
        format!(
            "non trovo la cartella dei dischi {} ({e}).\nCrea una cartella adf accanto all'eseguibile o nella cartella del progetto\ne copiaci le immagini ADF originali di Mad TV.",
            dir.display()
        )
    })?;
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

/// True se il programma è stato avviato con un doppio click (Esplora risorse):
/// in quel caso Windows crea una console solo per noi, che si chiude all'uscita
/// portandosi via eventuali messaggi d'errore.
#[cfg(windows)]
fn own_console() -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
    }
    let mut list = [0u32; 4];
    // SAFETY: il buffer è valido per 4 elementi, come dichiarato.
    let n = unsafe { GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32) };
    n == 1
}

#[cfg(not(windows))]
fn own_console() -> bool {
    false
}

/// Se la console è solo nostra, aspetta Invio prima di chiuderla.
fn pause_if_own_console() {
    if own_console() {
        eprintln!("\nPremi Invio per chiudere...");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

/// Mostra un errore e termina, lasciando il messaggio leggibile.
fn fatal(msg: &str, code: i32) -> ! {
    eprintln!("errore: {msg}");
    pause_if_own_console();
    std::process::exit(code);
}

/// Trova la cartella di base del progetto, quella che contiene `adf`: la
/// cartella corrente, quella dell'eseguibile o, per un eseguibile compilato in
/// `target\release`, la cartella del progetto due livelli più su.
fn find_base_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
    {
        candidates.push(dir.clone());
        if let Some(up) = dir.parent() {
            candidates.push(up.to_path_buf());
            if let Some(up2) = up.parent() {
                candidates.push(up2.to_path_buf());
            }
        }
    }
    candidates.into_iter().find(|c| c.join("adf").is_dir())
}

fn main() {
    // Anche un errore interno deve restare leggibile se la console è solo nostra.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        pause_if_own_console();
    }));

    let mut o = match parse_args() {
        Ok(o) => o,
        Err(e) => fatal(&format!("{e}\n\n{HELP}"), 2),
    };
    if let Some(base) = find_base_dir() {
        if !o.explicit_adf {
            o.adf_dir = base.join("adf");
        }
        if !o.explicit_save {
            o.save_dir = base.join("save");
        }
        if !o.explicit_shots {
            o.shot_dir = base.join("shots");
        }
    }
    let disks = match load_disks(&o.adf_dir) {
        Ok(d) => d,
        Err(e) => fatal(&e, 1),
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

    let prog = o.program.clone().unwrap_or_else(|| "mtv".into());
    let exe = disks[0]
        .read_file(&prog)
        .unwrap_or_else(|_| panic!("eseguibile '{prog}' non trovato sul disco 1"));
    let intro_disks = disks.clone();
    let mut m = Machine::new(Vfs::new(disks, o.save_dir.clone()));
    if let Some(t) = &o.trace {
        let f = std::fs::File::create(t).expect("impossibile creare il file di trace");
        m.os.trace = Some(std::sync::Arc::new(std::sync::Mutex::new(
            std::io::BufWriter::new(f),
        )));
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
    let args = match (&o.program, std::env::var("MADTV_ARGS")) {
        (Some(_), Ok(a)) => a,
        (Some(_), Err(_)) => String::new(),
        (None, _) => "s".into(),
    };
    let args = args.as_str();
    m.load_executable(&exe, args)
        .expect("eseguibile non valido");

    if std::env::var("MADTV_SEGS").is_ok() {
        for (i, (a, size, kind)) in m.segments.iter().enumerate() {
            println!("segmento {i}: {kind} ${a:06x}-${:06x}", a + size);
        }
        println!("pila: ${:06x}", m.cpu.a(7));
    }
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
                    audio: !o.no_audio,
                    // In basso il margine mostra le righe del display oltre la 200.
                    rows: if o.full_height {
                        render::HEIGHT
                    } else {
                        render::GAME_HEIGHT + o.margin
                    }
                    .min(render::HEIGHT),
                    margin: o.margin,
                    gpu: o.gpu,
                };
                match Frontend::new(&wo) {
                    None => StopReason::FrameLimit,
                    Some(mut fe) => {
                        fe.dev = o.dev || o.debug_overlay;
                        fe.enhancer.debug = o.debug_overlay;
                        fe.enhancer.highlights = o.dev;
                        if let Some(r) = &o.record {
                            match fe.start_recording(r) {
                                Ok(()) => println!("registrazione: {}", r.display()),
                                Err(e) => {
                                    eprintln!("impossibile registrare in {}: {e}", r.display())
                                }
                            }
                        }
                        let go_on = !o.intro
                            || !o.script.is_empty()
                            || play_intro(&mut fe, &intro_disks, &o);
                        if !go_on {
                            StopReason::FrameLimit
                        } else {
                            match fe.run(&mut m, false) {
                                RunEnd::Stopped(r) => r,
                                RunEnd::Skipped | RunEnd::Closed => StopReason::FrameLimit,
                            }
                        }
                    }
                }
            }
        }
    };
    let dt = t0.elapsed();
    if let Some(t) = &m.os.trace {
        use std::io::Write;
        let _ = t.lock().unwrap().flush();
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

/// Intro originale, come nella startup-sequence del disco 1: il logo Rainbow
/// Arts (`rai`, eseguito in una macchina a sé) e la schermata dei credits
/// (`u3.lbm`, mostrata dal visualizzatore `v`; qui la decodifichiamo noi e ne
/// riproduciamo l'effetto a tendina e l'attesa del click). Un click o un tasto
/// saltano il logo e chiudono i credits. False se la finestra è stata chiusa.
fn play_intro(fe: &mut Frontend, disks: &[Adf], o: &Opts) -> bool {
    if let Ok(exe) = disks[0].read_file("rai") {
        let mut intro = Machine::new(Vfs::new(disks.to_vec(), o.save_dir.clone()));
        intro.bus.custom.display.visible_rows = o_rows(o);
        intro.boot_os();
        if intro.load_executable(&exe, "").is_ok() {
            match fe.run(&mut intro, true) {
                RunEnd::Closed => return false,
                RunEnd::Stopped(StopReason::Exited(_)) | RunEnd::Skipped => {}
                RunEnd::Stopped(r) => eprintln!("nota: intro interrotta ({})", describe(&r)),
            }
        }
    }
    if let Ok(data) = disks[0].read_file("u3.lbm") {
        match madtv::iff::decode(&data) {
            Ok(img) => {
                // Adatta l'immagine alla larghezza dello schermo (320 pixel).
                let mut px = vec![0u32; render::WIDTH * img.height];
                for y in 0..img.height {
                    for x in 0..img.width.min(render::WIDTH) {
                        px[y * render::WIDTH + x] = img.pixels[y * img.width + x];
                    }
                }
                // Effetto a tendina dall'alto (1 secondo), poi si attende un
                // click o un tasto, come nell'originale.
                if !fe.show_image(&px, CREDITS_WIPE_FRAMES) {
                    return false;
                }
            }
            Err(e) => eprintln!("nota: credits non mostrati ({e})"),
        }
    }
    true
}

/// Durata della tendina dei credits, in frame (50 = 1 secondo).
const CREDITS_WIPE_FRAMES: u32 = 50;

fn o_rows(o: &Opts) -> usize {
    if o.full_height {
        render::HEIGHT
    } else {
        render::GAME_HEIGHT
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
            Action::Clock(t) => m.os.clock_override = Some(*t),
            Action::Cpu(None) => {
                m.cpu_speed = 100;
                m.auto_turbo = true;
            }
            Action::Cpu(Some(p)) => {
                m.cpu_speed = (*p).clamp(25, 2000);
                m.auto_turbo = false;
            }
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
    let mut wav = Vec::new();
    m.bus.custom.paula.enabled = o.wav.is_some();
    let r = run_headless_inner(m, o, &mut wav);
    if let Some(p) = &o.wav {
        match write_wav(p, &wav, m.bus.custom.paula.rate) {
            Ok(()) => println!(
                "audio: {} ({:.1} s)",
                p.display(),
                wav.len() as f64 / 2.0 / m.bus.custom.paula.rate as f64
            ),
            Err(e) => println!("impossibile scrivere il WAV: {e}"),
        }
    }
    r
}

/// Scrive un WAV stereo a 16 bit.
fn write_wav(path: &Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&2u16.to_le_bytes()); // stereo
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, b)
}

fn run_headless_inner(m: &mut Machine, o: &Opts, wav: &mut Vec<f32>) -> StopReason {
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
        if o.wav.is_some() {
            wav.extend(m.bus.custom.paula.take());
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
    println!("--- audio:\n  {}", c.paula.describe());
    println!(
        "--- memoria: in uso {} KB, massimo {} KB (librerie e strutture emulate comprese)",
        m.os.heap.in_use / 1024,
        m.os.heap.peak / 1024
    );
    if std::env::var("MADTV_HEAP").is_ok() {
        let mut v: Vec<(u32, u32)> = m.os.heap.used.iter().map(|(a, s)| (*s, *a)).collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.0));
        for (size, a) in v.iter().take(15) {
            println!("    blocco ${a:06x}: {} KB", size / 1024);
        }
        let small: u32 = v.iter().filter(|x| x.0 <= 64).map(|x| x.0).sum();
        println!(
            "    blocchi da <= 64 byte: {} ({} KB)",
            v.iter().filter(|x| x.0 <= 64).count(),
            small / 1024
        );
        let total: u32 = v.iter().map(|x| x.0).sum();
        println!(
            "    somma reale di {} blocchi: {} KB; libera: {} KB",
            v.len(),
            total / 1024,
            m.os.heap.available() / 1024
        );
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
