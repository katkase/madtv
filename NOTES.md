# Work notes

*English · [Italiano](NOTES.it.md)*

This file is the project's "memory" between sessions: status, findings,
decisions and next steps. It should be updated at the end of every session.

## Current status (end of phase 3, 1 November 2026)

- **Playable in a window** (minifb, 50 Hz), with mouse and keyboard: confirmed
  by Pam on Windows with a real game (schedule, programmes on air, clock).
  Tested via script: new game, building view, the boss's dialogues, editing
  player names with the keyboard.
- The initial loading (about 25 s at real speed) is sped up automatically
  while the screen is black; holding F11 = fast forward.
- Emulation speed: about 1,000 frames/s (20 times real time).
- **Audio**: Paula emulated (4 DMA channels, reload with interrupt, manual
  mode, A500 and LED filters), output through `cpal`; in the window the sound
  card sets the frame pace. Confirmed faithful by ear by Pam.
- Windowless mode (`--headless`) remains, with action scripts, for testing and
  analysis.

### History

- Phase 1: boot up to the player selection screen.
- Phase 2: line-by-line renderer, sprites, window, mouse, keyboard, scripts.
- Phase 3: audio, original intro (Rainbow Arts logo and credits).
- Fixes after Pam's test on Windows: flickering pointer (especially at the
  edges), delay when opening the game's windows, pointer jumping over areas
  with mouse-over labels, animations too fast with the CPU at 300% (hence the
  automatic turbo).

## Disk version

The "Mad TV v2.1 (1991)(Rainbow Arts)" set, with data files in Spanish and the
interface in English (see open questions). MD5:

- disk 1 (MTV1): `77f660cbb2c6ab7d4979f09acda3ebf7`
- disk 2 (MTV2): `a59073203f3d695bd261bba2b0057447`

For WinUAE (comparisons with the original): Kickstart 1.3 rev 34.5 A500,
`kick34005.A500`, CRC32 `c4f0f55f`, with **512 KB Chip + 512 KB Slow RAM**:
the game takes about 600 KB (about 270 KB allocated dynamically, 200 KB of it
in Chip RAM, plus about 310 KB of code and data), so it does not start with
512 KB only. Measurement: the emulator's final report shows the memory in use
and the peak (`MADTV_HEAP=1` lists the largest blocks).

Working WinUAE configuration: A500, KS 1.3 rev 34.5, 1 MB Chip + 512 KB Slow,
disk 1 in DF0 and disk 2 in DF1. With Quickstart chosen before the ROM scan,
WinUAE showed only a black screen. Schedule screen, silent menu, logo and the
credits wipe match the port.

`add21` (first command of the startup-sequence) is cosmetic: it adjusts the
Workbench screen viewport and redraws it; the game does not need it.

## What the game does with the system (from the trace)

Functions called up to the player selection screen:

- exec: `AllocMem`, `FreeMem`, `Forbid`, `Permit`, `OldOpenLibrary`,
  `FindTask`, `SetSignal`
- dos: `Open`, `Close`, `Read`, `Seek`, `Lock`, `UnLock`, `Examine`,
  `Input`, `Output`, `Write`, `IsInteractive`
- intuition, graphics: **no calls**. The game programs the hardware itself.

Hardware used directly:

- it installs its own level-3 interrupt handler by writing to vector `$6C`
  (address `$0401CC` in the loaded game); if the interrupt is not COPER it
  jumps to the old vector;
- its copper list (at `$04F338`) sets up 5 **interleaved** bitplanes
  (modulo 160), 32 colours, and ends with `MOVE $8010,INTREQ`: a copper
  interrupt every frame, which increments the frame counter (`$0456E0`), reads
  the keyboard (CIA-A SDR `$BFEC01`) and the mouse (JOY0DAT);
- blitter in area mode, never (so far) in line mode;
- POTGO/POTGOR every frame (right mouse button).
- **Mouse pointer**: drawn by the game with the blitter (it is not a sprite;
  the copper list actually turns sprite DMA off with `MOVE $0020,DMACON`).
  Its position, in screen pixels, is in two words at offset `$40`/`$42` of
  the data segment (segment 2 of the seglist; `$044650` in our load). A copy
  at `+6` looks like the previous position. The window uses it to keep the
  game pointer aligned with the PC one.
- **Mouse**: 1 Amiga mouse count = 1 lores pixel (measured). How the game
  handles it (read from the code):
  - the frame interrupt (`$0401CC`) reads JOY0DAT, computes the movement of
    the frame (`$045806`/`$045808`) and **adds** it to two accumulators,
    offset `$2C2E`/`$2C30` of the data segment (`$04723E`/`$047240`):
    movements are never lost;
  - the main loop (`$015530`) empties the accumulators and moves the pointer,
    all at once or in pieces (damping, function `$0167DC`); when it is busy
    (for example while drawing a mouse-over label) it skips a few frames;
  - the pointer is clamped between four variables that change with the
    screen: offset `$2BD2`/`$2BD4` (X/Y minimum) and `$2BD6`/`$2BD8`
    (maximum; 303/189 on the main screen).
  The window (`input::pointer_step`) uses these variables: predicted
  position = pointer + accumulators, target clamped to the screen's limits. A
  first version estimated pending movements and, after 3 frames of still
  pointer, assumed them lost: when the game was busy it resent them and the
  pointer jumped (up to 30 px past the target).
  Measurement tools: `cargo run --release --example mouse_lab` and
  `CLICKS=3 SPEED=3 cargo run --release --example mouse_sweep` (sweep over
  the building's doors, with the mouse-over labels).
- **Speed**: transitions (opening rooms, screen changes) are CPU-bound,
  because after reading the files the game decompresses them. Some animations
  are instead timed by the CPU (not by the frame): with the CPU fixed at 300%
  they "fly" (reported by Pam). Solution: **automatic turbo**
  (`--cpu-speed auto`, window default): CPU at 100% normally, at 400% after
  every dos Open/Read, until the game calls again its function that reads the
  frame counter (`$0434FC`, offset `$40364` of segment 1: returns the word at
  `$0456E0`, incremented by the interrupt). Measurements from the click on
  "New game":

  | | first change | boss | building animation |
  |---|---|---|---|
  | 100% | 4.3 s | 12.2 s | 100 frames |
  | 300% fixed | 1.5 s | 4 s | sped up |
  | auto | 1.4 s | 6.4 s | 100 frames (as the original) |

  The game clock is driven by the frame interrupts: it advances 10 minutes
  every 1,000 frames at any CPU speed. In headless mode the speed stays fixed
  at 100%, to keep scripts reproducible.

## Bugs found and fixed in phase 1 (useful for the future)

1. **Prefetch queue**: the `m68k` core emulates the 68000 prefetch; after
   every PC change made from outside (trap, Supervisor...) you must call
   `cpu.invalidate_prefetch()`, otherwise the CPU executes stale words.
2. **Interrupt level**: the core clears `int_level` after servicing an
   interrupt; it must be set again on every instruction (`set_irq`), like the
   IPL line.
3. **Initial hardware state**: AmigaOS leaves DMACON = `$03F0` and
   INTENA = `$602C`; the game does not set them itself and without these
   values neither the copper nor the interrupt start.
4. **Real-time copper**: it must run line by line during emulation (not just
   for drawing) because it generates the interrupt that drives the game.

## Known limitations

- Renderer: no sprite/playfield priority (BPLCON2), no fine scrolling
  (BPLCON1), no dual playfield or HAM; register changes in the middle of a
  line apply to the whole line; hires lines are halved.
- Blitter: line mode not emulated (counted in the report; never used so far).
- Keyboard: no numeric keypad; keys without an Amiga equivalent are ignored.
- Audio: no inter-channel modulation (ADKCON) and approximate DMA slot
  timing; CIA timers not emulated (the game does not use them so far).
- Delay() skips the frames in one go.
- The window cannot be resized (fixed scale chosen at start-up).
- Height and margins: screenshots show 200 lines (320×200), the only ones the
  game uses on all the screens tested (DIWSTRT/DIWSTOP $2C-$F4). At Pam's
  request the window has a 20-pixel margin (`--margin`): black at the top and
  sides, while at the bottom it shows display lines 200-219, so the intro
  (220 lines) is complete. Window of 360×240 game pixels (1080×720 at scale
  3). `--full-height` shows the 256 PAL lines.
- Intro disabled by default (Pam's choice): enabled with `--intro`.
- Double-click start (reported by Pam: it failed silently): the current
  folder is `target\release`, so `adf` was not found and the console closed
  at once. Now `adf` is also searched next to the executable and two levels
  up; with a console of our own (`GetConsoleProcessList` = 1) errors, even
  internal ones, wait for Enter. Windows-specific code cannot be compiled in
  the sandbox: to be checked on Windows (confirmed working by Pam).

## Audio (phase 3)

- **Music**: TFMX. The replayer is `098.HES` (disk 1), the songs are
  song+samples pairs on disk 2 (094/095, 086/087...). At start-up the game
  tries `Lock("078.HES")` in the current directory and, if not found, uses
  `MTV2:078.HES`: music loads normally (the phase-1 doubt was unfounded).
- The replayer rewrites the periods of channels 0-2 every frame (50 Hz, from
  its interrupt); at the end of a note it loops the channel on a word of
  zeros.
- The game does not enable audio interrupts (INTENA = `$603C`), so Paula's
  AUD0-3 interrupts are generated but not serviced; level 4 is handled
  anyway.
- The silence on the player selection screen is faithful: confirmed in WinUAE
  (the original has no music in the initial menu either).
- Tools: `--headless --wav FILE` records the audio; the final report shows the
  channel state; `cargo run --release --example audio_check` checks the audio
  device.

## Intro (Rainbow Arts logo and credits)

- **`rai`** (Rainbow Arts logo, with the fuse burning along the frame and the
  crackling "broken glass" noise Pam noticed in WinUAE) runs in the emulator
  in a machine of its own, before the game. It makes two exec calls and then
  takes control with `TRAP #0` after pointing the trap vector at its own
  code: this is why the main loop now lets the CPU take real TRAP/F-line/
  ILLEGAL exceptions (jump to the vector) instead of stopping. It works at
  fixed addresses above `$030000` (copper list, 4 bitplanes, a 220-line
  display window: the last 20 lines do not fit in a 200-line window). It
  lasts 658 frames (13 s) and ends by itself. Red, yellow and blue lines
  appear at the edges and fade with the logo: they are pixels drawn by the
  intro itself (free memory, zeroed in our emulator), probably an intended
  effect; to be confirmed in WinUAE.
- **Credits**: `v u3.lbm`. The `v` viewer would use intuition (OpenScreen) and
  graphics; instead of emulating it, we decode the IFF ILBM image ourselves
  (`iff.rs`: 320×256, 32 colours, ByteRun1) and reproduce the behaviour of the
  original observed by Pam in WinUAE: a top-to-bottom wipe, no audio, waiting
  for a click to continue. The wipe duration (`CREDITS_WIPE_FRAMES` in
  `main.rs`, currently 1 second) is an estimate to compare with the original.
- **Official credits** (from the image): Project Manager Ralph Stock,
  Programming PC Stefan Hoffmann, Programming Christian Krämer, Graphics
  Thomas Andrae, Sound Rudi Stember. The hidden signature in the Amiga
  executable ("I hate C, but worse, AmigaOS and BCPL - Thomas B.") has no
  certain author.
- Tools: `--program NAME` runs another program from disk 1 (with `MADTV_ARGS`
  for the arguments); `cargo run --release --example show_iff -- u3.lbm
  out.png` decodes an IFF image.

## Road to the highlights (from 2 November 2026)

Pam's goal: make drag-and-drop interactions more intuitive (panels taken from
lists and dropped into the schedule, the Movie Agency briefcase, the Library,
the office bulletin board, the news) with highlights and effects. The game's
mechanism works well: what is missing is visual feedback. Then: gradual
rewrite of the game functions in Rust, higher resolution and replacement
graphics.

1. **Game recording** (`--record FILE`): mouse (the movements actually sent
   to the game), keys, system clock and CPU mode, frame by frame. The file is
   a script: `madtv --headless --script @FILE --frames N` replays the session
   identically. Verified: two replays give the same memory byte for byte. In
   the stretch tested the game never reads the clock (DateStamp), but we
   record it anyway.
2. **Overlay layer** (`overlay.rs`): shapes in game coordinates drawn after
   magnification, with transparency, sharp at window resolution.
   `enhance.rs` produces the highlights every frame by reading the game
   state. `--debug-overlay` shows a crosshair on the pointer tip (alignment
   verified to the pixel, see `docs/img/overlay-mirino.png`) and the pointer
   limits; `cargo run --release --example overlay_demo -- out.png` composes a
   frame with the overlay offline.
3. **Drag state** (from Pam's recordings of 2 November, in
   `scripts/registrazioni/`: schedule, studio, Movie Agency, screenplay
   agency, news). All rooms use the same pick-up routine
   `$030D5A(width, height, grab_x, grab_y, text, type)` and release routine
   `$031016`. Variables in the data segment (base `$044610`):
   `$0448EC` image being held (0 = nothing), `$0448F0/F2` size,
   `$0448F4/F6` distance pointer − corner, `$044656/58` drawn position
   (−1 before the first draw), `$0471C8` pointer mode (2 = dragging),
   `$04E962` zone under the hot spot (0x0F = schedule list), `$04AA60`
   schedule table (player × 0x5A, one word per hour: programme code). The
   schedule panel is 144×16, the Movie Agency videotape 16×29, news items
   144×34.
   **The game decides where you drop with the top-left corner of the
   object**, not with the pointer tip (`$0179BC`/`$0179FC`): that is why it
   seems to need precision. In the schedule the row is `(y − 4) / 16`
   (`$006724`), programme column x 56..215, advertising 216..279; the drawing
   of the updated slot arrives about 50 frames after the click, but the table
   changes at once.
4. **Trial click** (`enhance::probe_drop`): instead of rewriting each room's
   rules (current hour, occupied blocks, briefcase...), the machine is cloned
   (`Machine::clone`, ~2 ms: RAM, chips, system, CPU via serde), the left
   button is pressed on the copy, and we check whether the object is released
   within 6 frames. The copy has a read-only file system. Verified: a copy
   evolves identically to the original (0 bytes different after 1,000 frames)
   and the trial click answers "accepted" right before every real drop in the
   recordings. Average cost 3–8 ms, at most every 3 frames and only when the
   hot spot moves.
5. **First highlights** (only in development mode, `--dev`; F9 toggles them.
   By default the window shows only the game: Pam's request): outline of the
   dragged object and a small square on the hot spot, green if a drop there
   would be accepted, red if not; in the schedule also the slot that will
   receive the programme. See `docs/img/evidenziazione-palinsesto.png` and
   `docs/img/evidenziazione-movie-agency.png`.

   Added after Pam's first feedback: **landing spot outside the schedule**. In
   the news (and wherever the game redraws the object unchanged) the landing
   slot is found with a long trial click: on the copy the object is released
   and we wait up to 70 frames (the game redraws after ~40) looking for the
   panel's pixels on screen (`find_panel`, ≥ 95% of samples). It costs
   ~85 ms, so it runs in a thread (`LandingWorker`) and starts only when the
   hot spot stays still for 5 frames on an accepted point; the area stays
   highlighted as long as the hot spot is inside it. Fill and outline of the
   landing area skip the rectangle of the dragged object (`Mask` shape of the
   layer), which therefore stays on top of the highlight. There is a single
   layer, on top of the already composed Amiga picture: the game draws the
   panel into its own bitplanes, there are no separate layers.

Analysis tools (in `examples/`): `drag_lab` (finds variables correlated with
the pointer and flags during a drag), `probe` (values over time, dumps,
`watch=` for the instructions reading/writing an address, `break=` for calls
with the stack), `drop_lab` (trial click with screenshots and RAM
differences), `clone_check`, `landing_check`, `highlight_demo` (frames at
window resolution with the highlights). The bus has a watchpoint
(`Bus::watch`) and the machine has probe points (`probe_pcs`).

Open issue: with the highlights on, Pam saw the game's own graphics flicker
under an animation (the book). Cause not yet verified; the most likely
hypothesis is irregular frame pacing, because the short trial click runs in
the window loop (3–9 ms, sometimes more). Possible remedy: move it to the
thread as well. Work paused on 2 November at Pam's request.

Next steps:

6. Pam's feedback on the highlights; then effects (panel shadow, target area
   outside the schedule as well, derived from the difference between the copy
   with and without the click, confirmation sound).
7. Hooks into the game functions (address → Rust function table) and gradual
   rewrite, starting from the ones found here.

## Next steps

0. Drag highlights: see "Road to the highlights".
1. ~~Listening check~~: done, Pam confirms the music is faithful to the Amiga.
2. Check in WinUAE: coloured lines at the edges during the Rainbow Arts logo;
   speed of the credits wipe.
3. Clean-up: known renderer limitations (sprite priority, fine scrolling) if
   some screen needs them.
4. Next phase, when wanted: rewrite of the game functions in Rust, one at a
   time, checking them against the original.

## Decisions taken

- Approach: high-level emulation (HLE) of the operating system + minimal
  chipset; rewrite of the game functions in Rust only later, one at a time,
  checking them against the original.
- CPU core: `m68k` 0.15 (MIT). Licence of our code: **GPL-3.0-or-later**
  (chosen also to be able to reuse, if needed, code from Copperline, which is
  GPL).
- The repository contains no game files: the ADFs go into `adf/` (ignored by
  git).
- Documentation: English by default, Italian versions alongside
  (`README.it.md`, `NOTES.it.md`, `docs/it/`). Code comments and CLI help
  remain in Italian for now.

## Open questions for Pam

- Language: these disks are a mix. Interface and dialogues seen so far are in
  English ("Select a player", "Welcome to MadTV!"), but many data files (film
  descriptions, news) are in Spanish. To be checked in WinUAE whether the
  original behaves the same; a consistent version would need the ADFs of
  another edition.
