# Architettura

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
            │ adf.rs: dischi ADF │   │ render.rs: immagine    │
            │ + salvataggi su PC │   │ (copper + bitplane)    │
            └────────────────────┘   └───────────────────────┘
```

## La CPU

Usiamo il crate `m68k` in modalità "passo singolo" (`CpuCore::step`), che
restituisce un `StepResult`. Due casi ci interessano:

- `Ok { cycles }`: istruzione eseguita; i cicli fanno avanzare il tempo
  emulato (pennello elettronico, copper, frame);
- `AlineTrap { opcode }`: il programma ha eseguito un opcode `$Axxx`. Non
  esiste sul 68000: lo usiamo come "chiamata al sistema operativo emulato".

Attenzione (vedi NOTES.md): ogni volta che cambiamo il PC dall'esterno va
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
- **mouse/joystick**: JOY0DAT, POTGOR, CIA-A PRA (tasti).

## Il renderer

`render.rs` ricostruisce l'immagine a fine frame: riesegue la copper list riga
per riga sullo stato dei registri e converte i bitplane in pixel RGB (palette a
12 bit). Gestisce lores/hires, finestra di visualizzazione (DIWSTRT/DIWSTOP),
fetch (DDFSTRT/DDFSTOP), moduli (quindi anche le bitmap interleaved del gioco)
ed Extra Half-Brite. Mancano sprite, scroll fine, dual playfield e HAM.

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
