#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/scripts/release-env.sh"
megadrive_configure_release_env

usage() {
  cat <<'USAGE'
Usage: ./dump_frame.sh <path-to-rom.bin> [options]

Options:
  --steps N            Number of 68k steps to execute before dumping (default: 10000000)
  --step-range A:B:C   Dump multiple step points from A to B (inclusive) by C
  --stop-frame N       Stop once VDP frame counter reaches N (or when --steps is hit)
  --out PATH           Output image path (.png or .ppm). Default: /tmp/megadrive_frame.png
  --hold-start         Keep START button pressed during simulation
  --hold-a             Keep A button pressed during simulation
  --disable-sprites    Force sprite rendering off (diagnostics)
  --disable-plane-a    Force plane A rendering off (diagnostics)
  --disable-plane-b    Force plane B rendering off (diagnostics)
  --force-window-off   Force window plane off (diagnostics)
  --dump-sprite-state  Print SAT/link/attr snapshot (diagnostics)
  --dump-line-state    Print per-line VDP latched state (diagnostics)
  --dump-plane-state   Print plane nametable snapshot (diagnostics)
  --dump-plane-rows A:B
                       Plane row range for --dump-plane-state (e.g. 20:55)
  --dump-plane-cols N  Plane columns printed per row (default: 3)
  --sat-live           Read SAT from live VRAM (diagnostics)
  --sat-line-latch     Read SAT from line-latched VRAM (diagnostics)
  --sat-per-line       Evaluate SAT per scanline latch (diagnostics)
  --sprite-pattern-line0
                       Read sprite pattern bytes from line-0 latch (diagnostics)
  --sprite-swap-size   Swap sprite size width/height decode (diagnostics)
  --sprite-row-major   Use row-major sprite tile order (diagnostics)
  --disable-sprite-mask
                       Disable X=0 sprite mask behavior (diagnostics)
  --control-behind-hi-plane
                       Apply control sprites behind hi-priority plane pixels
  --control-no-occupy  Control sprites do not occupy sprite priority slot
  --control-require-plane-opaque
                       Apply control sprites only on opaque plane pixels
  --plane-live-vram    Use live VRAM for plane fetch (diagnostics)
  --plane-paged        Use paged (32x32-block) scroll-plane name layout (diagnostics)
  --plane-paged-xmajor Use paged layout with x-major page order (diagnostics)
  --disable-64x32-paged
                       Disable default 64x32-page layout for large planes (diagnostics)
  --ignore-plane-priority
                       Compose planes without priority bit comparison (diagnostics)
  --vdp-byte-immediate
                       Commit VDP byte writes immediately instead of low-byte pair commit (diagnostics)
  --line-latch-next    Latch line state to line+1 at scanline boundary (diagnostics)
  --vscroll-swap-ab    Swap A/B VSRAM scroll source indices (diagnostics)
  --dump-frames LIST   Dump multiple frames (e.g. "850,851,860-865")
  --dump-prefix PATH   Prefix for --dump-frames outputs (without _NNNNNN.ppm)
  --dma-trace-limit N  Number of DMA trace entries to print (default: 8)
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
STEP_RANGE=""
STOP_FRAME=""
OUT_PATH="/tmp/megadrive_frame.png"
HOLD_START=0
HOLD_A=0
DISABLE_SPRITES=0
DISABLE_PLANE_A=0
DISABLE_PLANE_B=0
FORCE_WINDOW_OFF=0
DUMP_SPRITE_STATE=0
DUMP_LINE_STATE=0
DUMP_PLANE_STATE=0
DUMP_PLANE_ROWS=""
DUMP_PLANE_COLS=""
SAT_LIVE=0
SAT_LINE_LATCH=0
SAT_PER_LINE=0
SPRITE_PATTERN_LINE0=0
SPRITE_SWAP_SIZE=0
SPRITE_ROW_MAJOR=0
DISABLE_SPRITE_MASK=0
CONTROL_BEHIND_HIPLANE=0
CONTROL_NO_OCCUPY=0
CONTROL_REQUIRE_PLANE_OPAQUE=0
PLANE_LIVE_VRAM=0
PLANE_PAGED=0
PLANE_PAGED_XMAJOR=0
DISABLE_64X32_PAGED=0
IGNORE_PLANE_PRIORITY=0
VDP_BYTE_IMMEDIATE=0
LINE_LATCH_NEXT=0
VSCROLL_SWAP_AB=0
DUMP_FRAMES=""
DUMP_PREFIX=""
DMA_TRACE_LIMIT=""
INPUT_SCRIPT=""
PRESET=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --steps)
      shift
      [[ $# -gt 0 ]] || { echo "error: --steps requires a value" >&2; exit 1; }
      STEPS="$1"
      ;;
    --step-range)
      shift
      [[ $# -gt 0 ]] || { echo "error: --step-range requires a value" >&2; exit 1; }
      STEP_RANGE="$1"
      ;;
    --stop-frame)
      shift
      [[ $# -gt 0 ]] || { echo "error: --stop-frame requires a value" >&2; exit 1; }
      STOP_FRAME="$1"
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
    --disable-plane-a)
      DISABLE_PLANE_A=1
      ;;
    --disable-plane-b)
      DISABLE_PLANE_B=1
      ;;
    --force-window-off)
      FORCE_WINDOW_OFF=1
      ;;
    --dump-sprite-state)
      DUMP_SPRITE_STATE=1
      ;;
    --dump-line-state)
      DUMP_LINE_STATE=1
      ;;
    --dump-plane-state)
      DUMP_PLANE_STATE=1
      ;;
    --dump-plane-rows)
      shift
      [[ $# -gt 0 ]] || { echo "error: --dump-plane-rows requires a value" >&2; exit 1; }
      DUMP_PLANE_ROWS="$1"
      ;;
    --dump-plane-cols)
      shift
      [[ $# -gt 0 ]] || { echo "error: --dump-plane-cols requires a value" >&2; exit 1; }
      DUMP_PLANE_COLS="$1"
      ;;
    --sat-live)
      SAT_LIVE=1
      ;;
    --sat-line-latch)
      SAT_LINE_LATCH=1
      ;;
    --sat-per-line)
      SAT_PER_LINE=1
      ;;
    --sprite-pattern-line0)
      SPRITE_PATTERN_LINE0=1
      ;;
    --sprite-swap-size)
      SPRITE_SWAP_SIZE=1
      ;;
    --sprite-row-major)
      SPRITE_ROW_MAJOR=1
      ;;
    --disable-sprite-mask)
      DISABLE_SPRITE_MASK=1
      ;;
    --control-behind-hi-plane)
      CONTROL_BEHIND_HIPLANE=1
      ;;
    --control-no-occupy)
      CONTROL_NO_OCCUPY=1
      ;;
    --control-require-plane-opaque)
      CONTROL_REQUIRE_PLANE_OPAQUE=1
      ;;
    --plane-live-vram)
      PLANE_LIVE_VRAM=1
      ;;
    --plane-paged)
      PLANE_PAGED=1
      ;;
    --plane-paged-xmajor)
      PLANE_PAGED_XMAJOR=1
      ;;
    --disable-64x32-paged)
      DISABLE_64X32_PAGED=1
      ;;
    --ignore-plane-priority)
      IGNORE_PLANE_PRIORITY=1
      ;;
    --vdp-byte-immediate)
      VDP_BYTE_IMMEDIATE=1
      ;;
    --line-latch-next)
      LINE_LATCH_NEXT=1
      ;;
    --vscroll-swap-ab)
      VSCROLL_SWAP_AB=1
      ;;
    --dump-frames)
      shift
      [[ $# -gt 0 ]] || { echo "error: --dump-frames requires a value" >&2; exit 1; }
      DUMP_FRAMES="$1"
      ;;
    --dump-prefix)
      shift
      [[ $# -gt 0 ]] || { echo "error: --dump-prefix requires a value" >&2; exit 1; }
      DUMP_PREFIX="$1"
      ;;
    --dma-trace-limit)
      shift
      [[ $# -gt 0 ]] || { echo "error: --dma-trace-limit requires a value" >&2; exit 1; }
      DMA_TRACE_LIMIT="$1"
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
      # Sonic 3 title -> data select (no immediate stage start).
      # Format: frame,player,button,state.
      INPUT_SCRIPT="${INPUT_SCRIPT:+$INPUT_SCRIPT;}"\
"520,P1,START,1;526,P1,START,0"
      ;;
    *)
      echo "error: unknown preset: $PRESET" >&2
      exit 1
      ;;
  esac
fi

run_dump() {
  local run_steps="$1"
  local run_out_path="$2"
  local run_out_ext run_out_ext_lower run_ppm_path run_log_path run_done_path
  local run_dump_prefix
  local env_vars

  mkdir -p "$(dirname -- "$run_out_path")"

  run_out_ext="${run_out_path##*.}"
  run_out_ext_lower="$(printf '%s' "$run_out_ext" | tr '[:upper:]' '[:lower:]')"
  run_ppm_path="$run_out_path"
  if [[ "$run_out_ext_lower" == "png" ]]; then
    run_ppm_path="${run_out_path%.*}.ppm"
  fi
  run_log_path="${run_out_path%.*}.log"
  run_done_path="$run_out_path"

  echo "ROM   : $ROM_PATH"
  echo "steps : $run_steps"
  if [[ -n "$STOP_FRAME" ]]; then
    echo "stop  : frame $STOP_FRAME"
  fi
  echo "image : $run_out_path"
  echo "log   : $run_log_path"

  env_vars=()
  env_vars+=("DUMP_FRAME=$run_ppm_path")
  if [[ "$HOLD_START" == "1" ]]; then
    env_vars+=("HOLD_START=1")
  fi
  if [[ "$HOLD_A" == "1" ]]; then
    env_vars+=("HOLD_A=1")
  fi
  if [[ "$DISABLE_SPRITES" == "1" ]]; then
    env_vars+=("DISABLE_SPRITES=1")
  fi
  if [[ "$DISABLE_PLANE_A" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_DISABLE_PLANE_A=1")
  fi
  if [[ "$DISABLE_PLANE_B" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_DISABLE_PLANE_B=1")
  fi
  if [[ "$FORCE_WINDOW_OFF" == "1" ]]; then
    env_vars+=("FORCE_WINDOW_OFF=1")
  fi
  if [[ "$DUMP_SPRITE_STATE" == "1" ]]; then
    env_vars+=("DUMP_SPRITE_STATE=1")
  fi
  if [[ "$DUMP_LINE_STATE" == "1" ]]; then
    env_vars+=("DUMP_LINE_STATE=1")
  fi
  if [[ "$DUMP_PLANE_STATE" == "1" ]]; then
    env_vars+=("DUMP_PLANE_STATE=1")
  fi
  if [[ -n "$DUMP_PLANE_ROWS" ]]; then
    env_vars+=("DUMP_PLANE_ROW_RANGE=$DUMP_PLANE_ROWS")
  fi
  if [[ -n "$DUMP_PLANE_COLS" ]]; then
    env_vars+=("DUMP_PLANE_ROW_COLS=$DUMP_PLANE_COLS")
  fi
  if [[ "$SAT_LIVE" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_SAT_LIVE=1")
  fi
  if [[ "$SAT_LINE_LATCH" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_SAT_LINE_LATCH=1")
  fi
  if [[ "$SAT_PER_LINE" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_SAT_PER_LINE=1")
  fi
  if [[ "$SPRITE_PATTERN_LINE0" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_SPRITE_PATTERN_LINE0=1")
  fi
  if [[ "$SPRITE_SWAP_SIZE" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_SPRITE_SWAP_SIZE=1")
  fi
  if [[ "$SPRITE_ROW_MAJOR" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_SPRITE_ROW_MAJOR=1")
  fi
  if [[ "$DISABLE_SPRITE_MASK" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_DISABLE_SPRITE_MASK=1")
  fi
  if [[ "$CONTROL_BEHIND_HIPLANE" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_CONTROL_BEHIND_HIPLANE=1")
  fi
  if [[ "$CONTROL_NO_OCCUPY" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_CONTROL_NO_OCCUPY=1")
  fi
  if [[ "$CONTROL_REQUIRE_PLANE_OPAQUE" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_CONTROL_REQUIRE_PLANE_OPAQUE=1")
  fi
  if [[ "$PLANE_LIVE_VRAM" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_PLANE_LIVE_VRAM=1")
  fi
  if [[ "$PLANE_PAGED" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_PLANE_PAGED=1")
  fi
  if [[ "$PLANE_PAGED_XMAJOR" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_PLANE_PAGED_XMAJOR=1")
  fi
  if [[ "$DISABLE_64X32_PAGED" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_DISABLE_64X32_PAGED=1")
  fi
  if [[ "$IGNORE_PLANE_PRIORITY" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_IGNORE_PLANE_PRIORITY=1")
  fi
  if [[ "$VDP_BYTE_IMMEDIATE" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_VDP_BYTE_IMMEDIATE=1")
  fi
  if [[ "$LINE_LATCH_NEXT" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_LINE_LATCH_NEXT=1")
  fi
  if [[ "$VSCROLL_SWAP_AB" == "1" ]]; then
    env_vars+=("MEGADRIVE_DEBUG_VSCROLL_SWAP_AB=1")
  fi
  if [[ -n "$DUMP_FRAMES" ]]; then
    env_vars+=("DUMP_FRAMES=$DUMP_FRAMES")
    run_dump_prefix="$DUMP_PREFIX"
    if [[ -z "$run_dump_prefix" ]]; then
      run_dump_prefix="${run_out_path%.*}_seq"
    fi
    env_vars+=("DUMP_FRAME_PREFIX=$run_dump_prefix")
  fi
  if [[ -n "$DMA_TRACE_LIMIT" ]]; then
    env_vars+=("DMA_TRACE_LIMIT=$DMA_TRACE_LIMIT")
  fi
  if [[ -n "$STOP_FRAME" ]]; then
    env_vars+=("STOP_FRAME=$STOP_FRAME")
  fi
  if [[ -n "$INPUT_SCRIPT" ]]; then
    env_vars+=("INPUT_SCRIPT=$INPUT_SCRIPT")
  fi

  (
    cd "$SCRIPT_DIR"
    env "${env_vars[@]}" cargo run --release -p megadrive-cli --bin opcode_profile -- "$ROM_PATH" "$run_steps"
  ) | tee "$run_log_path"

  if [[ "$run_out_ext_lower" == "png" ]]; then
    if command -v sips >/dev/null 2>&1; then
      sips -s format png "$run_ppm_path" --out "$run_out_path" >/dev/null
      rm -f "$run_ppm_path"
    else
      echo "warning: sips not found, keeping ppm at $run_ppm_path" >&2
      run_done_path="$run_ppm_path"
    fi
  fi

  if [[ -n "$DUMP_FRAMES" ]]; then
    if command -v sips >/dev/null 2>&1; then
      shopt -s nullglob
      seq_ppm=("${run_dump_prefix}"_*.ppm)
      shopt -u nullglob
      for ppm in "${seq_ppm[@]}"; do
        png="${ppm%.ppm}.png"
        sips -s format png "$ppm" --out "$png" >/dev/null
        rm -f "$ppm"
      done
    else
      echo "warning: sips not found, keeping sequence dumps as ppm" >&2
    fi
  fi

  echo "done: $run_done_path"
}

if [[ -n "$STEP_RANGE" ]]; then
  if [[ -n "$DUMP_FRAMES" ]]; then
    echo "error: --step-range cannot be combined with --dump-frames" >&2
    exit 1
  fi
  IFS=':' read -r STEP_START STEP_END STEP_DELTA <<< "$STEP_RANGE"
  if [[ -z "$STEP_START" || -z "$STEP_END" || -z "$STEP_DELTA" ]]; then
    echo "error: --step-range format must be A:B:C" >&2
    exit 1
  fi
  if [[ ! "$STEP_START" =~ ^[0-9]+$ || ! "$STEP_END" =~ ^[0-9]+$ || ! "$STEP_DELTA" =~ ^[0-9]+$ ]]; then
    echo "error: --step-range values must be non-negative integers" >&2
    exit 1
  fi
  if (( STEP_DELTA == 0 )); then
    echo "error: --step-range delta must be > 0" >&2
    exit 1
  fi
  if (( STEP_START > STEP_END )); then
    echo "error: --step-range start must be <= end" >&2
    exit 1
  fi
  range_prefix="${DUMP_PREFIX:-${OUT_PATH%.*}}"
  range_ext="${OUT_PATH##*.}"
  range_ext_lower="$(printf '%s' "$range_ext" | tr '[:upper:]' '[:lower:]')"
  if [[ "$range_ext_lower" != "png" && "$range_ext_lower" != "ppm" ]]; then
    range_ext_lower="png"
  fi
  for ((step=STEP_START; step<=STEP_END; step+=STEP_DELTA)); do
    run_dump "$step" "${range_prefix}_${step}.${range_ext_lower}"
  done
else
  run_dump "$STEPS" "$OUT_PATH"
fi
