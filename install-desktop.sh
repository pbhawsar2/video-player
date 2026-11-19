#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_SRC="${ROOT_DIR}/target/release/video-player"
PREFIX="${PREFIX:-${HOME}/.local}"
BIN_DIR="${PREFIX}/bin"
BIN_DST="${BIN_DIR}/video-player"
ICON_DIR="${PREFIX}/share/icons/hicolor/scalable/apps"
APP_DIR="${PREFIX}/share/applications"

for arg in "$@"; do
  case "${arg}" in
    -h|--help)
      echo "Usage: ./install-desktop.sh"
      echo "Installs standalone release binary, icon, and validated desktop integration."
      exit 0
      ;;
    *) ;;
  esac
done

if [[ ! -x "${BIN_SRC}" ]]; then
  echo "[video-player] Error: Release binary not found at ${BIN_SRC}. Run ./build.sh first." >&2
  exit 1
fi

mkdir -p "${BIN_DIR}" "${ICON_DIR}" "${APP_DIR}"

# Industrial-grade installation: atomic standalone binary copy (no fragile symlinks)
install -p -m 755 "${BIN_SRC}" "${BIN_DST}"
install -p -m 644 "${ROOT_DIR}/assets/io.github.videoplayer.App.svg" "${ICON_DIR}/io.github.videoplayer.App.svg"

# Substitute portable Exec path into the installed .desktop file so GNOME finds it even if ~/.local/bin is not in PATH
sed "s|^Exec=.*|Exec=${BIN_DST} %U|" "${ROOT_DIR}/assets/io.github.videoplayer.App.desktop" > "${APP_DIR}/io.github.videoplayer.App.desktop"
chmod 644 "${APP_DIR}/io.github.videoplayer.App.desktop"

if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "${APP_DIR}/io.github.videoplayer.App.desktop"
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "${APP_DIR}" >/dev/null 2>&1 || true
fi
if command -v gtk4-update-icon-cache >/dev/null 2>&1; then
  gtk4-update-icon-cache -f -t "${PREFIX}/share/icons/hicolor" >/dev/null 2>&1 || true
elif command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "${PREFIX}/share/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "[video-player] Installed launcher, MIME handler & icon to ${PREFIX}:"
echo "  Binary : ${BIN_DST}"
echo "  Icon   : ${ICON_DIR}/io.github.videoplayer.App.svg"
echo "  Desktop: ${APP_DIR}/io.github.videoplayer.App.desktop"
