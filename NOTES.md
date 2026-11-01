# Note di lavoro

Questo file è la "memoria" del progetto tra una sessione e l'altra: stato,
scoperte, decisioni e prossimi passi. Va aggiornato alla fine di ogni sessione.

## Stato attuale (fine fase 1, 1 novembre 2026)

- Il gioco si avvia dal disco originale, decomprime il proprio eseguibile
  (Imploder) dentro l'emulatore, stampa `***** MAD TV AMIGA VERSION 2.1 ******`,
  carica circa 90 file dai due dischi e disegna la **schermata di selezione dei
  giocatori**.
- Stabile per almeno 15.000 frame (5 minuti di gioco emulato), eseguiti in circa
  8 secondi: l'emulazione è ~40 volte più veloce del tempo reale.
- Nessuna finestra: il programma esegue N frame e salva screenshot PNG.

## Versione dei dischi

Set "Mad TV v2.1 (1991)(Rainbow Arts)", **versione spagnola** (testi in
spagnolo con qualche avanzo inglese). MD5:

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

- Renderer: niente sprite, niente scroll fine (BPLCON1), niente dual
  playfield né HAM; i cambi di registro a metà riga valgono per tutta la riga.
- Blitter: modalità linea non emulata (viene contata nel rapporto).
- Tastiera: la CIA-A SDR restituisce sempre 0.
- Audio: nessuno. Nota: all'avvio il gioco prova `Lock("078.HES")` nella
  directory corrente (MTV1:), ma la musica TFMX sta sul disco 2: verificare in
  WinUAE se la versione su floppy suona davvero la musica.
- La startup-sequence originale lancia anche `rai` (intro Rainbow Arts) e
  `v u3.lbm` (visualizzatore IFF): per ora eseguiamo solo `mtv s`.
- Delay() salta i frame in un colpo solo (il copper gira solo sull'ultima riga).

## Prossimi passi (fase 2: grafica e input)

1. Finestra con `minifb` o `pixels`+`winit`, a 50 Hz; scala 2× o 3×.
2. Mouse: dalla finestra a JOY0DAT (contatori) + tasti (CIA-A PRA bit 6,
   POTGOR bit 10).
3. Tastiera: sequenza della CIA-A (SDR + interrupt PORTS/livello 2), codici
   tasto Amiga.
4. Renderer per riga con lo stato registrato dal copper durante il frame
   (invece di rieseguire la copper list alla fine).
5. Sprite (puntatore del mouse, se il gioco li usa).
6. Confronto con WinUAE sulle stesse schermate.

## Decisioni prese

- Approccio: emulazione ad alto livello (HLE) del sistema operativo + chipset
  minimo; riscrittura in Rust delle funzioni del gioco solo in seguito,
  una alla volta, verificandole contro l'originale.
- Core CPU: `m68k` 0.15 (MIT). Licenza del nostro codice: **da decidere**
  (vedi domande aperte).
- Il repository non contiene file del gioco: gli ADF vanno messi in `adf/`
  (ignorata da git).

## Domande aperte per Pam

- Licenza del progetto (MIT? GPL? nessuna per ora?).
- Lingua: con questi dischi il gioco è in spagnolo. Servono ADF di un'altra
  lingua, se la si vuole.
