# Analisi degli eseguibili originali

*[English](../analysis.md) · Italiano*

Risultati dell'analisi fatta prima di iniziare il porting, su due versioni:
DOS (eseguibili del 10/2/1994) e Amiga ("Mad TV v2.1 (1991)", versione spagnola).

## Sintesi

Le due versioni nascono **dagli stessi sorgenti C**. Su PC sono stati
ricompilati con Turbo C, e le parti in assembly specifiche dell'Amiga sono
state sostituite da driver esterni.

| | Amiga | DOS |
|---|---|---|
| Logica del gioco | C, **Aztec C (Manx)**, modello large, int a 16 bit | stessi sorgenti, **Turbo C 2.0**, modello large |
| Compressione dell'eseguibile | Imploder | LZEXE 0.91 |
| Grafica a basso livello | assembly per il blitter, dentro l'eseguibile | assembly in `MCV.BIN` (driver EGA a bitplane) |
| Musica | replayer TFMX in assembly (`098.HES`) | driver Palladix PSM (PC-Beeper, AdLib) in assembly |
| Dati | file `NNN.HES` | `SOURCE1.RSC`, `SOURCE2.RSC`, `DATAV.PCK` (PowerPacker) |

Prove dei sorgenti comuni: la stessa routine di stampa con `#` al posto di `%`
(`#02d:#02d`, `#_8l $`...), il messaggio tedesco `Fehler aufgetreten!`, la
tabella di caratteri `LQXWMNbOFJERD;<=>?@ABCG`.

## Versione Amiga

### Dischi

- **Disco 1 (MTV1)**: startup-sequence `add21` / `rai` / `v u3.lbm` /
  `mtv s`. `mtv` è il gioco (141.556 byte compressi).
- **Disco 2 (MTV2)**: 156 file di dati `NNN.HES`: testi, grafica in un formato
  compresso proprietario (63 file che iniziano con `01 A0 6D`), 10 moduli
  musicali TFMX (`TFMX-SONG`, con i campioni nel file successivo) e il
  replayer `098.HES`.

### L'eseguibile `mtv`

- Compresso con **Imploder**: un mini-hunk iniziale percorre la seglist e
  chiama `explode`, che decomprime all'indietro nei segmenti BSS e applica le
  rilocazioni; poi libera i segmenti del decompressore con `FreeMem` e salta al
  codice decompresso.
- Dopo la decompressione: 267.376 byte di codice + 42.072 byte di dati.
- **Codice di avvio di Aztec C**: `geta4` (`lea __H1_org+32766,a4`),
  azzeramento della BSS, `__savsp`, `_SysBase`, controllo della FPU
  (`btst #4,$129(a6)`), apertura di `dos.library` con `Alert($38007)` in caso
  di errore.
- 491 funzioni su 586 iniziano con `link a5` (C compilato); argomenti sullo
  stack, pulizia a carico del chiamante, interi a 16 bit (`move.w`).
- Circa 9 KB di **assembly scritto a mano** per il blitter (minterm `$FCA`,
  cookie-cut), con attesa `btst #6,DMACONR` e una chiamata per bitplane;
  parametri nei registri.
- Firma nascosta nei dati: *"I hate C, but worse, AmigaOS and BCPL - Thomas B."*
- Crediti ufficiali (schermata `u3.lbm`): Project Manager Ralph Stock,
  Programming PC Stefan Hoffmann, Programming Christian Krämer, Graphics Thomas
  Andrae, Sound Rudi Stember.

### Comportamento a runtime (dall'emulazione, fase 1)

Vedi NOTES.it.md: il gioco usa pochissimo il sistema operativo (solo exec e dos per
memoria e file), installa il proprio gestore di interrupt su `$6C` e si
sincronizza con un interrupt copper generato dalla propria copper list.

## Versione DOS

- `MTV.EXE`: C, **Turbo C 2.0** (stringa `Turbo-C - Copyright (c) 1988
  Borland Intl.`), modello large (tutte le funzioni far, argomenti da
  `[bp+6]`), compresso con **LZEXE 0.91**.
- Unico accesso diretto all'hardware nell'eseguibile: attesa del ritraccio
  verticale (porta `$3DA`), in un piccolo modulo assembly.
- `MCV.BIN`: driver grafico binario in assembly (11 punti d'ingresso, modo
  0Dh EGA 320×200×16, scrittura a bitplane tramite map mask).
- `SOURCE1.RSC` contiene i driver audio **Palladix PSM** (formato .COM):
  `V1.1 - PC-Beeper`, `V1.3 - AdLib (TM) soundcard`, `PALLADIX-PSMDETECT`.
- `INTRO.EXE`: Turbo C 1.5, modello small.
- `DATAV.PCK`: compresso con PowerPacker (`PP20`), formato tipico dell'Amiga.

## Strumenti usati

- Disassemblatore: Capstone (68000 e x86 a 16 bit), con disassemblaggio
  ricorsivo dai punti d'ingresso e raggruppamento per segmento.
- Decompressione: decompressore LZEXE scritto per l'occasione; per Imploder,
  esecuzione del decompressore originale in un 68000 emulato.
- Filesystem Amiga: `amitools` (xdftool), poi `src/adf.rs` in questo progetto.
