# Graphics encyclopedia

*English · [Italiano](../it/grafica/README.md)*

How the original Amiga Mad TV stores, loads and draws its graphics, with the
memory addresses and routines involved. Everything here was verified on the
"Mad TV v2.1 (1991)" disks by running the game in the emulator and reading
its code; see [catalogue.md](catalogue.md) for the list of all graphics files.

The extracted pictures are **not** in the repository (the artwork belongs to
its rights holders). Generate them from your own disks:

```
cargo run --release --example extract_assets
```

This writes `assets/` (ignored by git): one folder per file with every image
as a PNG with transparency, a contact sheet `NNN.png` per file and
`index.md`. The catalogue's thumbnails point there, so they show up once you
have run the extractor (in a local Markdown viewer, not on GitHub).

## At a glance

| | |
|---|---|
| Graphics files | 147 distinct `NNN.HES` files (151 counting the 4 present on both disks) |
| Format | 1-byte image count, a width/height pair per image, then compressed bitplanes |
| Decompressor | game routine `$042542`, called by the loaders `$013114` / `$013192` |
| In memory | a table of `{width in bytes, height, pointer}` entries followed by the image data |
| Bitplanes | 5 (32 colours), interleaved line by line; optional 1-bit mask plane |
| Palette | one global palette of 32 colours, file `MTV1:001.HES` |
| Screen | 320×200 lores, 5 interleaved bitplanes (modulo 160) |

## Files on disk

Both disks hold numbered files `NNN.HES` (disk 2 has the bulk). There are no
extensions or headers telling the type; the number is what the game asks for
(`$043080`, see below). Graphics files are recognised by their structure:

```
offset  size     content
0       1        n = number of images
1       2·n      for each image: width in pixels (byte), height in rows (byte)
1+2n    ...      compressed data (see "Compression")
```

The 50 files that begin with `01 A0 6D` are the most common case: one image
of 160×109 pixels, half of a room picture.

Examples: `006.HES` starts with `1E` (30 images) followed by `08 08`, `08
08`, ..., `10 0D`, `11 18`... (8×8, 16×13, 17×24...); `051.HES` starts with
`01 A0 6D` (one image of 160×109).

## Loading

| address | role |
|---|---|
| `$043080` | `load(number, buffer)`: builds the name `NNN.HES` from the number and reads the whole file with dos `Lock`, `Open` and `Read` |
| `$041CD2` | computes the decompressed size from the header: for each image `ceil(w/4) × h × 5`, or `× 6` with the mask, plus `8·n + 2` bytes of header. It overestimates the width (`w/4` instead of `w/8`), so the buffer is larger than needed |
| `$013482` | asks for the sizes (via `$041CD2`); if the file is missing it shows the "insert disk" request |
| `$013114` | **load a graphics file without mask**: sizes, allocate the packed buffer and the output buffer (chip memory, cleared), `load`, `unpack(packed, out, 0)`, free the packed buffer, return the output |
| `$013192` | the same with the mask (`unpack(packed, out, 1)`): used for everything drawn over a background (sprites, icons, animations) |
| `$0403B2` / `$04044C` | the game's allocator and its `free`: they keep a table of allocations (`$04051C`, pairs address/size) searched linearly |

Observed in Pam's recordings: at start-up the game loads `003 002 006 011 015
004 007 013 016 161 152 017–021 009 010 150 022`; each room loads its two
halves (N+50 left, N right, without mask) and its animation set (with mask),
for example the screenplay agency `053 103 165 154 185`.

## Compression

`$042542 unpack(packed, out, mask)` (C calling convention of Aztec C: three
arguments on the stack, the last a word). After the size table it reads a
16-bit and a 32-bit value from the stream (stored at `$045FF6` and
`$045FF0`), writes `n` into the first word of the output and decodes with the
helper `$0422DA`, which allocates two work buffers of 1,082 bytes and frees
them at the end. The algorithm itself has not been rewritten yet: the
extractor runs the original routine inside the emulator, so its output is
byte-for-byte what the game has in memory. A Rust reimplementation can be
checked against it file by file.

## Layout in memory

The output of `unpack` is:

```
word        n
n × 8 bytes for each image: width in bytes (word), height (word), pointer to the data (long)
...         image data
```

The pointers are absolute addresses inside the same buffer. This
`{width in bytes, height, pointer}` structure is the game's image descriptor
everywhere: the dragged object (`$0448EC`, see NOTES.md "Road to the
highlights") points to one.

Data of one image, `wb` bytes per line, `h` lines:

- **without mask**: `h` groups of 5 lines, one per bitplane (interleaved
  bitmap, like the screen): line `y` of plane `p` is at `(y·5 + p)·wb`;
- **with mask**: first the mask, one plane of `wb·h` bytes (1 = opaque), then
  the 5 interleaved planes as above, starting at `wb·h`.

The colour index of a pixel is the 5 bits taken from the planes (plane 0 =
bit 0). Widths are rounded up to whole bytes; the real width is in the file
header.

## Palette

`MTV1:001.HES` is 64 bytes: 32 colours in the Amiga 12-bit format `0x0RGB`.
It is the only palette found among the files, and decoding every image with
it reproduces the colours on screen (rooms, panel, characters). Colour 0 is
black and in practice the background. The TV programme pictures (`220–237`)
are drawn with the greys of the palette, so they look black and white on
purpose.

## How rooms are built

A room is 320×109 pixels made of **two halves of 160×109**: file `N+50` is the
left half, file `N` the right half (e.g. screenplay agency `103` + `053`,
player's office `104` + `054`, boss's office `105` + `055`, Movie Agency
`106` + `056`, news studio `110` + `060`, archive `116` + `066`). The moving
parts (the seller, the man at the desk, the briefcase, buttons) come from
separate files with masks, drawn over the halves.

## Drawing

| address | role |
|---|---|
| `$040E12` | copies a screen rectangle (save/restore the background under panels, labels, detail windows). With even addresses and sizes it uses the blitter (A→D copy); otherwise the CPU copies byte by byte, which is slow. **Rewritten in Rust** in this port (`src/hooks.rs`), identical result |
| `$041A9C` | fills a rectangle with a colour, byte by byte per bitplane (backgrounds of detail windows) |
| `$0413xx`, `$041Exx` | text drawing and other drawing routines used by the detail windows; candidates for the next rewrites |
| `$030D5A` / `$031016` | pick up / release an object on the mouse pointer (drag and drop); background saved in three buffers `$046236/3A/3E` |

The screen itself is drawn by the game's copper list (`$04F338`): 5
interleaved bitplanes with modulo 160, 32 colours, a copper interrupt every
frame.

## Other files on the disks (first look)

Not graphics, recorded here for completeness; formats not yet studied.

| files | what they seem to be |
|---|---|
| `001` | the palette (above) |
| `078`–`095`, `148` | TFMX music: song, with the samples in the following file |
| `098` | the TFMX replayer (an AmigaOS executable) |
| `042`/`246`, `045`/`250`, `047`/`251`, `049`/`248`, `207`/`252` | text tables beginning with an offset list, in pairs with the same structure: probably the same texts in two languages (the disks mix English and Spanish) |
| `199`, `253` | compressed text (start with `25 01`) |
| `210`–`213`, `219` | saved games (39,470 bytes each, same header) |
| `041`, `044`, `046`, `048`, `164`, `201`–`206`, `208`, `209`, `247`, `249`, `337` | game data tables (`209` contains names, e.g. "Nathan") |

## Tools

- `examples/extract_assets.rs`: the extractor (runs `unpack` in the emulator
  for every file, keeps those whose result matches the header).
- `examples/asset_survey.rs`: list of all files with size and first bytes.
- `examples/unpack_lab.rs`: decompress one file and print the image table.
- `examples/probe.rs` with `break=$013114 break=$013192`: which files the game
  loads during a recording, and when.
