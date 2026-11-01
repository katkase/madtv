# Note di lavoro

Questo file è la "memoria" del progetto tra una sessione e l'altra: stato,
scoperte, decisioni e prossimi passi. Va aggiornato alla fine di ogni sessione.

## Stato attuale (fine fase 2, 1 novembre 2026)

- **Giocabile in una finestra** (minifb, 50 Hz, scala 3 di default), con mouse
  e tastiera. Provato via script: nuova partita, vista dell'edificio, dialoghi
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
  (soprattutto ai bordi) e ritardo nell'apertura delle finestre del gioco.

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
  Il gioco applica i movimenti con 1-2 frame di ritardo e li **smorza**
  (circa metà del residuo per frame, assestamento in 6-8 frame): è un
  comportamento dell'originale. Limiti del puntatore: X 0..303, Y 0..189.
  L'inseguimento nella finestra (`input::PointerSync`) tiene conto dei
  conteggi già inviati e non ancora applicati, altrimenti li reinvia e il
  puntatore oscilla (era lo sfarfallio segnalato); il bersaglio viene limitato
  all'area del puntatore, così ai bordi non si spinge invano. Strumento di
  misura: `cargo run --release --example mouse_lab`.
- **Velocità**: le transizioni (apertura di finestre, cambi di schermata) sono
  limitate dalla CPU, non dal disco. Misure dal click su "New game":
  primo cambiamento dopo 4,3 s e capo dopo 12,2 s con la CPU al 100%;
  1,5 s e 4 s al 300%. L'orologio del gioco avanza di 10 minuti ogni 1.000
  frame a entrambe le velocità: il tempo di gioco è scandito dagli interrupt di
  frame, quindi accelerare la CPU non altera il gameplay. Default: 300% nella
  finestra (F10 per passare al 100%), 100% in headless per gli script.
- **Tastiera**: il gioco legge la CIA-A SDR nel suo interrupt e reagisce ai
  cambiamenti del valore (nessun handshake necessario). Invio conferma i
  campi di testo.

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
