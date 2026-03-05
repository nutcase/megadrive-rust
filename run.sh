#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 [--no-egui] <path-to-rom.bin> [--boot-frames N]"
  exit 1
fi

BIN_NAME="megadrive-egui"
if [[ "${1:-}" == "--no-egui" ]]; then
  BIN_NAME="megadrive-cli"
  shift
fi

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 [--no-egui] <path-to-rom.bin> [--boot-frames N]"
  exit 1
fi

ROM_PATH="$1"
shift

if [[ ! -f "$ROM_PATH" ]]; then
  echo "error: ROM file not found: $ROM_PATH" >&2
  exit 1
fi

# Default to no boot skip. You can still override with:
#   MEGADRIVE_BOOT_FRAMES=<N> ./run.sh <rom>
#   ./run.sh <rom> --boot-frames <N>
: "${MEGADRIVE_BOOT_FRAMES:=0}"
export MEGADRIVE_BOOT_FRAMES

# Keep cheat save/load path stable regardless of current working directory.
: "${MEGADRIVE_CHEAT_DIR:=$SCRIPT_DIR/cheats}"
export MEGADRIVE_CHEAT_DIR

exec cargo run --release --manifest-path "$SCRIPT_DIR/Cargo.toml" -p megadrive-cli --bin "$BIN_NAME" -- "$ROM_PATH" "$@"
