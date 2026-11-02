# Note di lavoro

Questo file è la "memoria" del progetto tra una sessione e l'altra: stato,
scoperte, decisioni e prossimi passi. Va aggiornato alla fine di ogni sessione.

## Stato attuale (fine fase 3, 1 novembre 2026)

- **Giocabile in una finestra** (minifb, 50 Hz, 960×600 di default), con mouse
  e tastiera: confermato da Pam su Windows con una partita vera (palinsesto,
  programmi in onda, orologio). Provato via script: nuova partita, vista dell'edificio, dialoghi
  del capo, modifica dei nomi dei giocatori con la tastiera.
- Il caricamento iniziale (circa 25 s a velocità reale) viene accelerato in
  automatico finché lo schermo è nero; F11 tenuto premuto = avanti veloce.
- Velocità dell'emulazione: circa 1.000 frame/s (20 volte il tempo reale).
- **Audio**: Paula emulata (4 canali DMA, ricarica con interrupt, modo
  manuale, filtri A500 e LED), uscita con `cpal`; nella finestra è la scheda
  audio a dettare il ritmo dei frame. Da confermare all'ascolto su Windows.
- La modalità senza finestra (`--headless`) resta, con gli script di azioni,
  per test e analisi.

### Storia

- Fase 1: avvio fino alla schermata di selezione dei giocatori.
- Fase 2: renderer riga per riga, sprite, finestra, mouse, tastiera, script.
- Fase 3: audio, intro originale (logo Rainbow Arts e credits).
- Correzioni dopo la prova di Pam su Windows: puntatore che sfarfallava
  (soprattutto ai bordi), ritardo nell'apertura delle finestre del gioco,
  puntatore che saltava sulle zone con etichetta al passaggio del mouse,
  animazioni troppo veloci con la CPU al 300% (da cui il turbo automatico).

## Versione dei dischi

Set "Mad TV v2.1 (1991)(Rainbow Arts)", con file di dati in spagnolo e
interfaccia in inglese (vedi domande aperte). MD5:

- disco 1 (MTV1): `77f660cbb2c6ab7d4979f09acda3ebf7`
- disco 2 (MTV2): `a59073203f3d695bd261bba2b0057447`

Per WinUAE (confronti con l'originale): Kickstart 1.3 rev 34.5 A500,
`kick34005.A500`, CRC32 `c4f0f55f`, con **512 KB Chip + 512 KB Slow RAM**:
il gioco occupa circa 600 KB (circa 270 KB allocati dinamicamente, di cui
200 KB in Chip RAM, più circa 310 KB di codice e dati), quindi con soli
512 KB non parte. Misura: il rapporto finale dell'emulatore riporta la memoria
in uso e il massimo (`MADTV_HEAP=1` elenca i blocchi più grandi).

`add21` (primo comando della startup-sequence) è estetico: ritocca la
viewport dello schermo del Workbench e lo ridisegna; non serve al gioco.

## Cosa fa il gioco con il sistema (dal trace)

Funzioni chiamate fino alla schermata di selezione:

- exec: `AllocMem`, `FreeMem`, `Forbid`, `Permit`, `OldOpenLibrary`,
  `FindTask`, `SetSignal`
- dos: `Open`, `Close`, `Read`, `Seek`, `Lock`, `UnLock`, `Examine`,
  `Input`, `Output`, `Write`, `IsInteractive`
- intuition, graphics: **nessuna chiamata**. Il gioco programma l'hardware da sé.

Hardware usato direttamente:

- installa il proprio gestore di interrupt di livello 3 scrivendo nel vettore
  `$6C` (indirizzo `$0401CC` nel gioco caricato); se l'interrupt non è COPER
  salta al vecchio vettore;
- la sua copper list (a `$04F338`) imposta 5 bitplane **interleaved**
  (modulo 160), 32 colori, e termina con `MOVE $8010,INTREQ`: un interrupt
  copper a ogni frame, che incrementa il contatore dei frame (`$0456E0`), legge
  la tastiera (CIA-A SDR `$BFEC01`) e il mouse (JOY0DAT);
- blitter in modalità area, mai (finora) in modalità linea;
- POTGO/POTGOR a ogni frame (tasto destro del mouse).
- **Puntatore del mouse**: lo disegna il gioco col blitter (non è uno sprite;
  la copper list spegne anzi il DMA degli sprite con `MOVE $0020,DMACON`).
  La sua posizione, in pixel dello schermo, sta in due word all'offset
  `$40`/`$42` del segmento dati (segmento 2 della seglist; `$044650` nel nostro
  caricamento). Una copia a `+6` sembra la posizione precedente. La finestra la
  usa per tenere allineato il puntatore del gioco con quello del PC.
- **Mouse**: 1 conteggio del mouse Amiga = 1 pixel lores (misurato).
  Come lo gestisce il gioco (letto nel codice):
  - l'interrupt di frame (`$0401CC`) legge JOY0DAT, calcola lo spostamento
    del frame (`$045806`/`$045808`) e lo **somma** in due accumulatori,
    offset `$2C2E`/`$2C30` del segmento dati (`$04723E`/`$047240`): i
    movimenti non vanno mai persi;
  - il ciclo principale (`$015530`) svuota gli accumulatori e sposta il
    puntatore, tutto in una volta o a pezzi (lo smorzamento, funzione
    `$0167DC`); quando è occupato (per esempio mentre disegna un'etichetta al
    passaggio del mouse) salta qualche frame;
  - il puntatore viene limitato tra quattro variabili che cambiano con la
    schermata: offset `$2BD2`/`$2BD4` (minimi X/Y) e `$2BD6`/`$2BD8`
    (massimi; 303/189 nella schermata principale).
  La finestra (`input::pointer_step`) usa queste variabili: posizione
  prevista = puntatore + accumulatori, bersaglio limitato ai limiti della
  schermata. Una prima versione stimava i movimenti in sospeso e dopo 3 frame
  di puntatore fermo li dava per persi: quando il gioco era occupato li
  reinviava e il puntatore saltava (fino a 30 px oltre il bersaglio).
  Strumenti di misura: `cargo run --release --example mouse_lab` e
  `CLICKS=3 SPEED=3 cargo run --release --example mouse_sweep` (scorrimento
  sopra le porte dell'edificio, con le etichette al passaggio del mouse).
- **Velocità**: le transizioni (apertura di stanze, cambi di schermata) sono
  limitate dalla CPU, perché dopo aver letto i file il gioco li decomprime.
  Alcune animazioni invece sono temporizzate dalla CPU (non dal frame): con
  la CPU al 300% fissa "vanno a razzo" (segnalato da Pam). Soluzione:
  **turbo automatico** (`--cpu-speed auto`, default nella finestra): CPU al
  100% di norma, al 400% dopo ogni Open/Read del dos, finché il gioco non
  torna a chiamare la sua funzione che legge il contatore dei frame
  (`$0434FC`, offset `$40364` del segmento 1: restituisce la word a
  `$0456E0`, incrementata dall'interrupt). Misure dal click su "New game":

  | | primo cambiamento | capo | animazione dell'edificio |
  |---|---|---|---|
  | 100% | 4,3 s | 12,2 s | 100 frame |
  | 300% fissa | 1,5 s | 4 s | accelerata |
  | auto | 1,4 s | 6,4 s | 100 frame (come l'originale) |

  L'orologio del gioco è scandito dagli interrupt di frame: avanza di 10
  minuti ogni 1.000 frame a qualunque velocità della CPU. In headless la
  velocità resta 100% fissa, per avere script riproducibili.

## Bug trovati e risolti in fase 1 (utili per il futuro)

1. **Coda di prefetch**: il core `m68k` emula il prefetch del 68000; dopo ogni
   cambio di PC fatto dall'esterno (trap, Supervisor...) va chiamato
   `cpu.invalidate_prefetch()`, altrimenti la CPU esegue parole vecchie.
2. **Livello di interrupt**: il core azzera `int_level` dopo aver servito un
   interrupt; va reimpostato a ogni istruzione (`set_irq`), come la linea IPL.
3. **Stato iniziale dell'hardware**: AmigaOS lascia DMACON = `$03F0` e
   INTENA = `$602C`; il gioco non li imposta da sé e senza questi valori non
   parte né il copper né l'interrupt.
4. **Copper in tempo reale**: serve eseguirlo riga per riga durante
   l'emulazione (non solo per disegnare) perché genera l'interrupt che fa
   avanzare il gioco.

## Limiti noti

- Renderer: niente priorità sprite/playfield (BPLCON2), niente scroll fine
  (BPLCON1), niente dual playfield né HAM; i cambi di registro a metà riga
  valgono per tutta la riga; le righe hires vengono dimezzate.
- Blitter: modalità linea non emulata (viene contata nel rapporto; finora mai usata).
- Tastiera: nessun tastierino numerico; i tasti senza equivalente Amiga sono ignorati.
- Audio: niente modulazione tra canali (ADKCON) e temporizzazione degli slot
  DMA approssimata; non emulati i timer delle CIA (il gioco finora non li usa).
- La startup-sequence originale lancia anche `rai` (intro Rainbow Arts) e
  `v u3.lbm` (visualizzatore IFF): per ora eseguiamo solo `mtv s`.
- Delay() salta i frame in un colpo solo.
- La finestra non è ridimensionabile (scala fissa scelta all'avvio).
- Altezza e margini: gli screenshot mostrano 200 righe (320×200), le sole che
  il gioco usa in tutte le schermate provate (DIWSTRT/DIWSTOP $2C-$F4). La
  finestra, su richiesta di Pam, ha un margine di 20 pixel (`--margin`): nero
  in alto e ai lati, mentre in basso mostra le righe 200-219 del display, così
  l'intro (220 righe) è completa. Finestra di 360×240 pixel del gioco
  (1080×720 alla scala 3). `--full-height` mostra le 256 righe PAL.
- Intro disattivata di default (scelta di Pam): si attiva con `--intro`.
- Avvio con doppio click (segnalato da Pam: falliva in silenzio): la cartella
  corrente è `target\release`, quindi `adf` non veniva trovata e la console si
  chiudeva subito. Ora `adf` si cerca anche accanto all'eseguibile e due livelli
  sopra; con una console tutta nostra (`GetConsoleProcessList` = 1) gli errori,
  anche interni, aspettano Invio. Codice specifico di Windows non compilabile
  nella sandbox: da verificare su Windows.

## Audio (fase 3)

- **Musica**: TFMX. Il replayer è `098.HES` (disco 1), le canzoni sono coppie
  canzone+campioni sul disco 2 (094/095, 086/087...). All'avvio il gioco
  prova `Lock("078.HES")` nella directory corrente e, se non lo trova, usa
  `MTV2:078.HES`: la musica viene caricata regolarmente (il dubbio della fase 1
  era infondato).
- Il replayer riscrive i periodi dei canali 0-2 a ogni frame (50 Hz, dal suo
  interrupt); a fine nota mette il canale in loop su una parola di zeri.
- Il gioco non abilita gli interrupt audio (INTENA = `$603C`), quindi gli
  interrupt AUD0-3 di Paula vengono generati ma non serviti; il livello 4 resta
  comunque gestito.
- Il silenzio sulla schermata di selezione dei giocatori è fedele: confermato
  in WinUAE (anche l'originale non ha musica nel menu iniziale).
- Strumenti: `--headless --wav FILE` registra l'audio; il rapporto finale mostra
  lo stato dei canali; `cargo run --release --example audio_check` verifica il
  dispositivo audio.

## Intro (logo Rainbow Arts e credits)

- **`rai`** (logo Rainbow Arts, con la miccia che brucia lungo la cornice e il
  rumore crepitante "di vetri" segnalato da Pam in WinUAE) gira nell'emulatore
  in una macchina a sé, prima del gioco. Usa due chiamate a exec e poi prende
  il controllo con `TRAP #0` dopo aver puntato il vettore della trap sul
  proprio codice: per questo il ciclo principale ora fa prendere alla CPU le
  vere eccezioni TRAP/F-line/ILLEGAL (salto al vettore) invece di fermarsi.
  Lavora a indirizzi fissi oltre `$030000` (copper list, 4 bitplane, finestra
  video di 220 righe: le ultime 20 non entrano nella finestra a 200 righe).
  Dura 658 frame (13 s) e termina da sé. Ai bordi compaiono righe rosse, gialle
  e blu che sfumano con il logo: sono pixel disegnati dall'intro stessa
  (memoria libera e azzerata nel nostro emulatore), probabilmente un effetto
  voluto; da confermare in WinUAE.
- **Credits**: `v u3.lbm`. Il visualizzatore `v` userebbe intuition
  (OpenScreen) e graphics; invece di emularlo, decodifichiamo noi l'immagine
  IFF ILBM (`iff.rs`: 320×256, 32 colori, ByteRun1) e riproduciamo il
  comportamento dell'originale osservato da Pam in WinUAE: comparsa con effetto
  a tendina dall'alto verso il basso, nessun audio, attesa di un click per
  proseguire. La durata della tendina (`CREDITS_WIPE_FRAMES` in `main.rs`, ora
  1 secondo) è una stima da confrontare con l'originale.
- **Crediti ufficiali** (dall'immagine): Project Manager Ralph Stock,
  Programming PC Stefan Hoffmann, Programming Christian Krämer, Graphics
  Thomas Andrae, Sound Rudi Stember. La firma nascosta nell'eseguibile Amiga
  ("I hate C, but worse, AmigaOS and BCPL - Thomas B.") resta senza un
  autore certo.
- Strumenti: `--program NOME` esegue un altro programma del disco 1 (con
  `MADTV_ARGS` per gli argomenti); `cargo run --release --example show_iff
  -- u3.lbm out.png` decodifica un'immagine IFF.

## Strada verso le evidenziazioni (dal 2 novembre 2026)

Obiettivo di Pam: rendere più intuitive le interazioni di trascinamento
(pannelli presi da liste e rilasciati nel palinsesto, nella valigetta della
Movie Agency, nella Library, sulla bacheca dell'ufficio, nelle news) con
evidenziazioni ed effetti. Il meccanismo del gioco funziona bene: manca il
riscontro visivo. Poi: riscrittura graduale delle funzioni in Rust, risoluzione
più alta e grafica sostitutiva.

Pezzi già pronti:

1. **Registrazione delle partite** (`--record FILE`): mouse (movimenti
   effettivamente inviati al gioco), tasti, orologio di sistema e modalità
   della CPU, frame per frame. Il file è uno script: `madtv --headless --script
   @FILE --frames N` riproduce la partita identica. Verificato: due
   riproduzioni danno la stessa memoria byte per byte. Nel tratto provato il
   gioco non legge mai l'orologio (DateStamp), ma lo registriamo comunque.
2. **Strato di sovrapposizione** (`overlay.rs`): forme in coordinate del gioco
   disegnate dopo l'ingrandimento, con trasparenza, nitide alla risoluzione
   della finestra. `enhance.rs` produrrà le evidenziazioni a ogni frame
   leggendo lo stato del gioco. `--debug-overlay` mostra il mirino sulla
   punta del puntatore (allineamento verificato al pixel, vedi
   `docs/img/overlay-mirino.png`) e i limiti del puntatore;
   `cargo run --release --example overlay_demo -- out.png` compone offline un
   frame con l'overlay.

3. **Stato del trascinamento** (dalle registrazioni di Pam del 2 novembre,
   in `scripts/registrazioni/`: palinsesto, studio, Movie Agency, agenzia
   sceneggiature, news). Tutte le stanze usano la stessa routine di aggancio
   `$030D5A(larghezza, altezza, x_presa, y_presa, testo, tipo)` e di sgancio
   `$031016`. Variabili nel segmento dati (base `$044610`):
   `$0448EC` immagine in mano (0 = niente), `$0448F0/F2` dimensioni,
   `$0448F4/F6` distanza puntatore − angolo, `$044656/58` posizione disegnata
   (−1 prima del primo disegno), `$0471C8` modalità del puntatore (2 =
   trascinamento), `$04E962` zona sotto il punto caldo (0x0F = lista del
   palinsesto), `$04AA60` tabella del palinsesto (giocatore × 0x5A, una word
   per ora: codice del programma). Il pannello del palinsesto è 144×16, la
   cassetta della Movie Agency 16×29, le notizie 144×34.
   **Il gioco decide dove si rilascia con l'angolo in alto a sinistra
   dell'oggetto**, non con la punta del puntatore (`$0179BC`/`$0179FC`):
   ecco perché sembra servire precisione. Nel palinsesto la riga è
   `(y − 4) / 16` (`$006724`), colonna programmi x 56..215, pubblicità
   216..279; il disegno della casella aggiornata arriva circa 50 frame dopo il
   click, ma la tabella cambia subito.
4. **Click di prova** (`enhance::probe_drop`): invece di riscrivere le regole
   di ogni stanza (ora corrente, blocchi occupati, valigetta...), si clona la
   macchina (`Machine::clone`, ~2 ms: RAM, chip, sistema, CPU via serde), si
   preme il tasto sinistro sulla copia e si guarda se entro 6 frame l'oggetto
   viene sganciato. Copia con file system in sola lettura. Verificato: una
   copia evolve identica all'originale (0 byte diversi dopo 1000 frame) e il
   click di prova dà "accettato" subito prima di tutti i rilasci reali delle
   registrazioni. Costo medio 3–8 ms, al massimo ogni 3 frame e solo quando il
   punto caldo si sposta.
5. **Prime evidenziazioni** (attive di default, `--originale` o F9 le
   spengono): contorno dell'oggetto trascinato e quadratino sul punto caldo,
   verdi se lì il rilascio sarebbe accettato, rossi se no; nel palinsesto
   anche la casella che riceverà il programma. Vedi
   `docs/img/evidenziazione-palinsesto.png` e
   `docs/img/evidenziazione-movie-agency.png`.

   Aggiunta dopo il primo riscontro di Pam: **punto d'arrivo fuori dal
   palinsesto**. Nelle news (e dove il gioco ridisegna l'oggetto uguale) la
   casella d'arrivo si ricava con un click di prova lungo: sulla copia si
   rilascia e si aspetta fino a 70 frame (il gioco ridisegna dopo ~40) cercando
   i pixel del pannello nello schermo (`find_panel`, ≥ 95% dei campioni). Costa
   ~85 ms, quindi gira in un thread (`LandingWorker`) e parte solo quando il
   punto caldo resta fermo 5 frame su un punto accettato; l'area resta
   evidenziata finché il punto caldo ci sta dentro. Riempimento e contorno
   dell'area d'arrivo saltano il rettangolo dell'oggetto trascinato (forma
   `Mask` dello strato), che così resta sopra l'evidenziazione. Lo strato è uno solo, sopra l'immagine Amiga già composta:
   il gioco disegna il pannello nei suoi bitplane, non esistono livelli
   separati.

Strumenti di analisi (in `examples/`): `drag_lab` (cerca variabili correlate
al puntatore e bandiere durante il trascinamento), `probe` (valori nel tempo,
dump, `watch=` per le istruzioni che leggono/scrivono un indirizzo,
`break=` per chiamate con lo stack), `drop_lab` (click di prova con
schermate e differenze di RAM), `clone_check`, `highlight_demo` (frame alla
risoluzione della finestra con le evidenziazioni). Il bus ha un punto di
osservazione (`Bus::watch`) e la macchina punti di sonda (`probe_pcs`).

Prossimi passi:

6. Riscontro di Pam sulle evidenziazioni; poi effetti (ombra del pannello,
   area di destinazione anche fuori dal palinsesto ricavata dalla differenza
   tra copia con e senza click, suono di conferma).
7. Agganci alle funzioni del gioco (tabella indirizzo → funzione Rust) e
   riscrittura graduale, a partire da quelle trovate qui.



Configurazione che funziona: A500, KS 1.3 rev 34.5, 1 MB Chip + 512 KB Slow,
disco 1 in DF0 e disco 2 in DF1. Con Quickstart scelto prima della scansione
delle ROM, WinUAE mostrava solo uno schermo nero. L'originale mostra, prima del
gioco, il logo Rainbow Arts (`rai`) e una schermata con i credits (`v u3.lbm`),
che il porting per ora salta. Schermata del palinsesto e menu muto coincidono.

## Prossimi passi

0. Evidenziazioni del trascinamento: vedi "Strada verso le evidenziazioni".
1. Conferma all'ascolto (Pam, su Windows).
2. Verificare in WinUAE: righe colorate ai bordi durante il logo Rainbow
   Arts; velocità della tendina dei credits.
3. Pulizia: limiti noti del renderer (priorità sprite, scroll fine) se
   qualche schermata ne ha bisogno.
4. Fase successiva, quando si vorrà: riscrittura in Rust delle funzioni del
   gioco, una alla volta, verificandole contro l'originale.

## Decisioni prese

- Approccio: emulazione ad alto livello (HLE) del sistema operativo + chipset
  minimo; riscrittura in Rust delle funzioni del gioco solo in seguito,
  una alla volta, verificandole contro l'originale.
- Core CPU: `m68k` 0.15 (MIT). Licenza del nostro codice: **GPL-3.0-or-later**
  (scelta anche per poter riusare, se servirà, codice di Copperline, che è GPL).
- Il repository non contiene file del gioco: gli ADF vanno messi in `adf/`
  (ignorata da git).

## Domande aperte per Pam

- Lingua: questi dischi sono un misto. Interfaccia e dialoghi visti finora
  sono in inglese ("Select a player", "Welcome to MadTV!"), ma molti file di
  dati (descrizioni dei film, notizie) sono in spagnolo. Da verificare in
  WinUAE se anche l'originale si comporta così; per una versione coerente
  servirebbero gli ADF di un'altra edizione.
