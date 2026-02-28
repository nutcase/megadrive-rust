# Repository Guidelines

## Project Structure & Module Organization
This repository is a Rust workspace for a Mega Drive/Genesis emulator.

- `crates/core`: emulator core (CPU, Z80, VDP, memory map, audio, input). Most logic and unit tests live here.
- `crates/cli`: SDL-based frontend binaries (`megadrive-cli`, `opcode_profile`) for running ROMs, profiling opcodes, and debugging frame/audio behavior.
- `roms/`: local ROM samples used for manual verification.
- `run.sh`: release launcher for `megadrive-cli`.
- `dump_frame.sh`: diagnostic script to dump frames/logs via `opcode_profile`.

Keep new emulator logic in `crates/core/src/*`; keep UI/tooling in `crates/cli/src/*`.

## Build, Test, and Development Commands
- `cargo test -q`: run all workspace tests.
- `cargo test -q -p megadrive-core`: run core-only tests (fast feedback for CPU/VDP/audio changes).
- `cargo run -p megadrive-cli -- <rom>`: run emulator in debug mode.
- `./run.sh <rom>`: run emulator in release mode (recommended for gameplay checks).
- `cargo run --release -p megadrive-cli --bin opcode_profile -- <rom> <steps>`: profile unknown opcodes, exceptions, and audio activity.

## Coding Style & Naming Conventions
- Follow Rust defaults: 4-space indentation, `snake_case` for functions/variables, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Prefer small, focused `exec_*` handlers for instruction implementations.
- Keep behavior-specific helpers near related instruction code.
- Use `cargo fmt` style (avoid manual formatting drift).

## Testing Guidelines
- Add unit tests in the same file under `#[cfg(test)]` modules.
- Test both decode and behavior: register/memory results, flags, PC/stack effects, exception vectors, and unknown-opcode counters.
- For emulator regressions, validate with `opcode_profile` against at least one ROM.

## Commit & Pull Request Guidelines
- Current history uses Conventional Commit style (`feat: ...`). Continue with `feat:`, `fix:`, `test:`, `refactor:`.
- Keep commits scoped to one subsystem (e.g., CPU, VDP, Z80).
- PRs should include:
  - What changed and why
  - Commands run (`cargo test`, profiling command)
  - Before/after evidence for behavior fixes (log snippets or frame dumps)

## Debugging & Safety Tips
- Prefer non-destructive diagnostics (`opcode_profile`, frame dumps) before broad refactors.
- Do not commit ROM binaries or machine-specific temporary outputs.
