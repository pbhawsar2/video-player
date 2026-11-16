#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE="video-player-builder:fc45"
LEGACY_IMAGE="ytzero-builder:fc45"
CARGO_CACHE_DIR="${HOME}/.cache/video-player-cargo"

if [[ "${1:-}" == "--native" ]]; then
  echo "[video-player] Building release binary natively with host cargo..."
  (cd "${ROOT_DIR}" && cargo build --release)
  echo "[video-player] Build complete: ${ROOT_DIR}/target/release/video-player"
  ls -lh "${ROOT_DIR}/target/release/video-player"
  exit 0
fi

if ! command -v podman >/dev/null 2>&1; then
  echo "[video-player] Error: podman not found. Install podman or run './build.sh --native'." >&2
  exit 1
fi

if ! podman image exists "${IMAGE}"; then
  if podman image exists "${LEGACY_IMAGE}"; then
    IMAGE="${LEGACY_IMAGE}"
  else
    echo "[video-player] Builder image not found; building ${IMAGE} from Containerfile..."
    podman build -t "${IMAGE}" -f "${ROOT_DIR}/Containerfile" "${ROOT_DIR}"
  fi
fi

mkdir -p "${CARGO_CACHE_DIR}"

echo "[video-player] Building release binary inside isolated Fedora 45 container (${IMAGE})..."
podman run --rm \
  --userns=keep-id \
  -v "${ROOT_DIR}:/workspace:Z" \
  -v "${CARGO_CACHE_DIR}:/cargo-cache:Z" \
  -e CARGO_HOME=/cargo-cache \
  "${IMAGE}" \
  cargo build --release

echo "[video-player] Build complete: ${ROOT_DIR}/target/release/video-player"
ls -lh "${ROOT_DIR}/target/release/video-player"
