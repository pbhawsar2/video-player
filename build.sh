#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE="video-player-builder:fc45"
CARGO_CACHE_DIR="${HOME}/.cache/video-player-cargo"
USE_CONTAINER=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --container)
      USE_CONTAINER=true
      shift
      ;;
    --native)
      USE_CONTAINER=false
      shift
      ;;
    -h|--help)
      echo "Usage: ./build.sh [--container | --native]"
      echo ""
      echo "Fast native release builder for Video Player (Zen 3 + Fat LTO)."
      echo "Defaults to native host compilation; pass --container for isolated Podman build."
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

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
  echo "[video-player] Building release binary natively with host cargo (Zen 3 + Fat LTO)..."
  (cd "${ROOT_DIR}" && cargo build --release)
fi

echo "[video-player] Build complete: ${ROOT_DIR}/target/release/video-player"
ls -lh "${ROOT_DIR}/target/release/video-player"
