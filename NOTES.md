# Note di lavoro

Questo file è la "memoria" del progetto tra una sessione e l'altra: stato,
scoperte, decisioni e prossimi passi. Va aggiornato alla fine di ogni sessione.

## Stato attuale (fine fase 2, 1 novembre 2026)

- **Giocabile in una finestra** (minifb, 50 Hz, 960×600 di default), con mouse
  e tastiera: confermato da Pam su Windows con una partita vera (palinsesto,
  programmi in onda, orologio). Provato via script: nuova partita, vista dell'edificio, dialoghi
  del capo, modifica dei nomi dei giocatori con la tastiera.
- Il caricamento iniziale (circa 25 s a velocità reale) viene accelerato in
  automatico finché lo schermo è nero; F11 tenuto premuto = avanti veloce.
- Velocità dell'emulazione: circa 1.000 frame/s (20 volte il tempo reale).
- **Manca l'audio**: il gioco accende i canali 0 e 1 (DMACON = `$03D3`)
  durante la partita, quindi ci sono già effetti sonori da emulare.
- La modalità senza finestra (`--headless`) resta, con gli script di azioni,
  per test e analisi.

### Storia

- Fase 1: avvio fino alla schermata di selezione dei giocatori.
- Fase 2: renderer riga per riga, sprite, finestra, mouse, tastiera, script.
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
`kick34005.A500`, CRC32 `c4f0f55f`.

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
- Audio: nessuno (vedi fase 3). All'avvio il gioco prova `Lock("078.HES")`
  nella directory corrente (MTV1:), ma la musica TFMX sta sul disco 2:
  verificare in WinUAE se la versione su floppy suona davvero la musica.
- La startup-sequence originale lancia anche `rai` (intro Rainbow Arts) e
  `v u3.lbm` (visualizzatore IFF): per ora eseguiamo solo `mtv s`.
- Delay() salta i frame in un colpo solo.
- La finestra non è ridimensionabile (scala fissa scelta all'avvio).
- Altezza: la finestra e gli screenshot mostrano 200 righe (320×200), le sole
  che il gioco usa in tutte le schermate provate (DIWSTRT/DIWSTOP $2C-$F4);
  `--full-height` mostra le 256 righe PAL. Se il gioco disegnasse oltre, la
  finestra lo segnala in console.

## Prossimi passi (fase 3: audio)

1. Paula: 4 canali DMA (AUDxLC, AUDxLEN, AUDxPER, AUDxVOL, AUDxDAT),
   interrupt di fine campione (AUD0-3, livello 4), ADKCON (modulazioni, rare).
2. Uscita audio con `cpal`, ricampionamento da ~3,5 MHz / periodo a 44,1 o 48 kHz,
   filtro passa-basso, stereo Amiga (canali 0/3 a sinistra, 1/2 a destra).
3. Sincronizzazione audio/video: l'audio guida il ritmo dei frame.
4. Capire perché la musica TFMX non parte (o se sulla versione floppy manca):
   `Lock("078.HES")` fallisce nella directory corrente.
5. Facoltativo: intro `rai` e immagine `u3.lbm` prima del gioco.

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
