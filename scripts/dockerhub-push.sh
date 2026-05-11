#!/usr/bin/env bash
# Build the repo Dockerfile and push to Docker Hub for a chosen OS/CPU.
# Typical Ubuntu VPS on x86_64 → linux/amd64 (default below).
# Apple Silicon Mac default images are linux/arm64; without --platform they
# will not run on most Ubuntu VPS unless the VPS is ARM (e.g. aarch64).

set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

: "${DOCKERHUB_USER:?Export DOCKERHUB_USER=your-dockerhub-username}"
IMAGE_NAME="${IMAGE_NAME:-localtunnel}"
TAG="${TAG:-latest}"
# Ubuntu VPS x86_64: linux/amd64. Ubuntu on ARM (e.g. Oracle Ampere): linux/arm64.
# Multi-arch (larger CI time): linux/amd64,linux/arm64
PLATFORM="${PLATFORM:-linux/amd64}"

IMAGE="${DOCKERHUB_USER}/${IMAGE_NAME}:${TAG}"

# linux/amd64 on Apple Silicon is built via QEMU; full Rust compiles often hit rustc SIGSEGV.
if [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]] && [[ "${PLATFORM}" == *"amd64"* ]]; then
  echo "Refusing to build: ${PLATFORM} on Apple Silicon uses QEMU and rustc frequently crashes." >&2
  echo "Use GitHub Actions (.github/workflows/dockerhub.yml), or build on the Ubuntu VPS, or set PLATFORM=linux/arm64 for an ARM VPS only." >&2
  exit 1
fi

docker buildx version >/dev/null

if ! docker buildx inspect rlt-cross >/dev/null 2>&1; then
  docker buildx create --name rlt-cross --driver docker-container --use
else
  docker buildx use rlt-cross
fi

docker buildx inspect --bootstrap >/dev/null

docker buildx build \
  --platform "${PLATFORM}" \
  -f Dockerfile \
  -t "${IMAGE}" \
  --push \
  .

echo "Pushed ${IMAGE} (platform=${PLATFORM})"
