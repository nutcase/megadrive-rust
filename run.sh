#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <path-to-rom.bin> [--boot-frames N]"
  exit 1
fi

ROM_PATH="$1"
shift

if [[ ! -f "$ROM_PATH" ]]; then
  echo "error: ROM file not found: $ROM_PATH" >&2
  exit 1
fi

: "${MEGADRIVE_BOOT_FRAMES:=600}"
export MEGADRIVE_BOOT_FRAMES

exec cargo run --release --manifest-path "$SCRIPT_DIR/Cargo.toml" -p megadrive-cli --bin megadrive-cli -- "$ROM_PATH" "$@"
