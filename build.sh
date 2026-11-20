#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE="video-player-builder:fc45"
CARGO_CACHE_DIR="${HOME}/.cache/video-player-cargo"
PROFILE_DIR="${ROOT_DIR}/target/pgo-profiles"
PROFDATA="${PROFILE_DIR}/merged.profdata"

DO_PGO=false
USE_CONTAINER=false
WORKLOAD=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --pgo)
      DO_PGO=true
      shift
      if [[ $# -gt 0 && (-d "$1" || -f "$1") ]]; then
        WORKLOAD="$1"
        shift
      fi
      ;;
    --container)
      USE_CONTAINER=true
      shift
      ;;
    --native)
      USE_CONTAINER=false
      shift
      ;;
    -h|--help)
      echo "Usage: ./build.sh [--pgo [VIDEO_OR_DIR]] [--container | --native]"
      echo ""
      echo "Fast native release builder for Video Player (Zen 3 + Fat LTO + optional PGO)."
      echo "Defaults to native host compilation; pass --container for isolated Podman build."
      exit 0
      ;;
    *)
      if [[ -d "$1" || -f "$1" ]]; then
        WORKLOAD="$1"
        shift
      else
        echo "Unknown option: $1" >&2
        exit 1
      fi
      ;;
  esac
done

check_host_deps() {
  local missing_tools=()
  local missing_pkgs=()

  for tool in cargo rustc pkg-config; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      missing_tools+=("$tool")
    fi
  done

  if [[ "${DO_PGO}" == "true" ]]; then
    if ! command -v llvm-profdata >/dev/null 2>&1 && ! command -v rust-profdata >/dev/null 2>&1; then
      missing_tools+=("llvm-profdata")
    fi
  fi

  local required_pkgs=("gtk4" "libadwaita-1" "gstreamer-1.0")
  for pkg in "${required_pkgs[@]}"; do
    if ! pkg-config --exists "$pkg" 2>/dev/null; then
      missing_pkgs+=("$pkg")
    fi
  done

  if [[ ${#missing_tools[@]} -gt 0 || ${#missing_pkgs[@]} -gt 0 ]]; then
    echo "[video-player] Error: Missing required host build dependencies!" >&2
    [[ ${#missing_tools[@]} -gt 0 ]] && echo "[video-player] Missing tools: ${missing_tools[*]}" >&2
    [[ ${#missing_pkgs[@]} -gt 0 ]] && echo "[video-player] Missing libraries: ${missing_pkgs[*]}" >&2
    echo "" >&2
    echo "Fedora: sudo dnf install rust cargo pkgconf-pkg-config gtk4-devel libadwaita-devel gstreamer1-devel llvm" >&2
    echo "Ubuntu: sudo apt install cargo rustc pkg-config libgtk-4-dev libadwaita-1-dev libgstreamer1.0-dev llvm" >&2
    echo "Arch:   sudo pacman -S rust cargo pkgconf gtk4 libadwaita gstreamer llvm" >&2
    echo "Or build in a container: ./build-container.sh" >&2
    return 1
  fi
  return 0
}

run_pgo_profiling_workload() {
  local binary="${ROOT_DIR}/target/release/video-player"
  export LLVM_PROFILE_FILE="${PROFILE_DIR}/default_%m_%p.profraw"
  export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
  local has_display=false
  if [[ (-n "${WAYLAND_DISPLAY:-}" || -n "${DISPLAY:-}") && -d "${XDG_RUNTIME_DIR}" ]]; then
    has_display=true
  fi

  local pgo_xdg
  pgo_xdg="$(mktemp -d /tmp/vp-pgo-xdg.XXXXXX)"
  mkdir -p "${pgo_xdg}/config" "${pgo_xdg}/data" "${pgo_xdg}/cache"

  echo "[video-player] [PGO Profiling] Running short representative training workload..."
  "${binary}" --probe >/dev/null 2>&1 || true

  if [[ "${has_display}" == "true" ]]; then
    echo "[video-player] (1/2) Running multi-codec & UI layout stress QA (VP9/H.264/HEVC)..."
    XDG_CONFIG_HOME="${pgo_xdg}/config" XDG_DATA_HOME="${pgo_xdg}/data" XDG_CACHE_HOME="${pgo_xdg}/cache" \
      "${binary}" --test-stress >/dev/null 2>&1 || true

    if [[ -n "${WORKLOAD}" ]]; then
      echo "[video-player] (2/2) Profiling brief 2s playback samples from: ${WORKLOAD}..."
      local count=0
      if [[ -f "${WORKLOAD}" ]]; then
        echo "[video-player]   Sampling (2s): $(basename "${WORKLOAD}")"
        XDG_CONFIG_HOME="${pgo_xdg}/config" XDG_DATA_HOME="${pgo_xdg}/data" XDG_CACHE_HOME="${pgo_xdg}/cache" \
          "${binary}" "${WORKLOAD}" --auto-quit-secs 2 >/dev/null 2>&1 || true
      elif [[ -d "${WORKLOAD}" ]]; then
        while IFS= read -r vid; do
          [[ -z "${vid}" ]] && continue
          echo "[video-player]   Sampling (2s): $(basename "${vid}")"
          XDG_CONFIG_HOME="${pgo_xdg}/config" XDG_DATA_HOME="${pgo_xdg}/data" XDG_CACHE_HOME="${pgo_xdg}/cache" \
            "${binary}" "${vid}" --auto-quit-secs 2 >/dev/null 2>&1 || true
          count=$((count + 1))
          [[ ${count} -ge 2 ]] && break
        done < <(find "${WORKLOAD}" -maxdepth 3 -type f \( -iname "*.mp4" -o -iname "*.mkv" -o -iname "*.webm" -o -iname "*.mov" \) 2>/dev/null | head -n 2)
      fi
    fi
  fi

  rm -rf "${pgo_xdg}"
}

if [[ "${USE_CONTAINER}" == "true" ]]; then
  if ! command -v podman >/dev/null 2>&1; then
    echo "[video-player] Error: podman not found. Install podman or run './build.sh' for native build." >&2
    exit 1
  fi
  if ! podman image exists "${IMAGE}"; then
    echo "[video-player] Builder image not found; building ${IMAGE} from Containerfile..."
    podman build -t "${IMAGE}" -f "${ROOT_DIR}/Containerfile" "${ROOT_DIR}"
  fi
  mkdir -p "${CARGO_CACHE_DIR}"
  echo "[video-player] Building release binary inside isolated container (${IMAGE})..."
  podman run --rm \
    --userns=keep-id \
    -v "${ROOT_DIR}:/workspace:Z" \
    -v "${CARGO_CACHE_DIR}:/cargo-cache:Z" \
    -e CARGO_HOME=/cargo-cache \
    "${IMAGE}" \
    cargo build --release
else
  check_host_deps
  if [[ "${DO_PGO}" == "true" ]]; then
    echo "[video-player] === Two-Pass Profile-Guided Optimization (PGO) Build (Native Host) ==="
    mkdir -p "${PROFILE_DIR}"
    rm -f "${PROFILE_DIR}"/*.profraw

    echo "[video-player] [PGO Pass 1/2] Compiling instrumented binary..."
    (cd "${ROOT_DIR}" && RUSTFLAGS="-C profile-generate=${PROFILE_DIR} -C target-cpu=znver3" cargo build --release)

    run_pgo_profiling_workload

    echo "[video-player] [PGO Merge] Merging execution profiles..."
    if command -v llvm-profdata >/dev/null 2>&1; then
      llvm-profdata merge -o "${PROFDATA}" "${PROFILE_DIR}"/*.profraw
    elif command -v rust-profdata >/dev/null 2>&1; then
      rust-profdata merge -o "${PROFDATA}" "${PROFILE_DIR}"/*.profraw
    fi

    echo "[video-player] [PGO Pass 2/2] Compiling final profile-optimized binary..."
    (cd "${ROOT_DIR}" && RUSTFLAGS="-C profile-use=${PROFDATA} -Cllvm-args=-pgo-warn-missing-function -C target-cpu=znver3" cargo build --release)
  elif [[ -f "${PROFDATA}" ]]; then
    echo "[video-player] Building release binary natively with cached PGO profile (${PROFDATA}) + Zen 3 tuning..."
    (cd "${ROOT_DIR}" && RUSTFLAGS="-C profile-use=${PROFDATA} -Cllvm-args=-pgo-warn-missing-function -C target-cpu=znver3" cargo build --release)
  else
    echo "[video-player] Building release binary natively with host cargo (Zen 3 + Fat LTO)..."
    (cd "${ROOT_DIR}" && cargo build --release)
  fi
fi

echo "[video-player] Build complete: ${ROOT_DIR}/target/release/video-player"
ls -lh "${ROOT_DIR}/target/release/video-player"
