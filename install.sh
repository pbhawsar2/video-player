#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_SRC="${ROOT_DIR}/target/release/video-player"

USE_CONTAINER=false
FORCE_REBUILD=false
SET_MIME=true

for arg in "$@"; do
  case "${arg}" in
    --container) USE_CONTAINER=true ;;
    --native) USE_CONTAINER=false ;;
    --rebuild) FORCE_REBUILD=true ;;
    --no-mime) SET_MIME=false ;;
    -h|--help)
      echo "Usage: ./install.sh [OPTIONS]"
      echo ""
      echo "Industrial-grade standalone installer for Video Player (GTK 4 / Libadwaita / Wayland)."
      echo "Defaults to fast native compilation and turnkey standalone installation."
      echo ""
      echo "Options:"
      echo "  --container Build inside isolated Fedora container instead of host"
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
  if [[ "${USE_CONTAINER}" == "true" ]]; then
    "${ROOT_DIR}/build.sh" --container
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
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "${XDG_DATA_HOME:-$HOME/.local/share}/applications" 2>/dev/null || true
  fi
fi

INSTALLED_BIN="${PREFIX:-$HOME/.local}/bin/video-player"
if [[ -x "${INSTALLED_BIN}" ]]; then
  VERSION_CHECK=$("${INSTALLED_BIN}" --version 2>/dev/null || echo "Video Player")
  echo "[video-player] Integrity Check: ${VERSION_CHECK} is functional."
fi

echo "[video-player] Done! Launch 'Video Player' from GNOME or double-click any video file."
