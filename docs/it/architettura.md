# Architettura

*[English](../architecture.md) · Italiano*

## Idea di fondo

Il gioco Amiga è un eseguibile 68000 di circa 270 KB (dopo la decompressione),
scritto in C con Aztec C più circa 9 KB di assembly per il blitter. Invece di
riscriverlo subito, lo eseguiamo così com'è dentro un "Amiga minimo" scritto in
Rust, che contiene solo ciò che il gioco usa davvero.

```
            ┌────────────────────────────────────────────┐
            │  codice originale di Mad TV (68000)         │
            └──────────┬──────────────────────┬──────────┘
        jsr -LVO(a6)   │                      │  accesso diretto
      (trap A-line)    ▼                      ▼  ai registri $DFFxxx
            ┌────────────────────┐   ┌───────────────────────┐
            │ os/: AmigaOS HLE   │   │ custom.rs: copper,     │
            │ exec, dos, gfx,    │   │ blitter, interrupt,    │
            │ intuition, vfs     │   │ mouse, CIA             │
            └─────────┬──────────┘   └──────────┬────────────┘
                      │                          │
                      ▼                          ▼
            ┌────────────────────┐   ┌───────────────────────┐
            │ adf.rs: dischi ADF │   │ render.rs: display     │
            │ + salvataggi su PC │   │ riga per riga          │
            └────────────────────┘   └──────────┬────────────┘
                                                 ▼
                                     ┌───────────────────────┐
                                     │ window.rs + input.rs   │
                                     │ finestra, mouse, tasti │
                                     └───────────────────────┘
```

## La CPU

Usiamo il crate `m68k` in modalità "passo singolo" (`CpuCore::step`), che
restituisce un `StepResult`. Due casi ci interessano:

- `Ok { cycles }`: istruzione eseguita; i cicli fanno avanzare il tempo
  emulato (pennello elettronico, copper, frame);
- `AlineTrap { opcode }`: il programma ha eseguito un opcode `$Axxx`. Non
  esiste sul 68000: lo usiamo come "chiamata al sistema operativo emulato".

Attenzione (vedi NOTES.it.md): ogni volta che cambiamo il PC dall'esterno va
chiamato `invalidate_prefetch()`, e il livello di interrupt va reimpostato a
ogni istruzione.

## Il sistema operativo emulato

Le librerie Amiga sono tabelle di salto a offset negativi rispetto alla base:
il programma chiama `jsr -198(a6)` per `AllocMem`, dove a6 = ExecBase. Noi
creiamo librerie in cui ogni voce di 6 byte contiene:

```
$A000 | (numero_libreria << 9) | indice_funzione   ; trap
rts
nop
```

Quando la CPU incontra la trap, `Os::handle_trap` decodifica libreria e
funzione, esegue l'implementazione in Rust (in `os/exec.rs`, `os/dos.rs`...),
mette il risultato in d0 ed emula l'`rts`. Se la funzione non è ancora
implementata, l'emulazione si ferma con un messaggio che dice esattamente quale
funzione serve e da dove è stata chiamata: così implementiamo solo ciò che serve.

Numeri di libreria: 0 exec, 1 dos, 2 intuition, 3 graphics, 4 icon,
7 trap interne (fine programma, dispatcher degli interrupt, eccezioni inattese).

### Ambiente di avvio

`Machine::boot_os` e `Machine::load_executable` ricreano ciò che AmigaDOS
prepara per un comando lanciato dalla Shell:

- ExecBase all'indirizzo 4, con `AttnFlags` = 0 (68000) e frequenza PAL;
- una struct Process con la sua CLI (nome del comando, seglist, console);
- l'eseguibile caricato come farebbe `LoadSeg`: ogni segmento preceduto da
  lunghezza e puntatore BCPL al successivo, rilocazioni applicate;
- pila utente con indirizzo di ritorno (→ fine programma) e dimensione;
- a0/d0 = argomenti (`"s\n"`, come nella startup-sequence originale).

Il decompressore Imploder dell'eseguibile non è stato riscritto: gira nel 68000
emulato, esattamente come su un Amiga vero, e usa `FreeMem` per liberare i
propri segmenti alla fine.

### File system virtuale

`os/vfs.rs` monta gli ADF per nome di volume (`MTV1:`, `MTV2:`, più `DF0:`,
`SYS:`). Le scritture (salvataggi) vanno in `save/<VOLUME>/...` sul PC, che
viene consultata per prima anche in lettura: gli ADF originali non vengono mai
modificati.

## I chip custom

`custom.rs` tiene tutti i registri `$DFF000-$DFF1FE`. Implementati:

- **tempo**: 454 cicli CPU per riga, 313 righe per frame (PAL);
  VPOSR/VHPOSR coerenti con i cicli;
- **interrupt**: INTENA/INTREQ; il livello 68000 corrispondente viene
  presentato alla CPU a ogni istruzione;
- **copper**: eseguito riga per riga (MOVE, WAIT; SKIP ignorato), riparte da
  COP1LC a ogni frame. È indispensabile perché il gioco usa un interrupt
  generato dal copper per scandire il tempo;
- **blitter**: modalità area completa (4 canali, scorrimenti, maschere,
  modalità discendente, riempimento); termina istantaneamente;
- **mouse/tastiera**: JOY0DAT, POTGOR, CIA-A PRA (tasti del mouse), CIA-A SDR (tastiera);
- **sprite**: puntatori SPRxPT; il DMA vero e proprio è nel display.

## Il display

`render.rs` disegna l'immagine **riga per riga durante l'emulazione**: a ogni
riga di scansione `Custom::advance` esegue prima il copper e poi
`Display::line`, che:

- legge i bitplane dagli indirizzi correnti BPLxPT e li fa avanzare di
  (parole lette × 2 + modulo), come il DMA dell'hardware: le bitmap
  interleaved del gioco e i cambi di puntatore a metà schermo funzionano senza
  casi speciali;
- usa la palette in vigore su quella riga (il copper può cambiarla);
- gestisce il DMA degli sprite (intestazioni POS/CTL, coppie di parole dati,
  sprite collegati a 15 colori) e li disegna sopra il playfield;
- all'inizio di ogni frame scambia i buffer: `display.front` è sempre l'ultimo
  frame completo, pronto per la finestra o per un PNG.

Gestisce lores/hires (le righe hires vengono dimezzate in larghezza),
finestra di visualizzazione (DIWSTRT/DIWSTOP), fetch (DDFSTRT/DDFSTOP),
moduli ed Extra Half-Brite. Mancano priorità degli sprite, scroll fine, dual
playfield e HAM.

## Input

`input.rs` traduce mouse e tastiera nei registri che il gioco legge:

- mouse: contatori di JOY0DAT (1 conteggio = 1 pixel lores), tasto sinistro
  sul bit 6 di CIA-A PRA, destro sul bit 10 di POTGOR;
- tastiera: CIA-A SDR, con la codifica della tastiera Amiga (codice ruotato a
  sinistra e negato, bit 7 = rilascio).

Nella finestra (`window.rs`) il puntatore del gioco viene tenuto allineato con
quello del PC: a ogni frame leggiamo la posizione del puntatore dalle variabili
del gioco (`Machine::game_pointer`) e inviamo la differenza come movimento del
mouse. Il gioco accumula i movimenti nell'interrupt e li applica nel ciclo
principale, a volte con qualche frame di ritardo: `input::pointer_step` legge
anche gli accumulatori e i limiti del puntatore della schermata corrente
(`Machine::game_mouse`), così non reinvia mai un movimento già fatto.

## Velocità della CPU

`Machine::cpu_speed` (percentuale di un A500) cambia quanti cicli del chipset
corrispondono a un ciclo della CPU: al 300% la CPU esegue tre volte più
istruzioni per frame. Il gioco scandisce il proprio tempo con l'interrupt di
frame, quindi il ritmo di gioco non cambia: si accorciano solo le attese di
calcolo (decompressione e disegno delle schermate). Alcune animazioni però sono
temporizzate dalla CPU e accelererebbero anch'esse; per questo nella finestra si
usa il **turbo automatico** (`Machine::auto_turbo`): CPU al 100%, che sale a
`turbo_speed` dopo ogni Open/Read del dos e torna normale appena il gioco
chiama la propria funzione che legge il contatore dei frame
(`Machine::pacing_pc`), cioè quando ricomincia a sincronizzarsi sul tempo.

### Script di azioni

`input.rs` definisce anche un piccolo linguaggio per automatizzare il gioco
senza finestra: voci `FRAME:azione` (`goto X Y`, `move DX DY`, `click`,
`key NOME`, `type TESTO`, `shot NOME`...). Serve per i test, per riprodurre un
problema e per confrontare il comportamento con WinUAE. Esempio in
`scripts/nuova-partita.txt`.

## Eccezioni

Le trap A-line sono il nostro canale verso il sistema operativo emulato. Le
altre eccezioni sincrone (TRAP #n, F-line, ILLEGAL) vengono prese dalla CPU
come sull'hardware, con salto al vettore: alcuni programmi (l'intro `rai`)
installano un proprio vettore e usano `TRAP #0` per passare in modo
supervisore. I vettori non installati dal programma puntano a stub che
fermano l'emulazione con un messaggio ("eccezione inattesa").

## La finestra

`window.rs` usa `minifb`. Un `Frontend` tiene aperta la finestra per tutta la
sessione ed esegue in sequenza ciò che faceva la startup-sequence del disco:
l'intro `rai` in una macchina emulata a sé (saltabile con un click o un tasto),
la schermata dei credits (`u3.lbm`, decodificata da `iff.rs`), poi il gioco in
una macchina nuova (l'intro è disattivata di default: `--intro`). L'immagine
viene ingrandita a pixel interi dentro un margine (20 pixel di default): nero in
alto e ai lati, mentre in basso il margine mostra le righe del display oltre la
200. 50 frame al secondo. Il cursore del PC è nascosto perché il
puntatore lo disegna il gioco. Avanti veloce automatico finché lo schermo è nero
(caricamento) e con F11; F12 salva uno screenshot.

## Audio

`paula.rs` emula i 4 canali di Paula. Per ogni canale: registri AUDxLC,
AUDxLEN, AUDxPER, AUDxVOL, AUDxDAT; all'accensione del DMA (DMACON) il canale
carica puntatore e lunghezza, legge la prima parola e segnala l'interrupt
(il programma può già preparare il blocco successivo); a fine blocco ricarica
dai registri e segnala di nuovo. Ogni campione da 8 bit dura AUDxPER cicli del
clock audio (3,546895 MHz, metà del clock della CPU): il tempo è quello del
chipset, quindi l'intonazione non dipende dalla velocità della CPU emulata.

Il mixaggio segue l'hardware (canali 0 e 3 a sinistra, 1 e 2 a destra). Il
ricampionamento alla frequenza della scheda audio integra l'uscita su ogni
intervallo (media mobile), poi vengono applicati il passa-basso fisso
dell'A500 (circa 4,4 kHz) e, se il programma lo accende tramite il bit 1 della
CIA-A PRA, il filtro "LED" (circa 3,3 kHz, 12 dB/ottava).

`audio.rs` apre il dispositivo predefinito con `cpal` e accoda i campioni in un
buffer condiviso con il callback della scheda. La finestra emula un frame solo
quando in coda restano meno di 3 frame di audio (60 ms): è la scheda audio a
dettare il ritmo, così non si accumulano né buchi né ritardo. Durante l'avanti
veloce l'audio viene scartato. Senza dispositivo audio la finestra torna al
ritmo fisso di 50 frame al secondo.

## Registrazione e riproduzione

L'emulazione è deterministica: a parità di input frame per frame, lo stato
della macchina è identico. `--record` scrive in uno script i movimenti del
mouse effettivamente inviati al gioco, i tasti, l'orologio di sistema
(`clock`, che fissa ciò che restituisce DateStamp) e la modalità della CPU
(`cpu`). Rigiocato con `--headless --script @FILE`, ricostruisce la partita:
è lo strumento per analizzare comportamenti che si raggiungono solo giocando.

## Strato di sovrapposizione

`overlay.rs` disegna forme (contorni, riempimenti trasparenti, linee) in
coordinate del gioco, ma alla risoluzione della finestra, dopo
l'ingrandimento, con maschere (`Mask`/`Unmask`) per lasciare scoperti oggetti
del gioco come il pannello trascinato. `enhance.rs` calcola a ogni frame le
forme da mostrare leggendo lo stato del gioco dalla memoria emulata; la
finestra le compone sopra l'immagine. Lo strato esiste solo in modalità
sviluppo (`--dev`): di default la finestra mostra solo il gioco.

## Disegno su GPU

`gpu.rs` è un'alternativa al disegno software della finestra, attiva con
`--gpu`. Riceve esattamente gli stessi dati (l'immagine del gioco, larga 320
pixel, e l'elenco di forme dello strato) e li disegna con `wgpu` in un solo
render pass con due pipeline:

1. **gioco**: un triangolo che copre tutta la finestra; il fragment shader
   calcola quale pixel del gioco cade sotto ogni pixel della finestra (stessa
   scala intera e stesso margine del percorso software) e lo legge con
   `textureLoad`, senza sampler, quindi l'ingrandimento è lo stesso, a blocchi;
2. **forme**: rettangoli in pixel della finestra disegnati come istanze, con
   trasparenza e fino a due maschere per istanza, applicate scartando i
   frammenti che vi cadono dentro.

La superficie si crea sulla finestra di `minifb` attraverso i suoi handle
`raw-window-handle`; in modalità GPU la finestra non riceve mai un buffer
(`update` elabora solo gli eventi), così minifb lascia il disegno al sistema.
Modalità di presentazione Mailbox o Immediate se disponibili (il ritmo lo
detta la scheda audio, non il vsync), altrimenti Fifo. Si sceglie un formato
non sRGB perché i colori vengano scritti così come sono, come nel percorso
software. Se qualcosa non va, la finestra torna al disegno software.

`Gpu::offscreen` disegna su una texture che si può rileggere: il test
`gpu::tests::stesso_risultato_del_disegno_software` e
`GPU=1 cargo run --release --example highlight_demo` confrontano i due
percorsi (0 pixel diversi su frame veri del gioco con le evidenziazioni, con
lavapipe, il Vulkan software di Mesa).

## Copie della macchina e click di prova

`Machine` implementa `Clone`: RAM, chip custom e sistema operativo emulato
vengono copiati (i dischi ADF sono condivisi con `Arc`), i registri della CPU
passano per la serializzazione (feature `serde` del core `m68k` + `bincode`),
perché il core non implementa `Clone`. Una copia costa circa 2 ms ed evolve
identica all'originale. La copia ha il file system in sola lettura
(`Vfs::read_only`): scritture e cancellazioni vengono scartate.

Le evidenziazioni la usano per chiedere al gioco "se cliccassi adesso, cosa
succederebbe?" (`enhance::probe_drop`): sulla copia si preme il tasto
sinistro, si eseguono fino a 6 frame e si guarda se l'oggetto trascinato è
stato sganciato. Così le regole di ogni stanza restano quelle del gioco
originale, senza riscriverle. Una variante lunga (`enhance::probe_landing`, ~85 ms, in un
thread a parte) fa proseguire la copia finché il gioco ridisegna l'oggetto
rilasciato, per scoprire dove arriva.

Per l'analisi, il bus ha un punto di osservazione (`Bus::watch`, intervallo di
RAM: ogni accesso della CPU finisce in `Machine::watch_log` con il PC
dell'istruzione) e la macchina dei punti di sonda (`Machine::probe_pcs`:
quando la CPU sta per eseguire uno di quegli indirizzi annota frame e stack).

## Memoria

RAM piatta di 2 MB, tutta trattata come Chip RAM (i chip sono emulati in
software, quindi il limite di 512 KB dell'A500 non serve):

| Indirizzi | Uso |
|---|---|
| `$000000-$0003FF` | vettori di eccezione |
| `$001000-$001FFF` | pila del supervisore |
| `$002000-$1FEFFF` | memoria gestita da AllocMem (librerie, processo, gioco) |
| `$BFD000-$BFEFFF` | CIA |
| `$DFF000-$DFF1FF` | chip custom |
