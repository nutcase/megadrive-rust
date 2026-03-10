#!/usr/bin/env bash

# Configure release-build optimization knobs based on the host build environment.
#
# Default behavior:
#   - Apple Silicon macOS (aarch64-apple-darwin): optimize for the local CPU.
#   - Other hosts: keep Cargo's portable release defaults.
#
# Optional overrides:
#   MEGADRIVE_RELEASE_OPT_MODE=auto|portable|native
#     auto     : Apple Silicon gets native tuning, everything else stays portable.
#     portable : never inject host-specific tuning.
#     native   : always inject target-cpu=native on the current host.

megadrive_append_rustflag() {
  local flag="$1"
  case " ${RUSTFLAGS:-} " in
    *" ${flag} "*)
      return 0
      ;;
  esac
  if [[ -n "${RUSTFLAGS:-}" ]]; then
    export RUSTFLAGS="${RUSTFLAGS} ${flag}"
  else
    export RUSTFLAGS="$flag"
  fi
}

megadrive_detect_host_triple() {
  if command -v rustc >/dev/null 2>&1; then
    rustc -vV | awk '/^host: / { print $2; exit }'
    return 0
  fi
  printf '%s\n' "$(uname -m)-$(uname -s | tr '[:upper:]' '[:lower:]')"
}

megadrive_configure_release_env() {
  local mode host_triple
  mode="${MEGADRIVE_RELEASE_OPT_MODE:-auto}"
  host_triple="$(megadrive_detect_host_triple)"

  export MEGADRIVE_RELEASE_ENV_HOST="$host_triple"
  export MEGADRIVE_RELEASE_ENV_NAME="portable-release"

  case "$mode" in
    auto)
      if [[ "$host_triple" == "aarch64-apple-darwin" ]]; then
        megadrive_append_rustflag "-Ctarget-cpu=native"
        export CARGO_PROFILE_RELEASE_LTO="${CARGO_PROFILE_RELEASE_LTO:-thin}"
        export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-1}"
        export MEGADRIVE_RELEASE_ENV_NAME="apple-silicon-native"
      fi
      ;;
    portable)
      ;;
    native)
      megadrive_append_rustflag "-Ctarget-cpu=native"
      export CARGO_PROFILE_RELEASE_LTO="${CARGO_PROFILE_RELEASE_LTO:-thin}"
      export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-1}"
      export MEGADRIVE_RELEASE_ENV_NAME="host-native"
      ;;
    *)
      echo "error: unsupported MEGADRIVE_RELEASE_OPT_MODE: $mode" >&2
      return 1
      ;;
  esac
}

megadrive_release_env_summary() {
  local target_cpu lto codegen_units
  target_cpu="portable"
  if [[ " ${RUSTFLAGS:-} " == *" -Ctarget-cpu=native "* ]]; then
    target_cpu="native"
  fi
  lto="${CARGO_PROFILE_RELEASE_LTO:-default}"
  codegen_units="${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-default}"
  printf 'release profile: %s | host: %s | cpu: %s | lto: %s | codegen-units: %s\n' \
    "${MEGADRIVE_RELEASE_ENV_NAME:-portable-release}" \
    "${MEGADRIVE_RELEASE_ENV_HOST:-unknown}" \
    "$target_cpu" \
    "$lto" \
    "$codegen_units"
}
