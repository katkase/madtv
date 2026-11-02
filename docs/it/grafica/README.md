# Enciclopedia della grafica

*[English](../../graphics/README.md) · Italiano*

Come Mad TV per Amiga conserva, carica e disegna la sua grafica, con gli
indirizzi di memoria e le routine coinvolte. Tutto è stato verificato sui
dischi "Mad TV v2.1 (1991)" facendo girare il gioco nell'emulatore e leggendo
il suo codice; l'elenco di tutti i file di grafica è in
[catalogo.md](catalogo.md).

Le immagini estratte **non** sono nel repository (la grafica appartiene ai
titolari dei diritti). Si generano dai propri dischi:

```
cargo run --release --example extract_assets
```

Il comando crea `assets/` (esclusa da git): una cartella per file con ogni
immagine in PNG con trasparenza, un foglio `NNN.png` per file e `index.md`.
Le miniature del catalogo puntano lì, quindi si vedono dopo aver eseguito
l'estrattore (in un visualizzatore Markdown locale, non su GitHub).

## In breve

| | |
|---|---|
| File di grafica | 147 file `NNN.HES` distinti (151 contando i 4 presenti su entrambi i dischi) |
| Formato | numero di immagini (1 byte), una coppia larghezza/altezza per immagine, poi i bitplane compressi |
| Decompressore | routine del gioco `$042542`, chiamata dai caricatori `$013114` / `$013192` |
| In memoria | una tabella di voci `{larghezza in byte, altezza, puntatore}` seguita dai dati delle immagini |
| Bitplane | 5 (32 colori), interleaved riga per riga; piano di maschera da 1 bit facoltativo |
| Tavolozza | una sola tavolozza globale di 32 colori, file `MTV1:001.HES` |
| Schermo | 320×200 lores, 5 bitplane interleaved (modulo 160) |

## I file sui dischi

Entrambi i dischi contengono file numerati `NNN.HES` (la maggior parte sul
disco 2). Non ci sono estensioni né intestazioni che dicano il tipo: il numero
è quello che il gioco chiede (`$043080`, vedi sotto). I file di grafica si
riconoscono dalla struttura:

```
offset  dimensione  contenuto
0       1           n = numero di immagini
1       2·n         per ogni immagine: larghezza in pixel (byte), altezza in righe (byte)
1+2n    ...         dati compressi (vedi "Compressione")
```

I 50 file che iniziano con `01 A0 6D` sono il caso più comune: una sola
immagine di 160×109 pixel, metà dell'immagine di una stanza.

Esempi: `006.HES` inizia con `1E` (30 immagini) seguito da `08 08`, `08 08`,
..., `10 0D`, `11 18`... (8×8, 16×13, 17×24...); `051.HES` inizia con
`01 A0 6D` (un'immagine di 160×109).

## Caricamento

| indirizzo | ruolo |
|---|---|
| `$043080` | `load(numero, buffer)`: costruisce il nome `NNN.HES` dal numero e legge tutto il file con `Lock`, `Open` e `Read` del dos |
| `$041CD2` | calcola la dimensione decompressa dall'intestazione: per ogni immagine `ceil(w/4) × h × 5`, oppure `× 6` con la maschera, più `8·n + 2` byte di intestazione. Sovrastima la larghezza (`w/4` invece di `w/8`), quindi il buffer è più grande del necessario |
| `$013482` | chiede le dimensioni (tramite `$041CD2`); se il file manca mostra la richiesta di inserire il disco |
| `$013114` | **carica un file di grafica senza maschera**: dimensioni, alloca il buffer compresso e quello d'uscita (memoria chip, azzerata), `load`, `unpack(compresso, uscita, 0)`, libera il buffer compresso, restituisce l'uscita |
| `$013192` | lo stesso con la maschera (`unpack(compresso, uscita, 1)`): per tutto ciò che si disegna sopra uno sfondo (sprite, icone, animazioni) |
| `$0403B2` / `$04044C` | l'allocatore del gioco e il suo `free`: tengono una tabella delle allocazioni (`$04051C`, coppie indirizzo/dimensione) cercata in modo lineare |

Osservato nelle registrazioni di Pam: all'avvio il gioco carica `003 002 006
011 015 004 007 013 016 161 152 017–021 009 010 150 022`; ogni stanza carica
le sue due metà (N+50 sinistra, N destra, senza maschera) e il suo gruppo di
animazioni (con maschera), per esempio l'agenzia delle sceneggiature `053 103
165 154 185`.

## Compressione

`$042542 unpack(compresso, uscita, maschera)` (convenzione di chiamata C di
Aztec C: tre argomenti sulla pila, l'ultimo una word). Dopo la tabella delle
dimensioni legge dal flusso un valore a 16 bit e uno a 32 bit (salvati in
`$045FF6` e `$045FF0`), scrive `n` nella prima word dell'uscita e decodifica
con la routine ausiliaria `$0422DA`, che alloca due buffer di lavoro da 1.082
byte e li libera alla fine. L'algoritmo non è ancora stato riscritto:
l'estrattore esegue la routine originale dentro l'emulatore, quindi il suo
risultato è byte per byte quello che il gioco ha in memoria. Una versione in
Rust si potrà verificare contro di essa file per file.

## Disposizione in memoria

L'uscita di `unpack` è:

```
word        n
n × 8 byte  per ogni immagine: larghezza in byte (word), altezza (word), puntatore ai dati (long)
...         dati delle immagini
```

I puntatori sono indirizzi assoluti dentro lo stesso buffer. La struttura
`{larghezza in byte, altezza, puntatore}` è il descrittore d'immagine del
gioco ovunque: l'oggetto trascinato (`$0448EC`, vedi NOTES.it.md "Strada
verso le evidenziazioni") punta a uno di questi.

Dati di un'immagine, `wb` byte per riga, `h` righe:

- **senza maschera**: `h` gruppi di 5 righe, una per bitplane (bitmap
  interleaved, come lo schermo): la riga `y` del piano `p` è a `(y·5 + p)·wb`;
- **con maschera**: prima la maschera, un piano di `wb·h` byte (1 = opaco),
  poi i 5 piani interleaved come sopra, a partire da `wb·h`.

L'indice di colore di un pixel sono i 5 bit presi dai piani (piano 0 = bit 0).
Le larghezze sono arrotondate a byte interi; quella vera è nell'intestazione
del file.

## Tavolozza

`MTV1:001.HES` è di 64 byte: 32 colori nel formato Amiga a 12 bit `0x0RGB`. È
l'unica tavolozza trovata tra i file, e decodificando ogni immagine con essa
si ottengono i colori dello schermo (stanze, pannello, personaggi). Il colore
0 è il nero e in pratica lo sfondo. Le immagini dei programmi TV (`220–237`)
usano i grigi della tavolozza, quindi sono in bianco e nero di proposito.

## Come sono fatte le stanze

Una stanza è di 320×109 pixel ed è composta da **due metà di 160×109**: il
file `N+50` è la metà sinistra, il file `N` la metà destra (per esempio
agenzia delle sceneggiature `103` + `053`, ufficio del giocatore `104` + `054`,
ufficio del capo `105` + `055`, Movie Agency `106` + `056`, studio delle news
`110` + `060`, archivio `116` + `066`). Le parti in movimento (il venditore,
l'uomo alla scrivania, la valigetta, i pulsanti) vengono da file separati con
maschera, disegnati sopra le metà.

## Disegno

| indirizzo | ruolo |
|---|---|
| `$040E12` | copia un rettangolo di schermo (salva e ripristina lo sfondo sotto pannelli, etichette, finestre di dettaglio). Con indirizzi e dimensioni pari usa il blitter (copia A→D); altrimenti la CPU copia un byte alla volta, ed è lento. **Riscritta in Rust** in questo porting (`src/hooks.rs`), risultato identico |
| `$041A9C` | riempie un rettangolo con un colore, un byte alla volta per bitplane (sfondi delle finestre di dettaglio) |
| `$0413xx`, `$041Exx` | disegno del testo e altre routine usate dalle finestre di dettaglio; candidate alle prossime riscritture |
| `$030D5A` / `$031016` | aggancio / sgancio di un oggetto al puntatore (trascinamento); sfondo salvato in tre buffer `$046236/3A/3E` |

Lo schermo lo disegna la copper list del gioco (`$04F338`): 5 bitplane
interleaved con modulo 160, 32 colori, un interrupt del copper a ogni frame.

## Altri file sui dischi (prima occhiata)

Non sono grafica; li annotiamo per completezza, formati ancora da studiare.

| file | cosa sembrano |
|---|---|
| `001` | la tavolozza (sopra) |
| `078`–`095`, `148` | musica TFMX: canzone, con i campioni nel file successivo |
| `098` | il replayer TFMX (un eseguibile AmigaOS) |
| `042`/`246`, `045`/`250`, `047`/`251`, `049`/`248`, `207`/`252` | tabelle di testi che iniziano con un elenco di offset, a coppie con la stessa struttura: probabilmente gli stessi testi in due lingue (i dischi mescolano inglese e spagnolo) |
| `199`, `253` | testo compresso (iniziano con `25 01`) |
| `210`–`213`, `219` | partite salvate (39.470 byte ciascuna, stessa intestazione) |
| `041`, `044`, `046`, `048`, `164`, `201`–`206`, `208`, `209`, `247`, `249`, `337` | tabelle di dati del gioco (`209` contiene nomi, per esempio "Nathan") |

## Strumenti

- `examples/extract_assets.rs`: l'estrattore (esegue `unpack` nell'emulatore
  per ogni file, tiene quelli il cui risultato corrisponde all'intestazione).
- `examples/asset_survey.rs`: elenco di tutti i file con dimensione e primi byte.
- `examples/unpack_lab.rs`: decomprime un file e stampa la tabella delle immagini.
- `examples/probe.rs` con `break=$013114 break=$013192`: quali file il gioco
  carica durante una registrazione, e quando.
