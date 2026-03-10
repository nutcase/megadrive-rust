#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/scripts/release-env.sh"
megadrive_configure_release_env
SNAP_DIR="$SCRIPT_DIR/tests/opcode_profile"
MODE="verify"
STEPS="${MEGADRIVE_PROFILE_STEPS:-10000000}"

usage() {
  cat <<'USAGE'
Usage: ./verify_accuracy.sh [--update]

Modes:
  (default)        Verify against checked-in opcode profile snapshots.
  --update         Re-generate snapshots from current implementation.

Env:
  MEGADRIVE_PROFILE_STEPS=N
      Override step count used by opcode_profile (default: 10000000).

Notes:
  - If a ROM file is missing, that case is skipped.
  - This script always runs core unit/regression tests first.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ "${1:-}" == "--update" ]]; then
  MODE="update"
  shift
fi

if [[ $# -gt 0 ]]; then
  echo "error: unknown argument: $1" >&2
  usage
  exit 1
fi

mkdir -p "$SNAP_DIR"

echo "[1/2] Running core tests..."
cargo test -q -p megadrive-core

declare -a CASES=(
  "sonic3|roms/Sonic The Hedgehog 3.md|$SNAP_DIR/sonic3_10m.snapshot"
  "comix_zone|roms/Comix Zone (Japan).md|$SNAP_DIR/comix_zone_10m.snapshot"
  "dbz_buyuu|roms/Dragon Ball Z - Buyuu Retsuden (Japan).md|$SNAP_DIR/dbz_buyuu_10m.snapshot"
)

echo "[2/2] Running opcode profile snapshots ($MODE)..."
for entry in "${CASES[@]}"; do
  IFS='|' read -r name rom snap <<<"$entry"
  if [[ ! -f "$SCRIPT_DIR/$rom" ]]; then
    echo "  - [$name] skip (missing ROM: $rom)"
    continue
  fi

  if [[ "$MODE" == "update" ]]; then
    echo "  - [$name] update snapshot"
    cargo run --release -q --manifest-path "$SCRIPT_DIR/Cargo.toml" \
      -p megadrive-cli --bin opcode_profile -- \
      "$SCRIPT_DIR/$rom" --steps "$STEPS" --snapshot "$snap"
  else
    if [[ ! -f "$snap" ]]; then
      echo "error: snapshot not found for $name: $snap" >&2
      echo "hint: run ./verify_accuracy.sh --update once to create baselines." >&2
      exit 1
    fi
    echo "  - [$name] verify snapshot"
    cargo run --release -q --manifest-path "$SCRIPT_DIR/Cargo.toml" \
      -p megadrive-cli --bin opcode_profile -- \
      "$SCRIPT_DIR/$rom" --steps "$STEPS" --verify-snapshot "$snap"
  fi
done

echo "done: accuracy regression checks completed ($MODE)"
