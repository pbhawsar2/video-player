#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_SRC="${ROOT_DIR}/target/release/video-player"
BIN_DST="${HOME}/.local/bin/video-player"
ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"
APP_DIR="${HOME}/.local/share/applications"

mkdir -p "${HOME}/.local/bin" "${ICON_DIR}" "${APP_DIR}"

ln -sf "${BIN_SRC}" "${BIN_DST}"
install -m 644 "${ROOT_DIR}/assets/io.github.videoplayer.App.svg" "${ICON_DIR}/io.github.videoplayer.App.svg"
install -m 644 "${ROOT_DIR}/assets/io.github.videoplayer.App.desktop" "${APP_DIR}/io.github.videoplayer.App.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "${APP_DIR}" >/dev/null 2>&1 || true
fi
if command -v gtk4-update-icon-cache >/dev/null 2>&1; then
  gtk4-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" >/dev/null 2>&1 || true
elif command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "[video-player] Installed user-local launcher, MIME handler & icon:"
echo "  Binary : ${BIN_DST} -> ${BIN_SRC}"
echo "  Icon   : ${ICON_DIR}/io.github.videoplayer.App.svg"
echo "  Desktop: ${APP_DIR}/io.github.videoplayer.App.desktop"
