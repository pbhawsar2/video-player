#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_SRC="${ROOT_DIR}/target/release/video-player"

BUILD_NATIVE=false
FORCE_REBUILD=false
SET_MIME=true

for arg in "$@"; do
  case "${arg}" in
    --native) BUILD_NATIVE=true ;;
    --rebuild) FORCE_REBUILD=true ;;
    --no-mime) SET_MIME=false ;;
    -h|--help)
      echo "Usage: ./install.sh [OPTIONS]"
      echo ""
      echo "Industrial-grade standalone installer for Video Player (GTK 4 / Libadwaita / Wayland)."
      echo "Defaults to complete, turnkey installation without requiring any arguments."
      echo ""
      echo "Options:"
      echo "  --native    Build with host cargo instead of containerized Fedora toolchain"
      echo "  --rebuild   Force re-compilation even if release binary already exists"
      echo "  --no-mime   Skip registering Video Player as default handler for video formats"
      echo "  -h, --help  Show this help message and exit"
      exit 0
      ;;
    *)
      echo "Unknown option: ${arg}" >&2
      echo "Run './install.sh --help' for usage." >&2
      exit 1
      ;;
  esac
done

# Step 1: Ensure optimized release binary is built
if [[ ! -x "${BIN_SRC}" ]] || [[ "${FORCE_REBUILD}" == "true" ]]; then
  echo "[video-player] Building optimized release binary..."
  if [[ "${BUILD_NATIVE}" == "true" ]]; then
    "${ROOT_DIR}/build.sh" --native
  else
    "${ROOT_DIR}/build.sh"
  fi
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
fi

echo "[video-player] Done! Launch 'Video Player' from GNOME or double-click any video file."
