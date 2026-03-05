# Mega Drive Emulator (Rust)

This is a Mega Drive / Genesis emulator implemented in Rust.
The core components (68000, Z80, VDP, YM2612/PSG) live in `crates/core`, and SDL/egui frontends live in `crates/cli`.

The current priority is to expand compatibility while preserving correctness, using real ROM validation plus regression tests.

## Implemented
- 68000 core (major instruction groups, exception handling, interrupts, and key addressing modes)
- Z80 core (instruction execution, BUSREQ/RESET handling, and 68k RAM bus arbitration)
- VDP (VRAM/CRAM/VSRAM, plane/sprite composition, scrolling, window, DMA, NTSC/PAL timing)
- Input (player 1/2, 3-button and 6-button controllers)
- Audio path (YM2612/PSG register model and mixed output)
- Cheat UI (egui)
  - Hex Viewer
  - Cheat Search (snapshot-based filtering)
  - Enable/disable, edit, save/load cheats as JSON
- Debug tools
  - `opcode_profile` (unknown opcode/exception/VDP DMA tracing)
  - `dump_frame.sh` (frame dumps, logs, and VDP diagnostics)

## Quick Start
Recommended (release + cheat UI enabled):
```bash
./run.sh roms/<game>.md
./run.sh roms/<game>.md --boot-frames 600
```

Classic SDL frontend (without egui):
```bash
./run.sh --no-egui roms/<game>.md
```

Direct launch:
```bash
cargo run -p megadrive-cli --bin megadrive-egui -- roms/<game>.md
cargo run -p megadrive-cli --bin megadrive-cli -- roms/<game>.md
```

Notes:
- `run.sh` starts `megadrive-egui` by default.
- Pre-boot skipping is controlled by `--boot-frames` or `MEGADRIVE_BOOT_FRAMES`.
- Controller type can be set with `MEGADRIVE_PAD1` / `MEGADRIVE_PAD2` (`3` or `6`).

## Controls
- Player 1
  - D-pad: `Up Down Left Right`
  - `A/B/C`: `A / Z / X`
  - `X/Y/Z`: `S / D / F`
  - `Start/Mode`: `Enter / Q`
- Player 2
  - D-pad: `I K J L`
  - `A/B/C`: `R / T / Y`
  - `X/Y/Z`: `U / O / P`
  - `Start/Mode`: `Right Shift / /`
- Common
  - Quit: `Esc`
  - Toggle cheat panel (egui frontend): `Tab`

## Cheat UI
- Cheat files are saved to `<workspace>/cheats/<ROM_NAME>.json` by default.
- You can override the directory with `MEGADRIVE_CHEAT_DIR=/path/to/cheats`.
- You can edit WRAM directly from the Hex Viewer.
- Cheat Search narrows candidates via snapshots and can add candidates directly as active cheats.

## Debugging Tools
Frame dump wrapper (`opcode_profile`):
```bash
./dump_frame.sh roms/<game>.md --steps 11000000 --out /tmp/frame.png
./dump_frame.sh roms/<game>.md --stop-frame 900 --dump-line-state
```

Run `opcode_profile` directly:
```bash
cargo run --release -p megadrive-cli --bin opcode_profile -- roms/<game>.md 12000000
```

## Project Layout
- `crates/core`: emulator core (CPU/Z80/VDP/audio/memory map/input)
- `crates/cli`: executable frontends (`megadrive-cli`, `megadrive-egui`, `opcode_profile`)
- `run.sh`: release launcher (defaults to egui frontend)
- `dump_frame.sh`: frame/diagnostic dump script
- `roms/`: local ROMs for manual verification
- `tests` / `crates/core/tests`: regression test suites

## Development Commands
```bash
cargo fmt
cargo test -q
cargo test -q -p megadrive-core
cargo test -q -p megadrive-cli
cargo check -q
```

## Accuracy Regression Guard
Use the profile snapshot checker before and after VDP/APU work to reduce regressions:

```bash
# Verify current output against checked-in baselines
./verify_accuracy.sh

# Re-generate baselines intentionally after validated improvements
./verify_accuracy.sh --update
```

Current baselines are stored in `tests/opcode_profile/*.snapshot`.

## Known Limitations
- Instruction/timing/bus arbitration coverage is still in progress, so game-specific rendering or audio differences may remain.
- Some VDP edge cases (special scrolling and priority conflicts) are still being refined.
- Full cycle-accurate YM2612/PSG behavior (fine envelope/mixing details) is not finished yet.

## License

This project is for educational and research purposes.
