#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  cat <<'USAGE'
Usage: ./dump_frame.sh <path-to-rom.bin> [options]

Options:
  --steps N            Number of 68k steps to execute before dumping (default: 10000000)
  --out PATH           Output image path (.png or .ppm). Default: /tmp/megadrive_frame.png
  --hold-start         Keep START button pressed during simulation
  --hold-a             Keep A button pressed during simulation
  --disable-sprites    Force sprite rendering off (diagnostics)
  --force-window-off   Force window plane off (diagnostics)
  --input-script STR   Input events: "frame,player,button,state;..."
                       Example: "120,P1,START,1;124,P1,START,0"
  --preset NAME        Built-in input script preset (currently: data-select)
  -h, --help           Show this help

Outputs:
  - Image: PATH
  - Log:   PATH with .log extension
USAGE
}

if [[ $# -lt 1 ]]; then
  usage
  exit 1
fi

if [[ "$1" == "-h" || "$1" == "--help" ]]; then
  usage
  exit 0
fi

ROM_PATH="$1"
shift

if [[ ! -f "$ROM_PATH" ]]; then
  echo "error: ROM file not found: $ROM_PATH" >&2
  exit 1
fi

STEPS="${MEGADRIVE_DUMP_STEPS:-10000000}"
OUT_PATH="/tmp/megadrive_frame.png"
HOLD_START=0
HOLD_A=0
DISABLE_SPRITES=0
FORCE_WINDOW_OFF=0
INPUT_SCRIPT=""
PRESET=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --steps)
      shift
      [[ $# -gt 0 ]] || { echo "error: --steps requires a value" >&2; exit 1; }
      STEPS="$1"
      ;;
    --out)
      shift
      [[ $# -gt 0 ]] || { echo "error: --out requires a value" >&2; exit 1; }
      OUT_PATH="$1"
      ;;
    --hold-start)
      HOLD_START=1
      ;;
    --hold-a)
      HOLD_A=1
      ;;
    --disable-sprites)
      DISABLE_SPRITES=1
      ;;
    --force-window-off)
      FORCE_WINDOW_OFF=1
      ;;
    --input-script)
      shift
      [[ $# -gt 0 ]] || { echo "error: --input-script requires a value" >&2; exit 1; }
      INPUT_SCRIPT="$1"
      ;;
    --preset)
      shift
      [[ $# -gt 0 ]] || { echo "error: --preset requires a value" >&2; exit 1; }
      PRESET="$1"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
  shift
done

if [[ -n "$PRESET" ]]; then
  case "$PRESET" in
    data-select)
      # Pulse START several times across boot/title to land on file select.
      # Format: frame,player,button,state
      INPUT_SCRIPT="${INPUT_SCRIPT:+$INPUT_SCRIPT;}"\
"30,P1,START,1;36,P1,START,0;120,P1,START,1;126,P1,START,0;220,P1,START,1;226,P1,START,0"
      ;;
    *)
      echo "error: unknown preset: $PRESET" >&2
      exit 1
      ;;
  esac
fi

mkdir -p "$(dirname -- "$OUT_PATH")"

out_ext="${OUT_PATH##*.}"
out_ext_lower="$(printf '%s' "$out_ext" | tr '[:upper:]' '[:lower:]')"

PPM_PATH="$OUT_PATH"
if [[ "$out_ext_lower" == "png" ]]; then
  PPM_PATH="${OUT_PATH%.*}.ppm"
fi

LOG_PATH="${OUT_PATH%.*}.log"

echo "ROM   : $ROM_PATH"
echo "steps : $STEPS"
echo "image : $OUT_PATH"
echo "log   : $LOG_PATH"

env_vars=()
env_vars+=("DUMP_FRAME=$PPM_PATH")
if [[ "$HOLD_START" == "1" ]]; then
  env_vars+=("HOLD_START=1")
fi
if [[ "$HOLD_A" == "1" ]]; then
  env_vars+=("HOLD_A=1")
fi
if [[ "$DISABLE_SPRITES" == "1" ]]; then
  env_vars+=("DISABLE_SPRITES=1")
fi
if [[ "$FORCE_WINDOW_OFF" == "1" ]]; then
  env_vars+=("FORCE_WINDOW_OFF=1")
fi
if [[ -n "$INPUT_SCRIPT" ]]; then
  env_vars+=("INPUT_SCRIPT=$INPUT_SCRIPT")
fi

(
  cd "$SCRIPT_DIR"
  env "${env_vars[@]}" cargo run --release -p megadrive-cli --bin opcode_profile -- "$ROM_PATH" "$STEPS"
) | tee "$LOG_PATH"

if [[ "$out_ext_lower" == "png" ]]; then
  if command -v sips >/dev/null 2>&1; then
    sips -s format png "$PPM_PATH" --out "$OUT_PATH" >/dev/null
    rm -f "$PPM_PATH"
  else
    echo "warning: sips not found, keeping ppm at $PPM_PATH" >&2
    OUT_PATH="$PPM_PATH"
  fi
fi

echo "done: $OUT_PATH"
