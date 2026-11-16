#!/usr/bin/env bash
set -euo pipefail

BIN_DST="${HOME}/.local/bin/video-player"
ICON_DST="${HOME}/.local/share/icons/hicolor/scalable/apps/io.github.videoplayer.App.svg"
APP_DST="${HOME}/.local/share/applications/io.github.videoplayer.App.desktop"

rm -f "${BIN_DST}" "${ICON_DST}" "${APP_DST}"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "${HOME}/.local/share/applications" >/dev/null 2>&1 || true
fi
if command -v gtk4-update-icon-cache >/dev/null 2>&1; then
  gtk4-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" >/dev/null 2>&1 || true
elif command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "[video-player] Uninstalled user-local binary, icon, and desktop entry."
