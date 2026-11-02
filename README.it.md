# Mad TV — porting Rust della versione Amiga

*[English](README.md) · Italiano*

Porting per Windows (e Linux/macOS) di **Mad TV** (Rainbow Arts, 1991), il
gestionale in cui dirigi un'emittente televisiva.

Il progetto **non contiene nulla del gioco originale**: grafica, testi, musica
ed eseguibile vengono letti dalle immagini disco originali (ADF) che fornisci
tu, come fanno OpenRCT2 o ScummVM.

## Stato

**Fase 3 completata**: il gioco si gioca in una finestra, con mouse, tastiera
e audio (musica TFMX ed effetti, attraverso l'emulazione di Paula).

| Selezione dei giocatori | L'edificio della rete | Il capo |
|---|---|---|
| ![](docs/img/fase1-selezione-giocatori.png) | ![](docs/img/fase2-edificio.png) | ![](docs/img/fase2-capo.png) |

**Evidenziazioni** (novembre 2026): mentre trascini un pannello o una
cassetta, il porting mostra il punto che il gioco usa davvero per decidere dove
rilasci (l'angolo in alto a sinistra dell'oggetto, non la punta della freccia)
e lo colora di verde se lì il rilascio verrebbe accettato, di rosso se no; nel
palinsesto evidenzia anche la casella di destinazione. La risposta la dà il
gioco stesso, con un click "di prova" su una copia della macchina emulata.
Sono uno strumento di sviluppo: si attivano con `--dev` (poi F9 le alterna);
di default la finestra mostra solo il gioco emulato, senza nulla sopra.

![](docs/img/evidenziazione-palinsesto.png)

Lo stato dettagliato e i prossimi passi sono in [NOTES.it.md](NOTES.it.md).

## Come funziona, in breve

Il gioco Amiga è scritto in C (Aztec C) con alcune routine in assembly 68000
per il blitter. Invece di riscriverlo da zero, il porting:

1. legge i dischi ADF originali;
2. esegue il codice 68000 originale con il core [`m68k`](https://crates.io/crates/m68k)
   (puro Rust, licenza MIT);
3. sostituisce AmigaOS con un'implementazione in Rust delle poche funzioni di
   sistema che il gioco usa (circa 20) — niente ROM Kickstart;
4. emula solo i chip custom che il gioco tocca davvero: copper, blitter,
   bitplane, sprite, palette, mouse, tastiera, audio.

I dettagli sono in [docs/it/architettura.md](docs/it/architettura.md); l'analisi dei
due eseguibili originali (Amiga e DOS) in [docs/it/analisi.md](docs/it/analisi.md).
L'**enciclopedia della grafica** ([docs/it/grafica/](docs/it/grafica/README.md))
spiega come il gioco conserva e disegna la grafica, con il catalogo di tutti i
file; le immagini si estraggono dai propri dischi con
`cargo run --release --example extract_assets`.

## Compilare ed eseguire (Windows)

Serve Rust (https://rustup.rs). Dalla cartella del progetto:

```
cargo build --release
```

Copia le due immagini ADF originali di Mad TV nella cartella `adf\` (i nomi
dei file non contano: i dischi vengono riconosciuti dal nome di volume, MTV1 e
MTV2). Poi avvia il gioco:

```
target\release\madtv.exe
```

Si può avviare anche con un doppio click da Esplora risorse: il programma cerca
la cartella `adf` nella cartella corrente, accanto all'eseguibile e nella
cartella del progetto (due livelli sopra `target\release`); `save` e `shots`
vengono create accanto ad `adf`. Per una copia "portatile" basta mettere
`madtv.exe` e la cartella `adf` nella stessa cartella. In caso di errore, se il
programma è stato avviato con un doppio click la console resta aperta con il
messaggio finché non si preme Invio.

Con `--intro`, prima del gioco partono come sul disco originale il logo Rainbow
Arts (un click o un tasto lo saltano) e la schermata dei credits, che scende a
tendina e attende un click. L'immagine ha un margine di 20 pixel tutto intorno;
quello inferiore mostra le righe del display oltre la 200, usate dall'intro. Il caricamento del gioco viene accelerato automaticamente (schermo
nero per pochi secondi). Nella finestra la CPU emulata ha la velocità di un A500, con un
**turbo automatico** durante i caricamenti dal disco: cambi di schermata e
aperture delle stanze sono più rapidi, mentre animazioni e ritmo del gioco
restano quelli originali. Comandi:

| Tasto | Effetto |
|---|---|
| mouse | il puntatore del gioco segue quello del PC |
| tastiera | come su un Amiga (nomi dei giocatori, ecc.) |
| F9 | evidenziazioni sì / no (solo con `--dev`) |
| F10 | turbo automatico sì (default) / no |
| F11 (tenuto premuto) | avanti veloce |
| F12 | screenshot nella cartella `shots\` |

Opzioni utili (`madtv --help` per l'elenco completo):

| Opzione | Effetto |
|---|---|
| `--scale N` | ingrandimento della finestra (default 3: 1080×720 con il margine) |
| `--margin N` | margine intorno all'immagine in pixel (default 20, 0 per nessun margine) |
| `--full-height` | mostra tutte le 256 righe PAL invece delle 200 usate dal gioco |
| `--no-audio` | gioca senza suono |
| `--intro` | mostra il logo Rainbow Arts e i credits prima del gioco |
| `--headless --wav audio.wav` | registra in un file l'audio prodotto dal gioco |
| `--cpu-speed P` | velocità della CPU: `auto` (default nella finestra) o una percentuale fissa di un A500, es. `300` |
| `--headless --frames N` | nessuna finestra: esegue N frame (50 = 1 secondo) e salva `shots\final.png` |
| `--script "1290:goto 55 101; 1320:click; 2000:shot capo"` | azioni automatiche a frame stabiliti (anche da file: `--script @scripts\nuova-partita.txt`) |
| `--record partita.txt` | registra la partita (mouse, tasti): il file la riproduce identica con `--headless --script @partita.txt` |
| `--originale` | esegue solo il codice originale del gioco, senza le funzioni riscritte in Rust (più lento in alcuni punti) |
| `--gpu` | disegna con la GPU (wgpu) invece che in software; sperimentale, se la GPU non parte torna al software |
| `--dev` | modalità sviluppo: strato di sovrapposizione con le evidenziazioni del trascinamento |
| `--debug-overlay` | mirino sulla punta del puntatore (verifica dello strato di sovrapposizione) |
| `--trace trace.log` | registra ogni chiamata di sistema del gioco |
| `--dump ram.bin` | salva la RAM emulata alla fine (per l'analisi con un disassembler) |

I salvataggi del gioco finiscono nella cartella `save\`; gli ADF originali non
vengono mai modificati.

## Test

```
cargo test
```

## Struttura

```
src/
  adf.rs        lettore di immagini disco Amiga (OFS/FFS)
  hunk.rs       caricatore degli eseguibili AmigaOS
  bus.rs        memoria e mappa degli indirizzi
  custom.rs     chip custom: copper, blitter, interrupt, mouse, CIA
  render.rs     display riga per riga: bitplane e sprite
  paula.rs      audio: i 4 canali di Paula, mixaggio e filtri
  audio.rs      uscita verso la scheda audio del PC (cpal)
  input.rs      mouse, tastiera, script di azioni automatiche
  iff.rs        decodifica delle immagini IFF ILBM (schermata dei credits)
  window.rs     finestra (minifb): intro, credits, gioco, registrazione
  overlay.rs    strato di sovrapposizione nitido (evidenziazioni)
  hooks.rs      funzioni del gioco riscritte in Rust (agganci)
  gpu.rs        disegno su GPU (wgpu): immagine del gioco + strato, alternativo al software
  enhance.rs    miglioramenti dell'interfaccia sopra il gioco
  machine.rs    CPU + bus + sistema operativo; ciclo principale
  os/           AmigaOS emulato: exec, dos, graphics, intuition, file system virtuale
  main.rs       programma a riga di comando
docs/           documentazione tecnica (inglese); docs/it/ = versione italiana;
                docs/it/grafica/ = enciclopedia della grafica
scripts/        esempi di script di azioni; registrazioni/ = partite di prova
examples/       strumenti di analisi (drag_lab, probe, drop_lab, highlight_demo...)
NOTES.it.md     stato del lavoro e prossimi passi (NOTES.md in inglese)
```

## Licenza

Il codice di questo progetto è distribuito con licenza
[GNU GPL versione 3 o successiva](LICENSE).

La licenza copre solo il codice scritto per il porting (emulatore, sistema
operativo emulato, strumenti). Mad TV, con la sua grafica, i testi, la musica e
il codice originale, appartiene ai rispettivi titolari dei diritti (Rainbow
Arts) e non è incluso nel repository.
