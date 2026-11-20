#!/usr/bin/env bash
set -euo pipefail

PREFIX="${PREFIX:-${HOME}/.local}"
BIN_DST="${PREFIX}/bin/video-player"
ICON_DST="${PREFIX}/share/icons/hicolor/scalable/apps/io.github.videoplayer.App.svg"
APP_DST="${PREFIX}/share/applications/io.github.videoplayer.App.desktop"

XDG_DATA_HOME="${XDG_DATA_HOME:-${HOME}/.local/share}"
XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-${HOME}/.config}"
XDG_CACHE_HOME="${XDG_CACHE_HOME:-${HOME}/.cache}"

PURGE_DATA=false
for arg in "$@"; do
  case "${arg}" in
    --purge|--all-data)
      PURGE_DATA=true
      ;;
    -h|--help)
      echo "Usage: ./uninstall.sh [--purge]"
      echo "Uninstalls binary, icon, desktop entry, and restores MIME handlers."
      echo "  --purge, --all-data  Also delete user configuration, GStreamer cache, and application data"
      exit 0
      ;;
    *)
      echo "Unknown option: ${arg}" >&2
      exit 1
      ;;
  esac
done

# Remove binaries and desktop integration
rm -f "${BIN_DST}" "${ICON_DST}" "${APP_DST}"

# Clean MIME associations from user's mimeapps.list (preserving any other handlers on the same line)
for mime_file in "${XDG_CONFIG_HOME}/mimeapps.list" "${XDG_DATA_HOME}/applications/mimeapps.list"; do
  if [[ -f "${mime_file}" ]]; then
    sed -i -E \
      -e 's/io\.github\.videoplayer\.App\.desktop;//g' \
      -e 's/;io\.github\.videoplayer\.App\.desktop//g' \
      -e 's/=io\.github\.videoplayer\.App\.desktop$/=/g' \
      -e '/^[^=[]+=\s*$/d' \
      "${mime_file}" 2>/dev/null || true
  fi
done

# Clean up application data, config, and cache if requested
if [[ "${PURGE_DATA}" == "true" ]]; then
  rm -rf "${XDG_CACHE_HOME}/video-player"
  rm -rf "${XDG_CONFIG_HOME}/video-player"
  rm -rf "${PREFIX}/share/video-player"
  echo "[video-player] Removed user cache, config, and application data."
fi

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "${PREFIX}/share/applications" >/dev/null 2>&1 || true
fi
if command -v gtk4-update-icon-cache >/dev/null 2>&1; then
  gtk4-update-icon-cache -f -t "${PREFIX}/share/icons/hicolor" >/dev/null 2>&1 || true
elif command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "${PREFIX}/share/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "[video-player] Uninstalled binary, icon, and desktop entry from ${PREFIX}."
