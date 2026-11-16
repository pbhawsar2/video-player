#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

BUILD_ARG=""
SET_DEFAULT=false
for arg in "$@"; do
  case "${arg}" in
    --native) BUILD_ARG="--native" ;;
    --default) SET_DEFAULT=true ;;
    -h|--help)
      echo "Usage: ./install.sh [--native] [--default]"
      echo "  --native   Build using host cargo instead of the rootless Fedora 45 container"
      echo "  --default  Register Video Player as default GNOME handler for common video MIME types"
      exit 0
      ;;
    *)
      echo "Unknown option: ${arg}" >&2
      exit 1
      ;;
  esac
done

"${ROOT_DIR}/build.sh" ${BUILD_ARG}
"${ROOT_DIR}/install-desktop.sh"

if [[ "${SET_DEFAULT}" == "true" ]] && command -v xdg-mime >/dev/null 2>&1; then
  echo "[video-player] Setting Video Player as default handler for common video formats..."
  for mime in video/mp4 video/x-matroska video/webm video/quicktime video/x-msvideo video/mpeg; do
    xdg-mime default io.github.videoplayer.App.desktop "${mime}" || true
  done
fi

echo "[video-player] Done! Launch 'Video Player' from GNOME or double-click any video file."
