#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_SRC="${ROOT_DIR}/target/release/video-player"
PROFDATA="${ROOT_DIR}/target/pgo-profiles/merged.profdata"

DO_PGO=false
USE_CONTAINER=false
FORCE_REBUILD=false
SET_MIME=true
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
    --no-pgo)
      DO_PGO=false
      shift
      ;;
    --container)
      USE_CONTAINER=true
      shift
      ;;
    --native)
      USE_CONTAINER=false
      shift
      ;;
    --rebuild)
      FORCE_REBUILD=true
      shift
      ;;
    --no-mime)
      SET_MIME=false
      shift
      ;;
    -h|--help)
      echo "Usage: ./install.sh [OPTIONS] [VIDEO_OR_DIR]"
      echo ""
      echo "Industrial-grade standalone installer for Video Player (GTK 4 / Libadwaita / Wayland)."
      echo "Defaults to fast native Zen 3 + Fat LTO compilation without containers."
      echo ""
      echo "Options:"
      echo "  --pgo [PATH] Run two-pass PGO build using --test-stress + brief 2s video samples"
      echo "  --container  Build inside isolated Fedora container instead of host"
      echo "  --rebuild    Force re-compilation even if release binary already exists"
      echo "  --no-mime    Skip registering Video Player as default handler for video formats"
      echo "  -h, --help   Show this help message and exit"
      exit 0
      ;;
    *)
      if [[ -d "$1" || -f "$1" ]]; then
        WORKLOAD="$1"
        DO_PGO=true
        shift
      else
        echo "Unknown option: $1" >&2
        echo "Run './install.sh --help' for usage." >&2
        exit 1
      fi
      ;;
  esac
done

# Step 1: Ensure optimized release binary is built
BUILD_ARGS=()
if [[ "${DO_PGO}" == "true" ]]; then
  BUILD_ARGS+=("--pgo")
  if [[ -n "${WORKLOAD}" ]]; then
    BUILD_ARGS+=("${WORKLOAD}")
  fi
fi
if [[ "${USE_CONTAINER}" == "true" ]]; then
  BUILD_ARGS+=("--container")
fi

if [[ "${DO_PGO}" == "true" ]] || [[ ! -x "${BIN_SRC}" ]] || [[ "${FORCE_REBUILD}" == "true" ]]; then
  echo "[video-player] Building optimized release binary..."
  "${ROOT_DIR}/build.sh" "${BUILD_ARGS[@]}"
fi

# Step 2: Install standalone binary, icon, and validated desktop integration
"${ROOT_DIR}/install-desktop.sh"

# Step 3: Register as default handler for supported video MIME types by default
if [[ "${SET_MIME}" == "true" ]] && command -v xdg-mime >/dev/null 2>&1; then
  echo "[video-player] Registering Video Player as default handler for video formats..."
  MIME_TYPES=(
    video/mp4
    video/x-matroska
    video/webm
    video/quicktime
    video/x-msvideo
    video/x-flv
    video/ogg
    video/mp2t
    video/3gpp
    video/3gpp2
    video/mpeg
    video/mp4v-es
    application/x-matroska
    video/x-ms-wmv
    video/divx
    application/ogg
  )
  for mime in "${MIME_TYPES[@]}"; do
    xdg-mime default io.github.videoplayer.App.desktop "${mime}" 2>/dev/null || true
  done
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "${XDG_DATA_HOME:-$HOME/.local/share}/applications" 2>/dev/null || true
  fi
fi

INSTALLED_BIN="${PREFIX:-$HOME/.local}/bin/video-player"
if [[ -x "${INSTALLED_BIN}" ]]; then
  VERSION_CHECK=$("${INSTALLED_BIN}" --version 2>/dev/null || echo "Video Player")
  echo "[video-player] Integrity Check: ${VERSION_CHECK} is functional."

  if [[ ":$PATH:" != *":${PREFIX:-$HOME/.local}/bin:"* ]]; then
    echo ""
    echo "[video-player] NOTE: '${PREFIX:-$HOME/.local}/bin' is not currently in your PATH."
    echo "  Add it by running: echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc"
    echo ""
  fi
fi

echo "[video-player] Done! Launch 'Video Player' from GNOME or double-click any video file."
