#!/bin/bash
# Build script for Linux. Builds two separate installer binaries (x86_64 and
# arm64) using Docker, so this works from any host (including non-Linux
# hosts, and Apple Silicon/Intel Macs) without a local cross-toolchain.
# ELF has no "universal binary" equivalent, so -- unlike build_macos.sh --
# this produces one binary per architecture, not a merged one.
set -e

KINETIX_BUILD="36"

# Ensure we are running from the workspace root
cd "$(dirname "$0")/.."

if ! command -v docker &> /dev/null; then
    echo "Error: Docker is required (https://www.docker.com/) -- this script"
    echo "cross-builds both Linux architectures inside containers so it works"
    echo "the same way on any host OS."
    exit 1
fi
if ! docker info &> /dev/null; then
    echo "Error: Docker is installed but the daemon isn't running. Start Docker Desktop (or dockerd) and retry."
    exit 1
fi

OUTPUT_DIR="dist_linux"
RUST_IMAGE="rust:1-bookworm"
# System packages needed to build kinetix-installer's GUI (eframe/glow needs
# GL + X11/Wayland headers; rfd's Linux file-dialog backend needs GTK3;
# rodio's audio backend needs ALSA).
APT_PACKAGES="pkg-config libasound2-dev libgtk-3-dev libxcb1-dev libxkbcommon-dev libgl1-mesa-dev libxrandr-dev libxi-dev libxcursor-dev"

# Docker --platform name -> output label / CARGO_TARGET_DIR, as parallel
# arrays (not `declare -A`: macOS ships bash 3.2, which has no associative
# array support). Each arch gets its own target dir: reusing one across
# container archs risks cargo reusing a fingerprint-cached artifact built
# for the *other* architecture.
PLATFORMS=("linux/amd64" "linux/arm64")
LABELS=("x86_64" "aarch64")

echo "=== Building Kinetix for Linux (x86_64 + arm64, via Docker) ==="

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

for i in "${!PLATFORMS[@]}"; do
    platform="${PLATFORMS[$i]}"
    label="${LABELS[$i]}"
    target_dir="target-docker-$label"
    echo ""
    echo "--- [$label] Compiling (platform: $platform) ---"
    rm -rf "$target_dir"

    docker run --rm --platform "$platform" \
        -v "$PWD":/build -w /build \
        -e KINETIX_BUILD="$KINETIX_BUILD" \
        -e CARGO_TARGET_DIR="/build/$target_dir" \
        "$RUST_IMAGE" \
        bash -c "apt-get update -qq && apt-get install -y -qq $APT_PACKAGES > /dev/null && \
                 cargo build --release --package kinetix-cli --package kinetix-kicomp && \
                 cargo build --release --package kinetix-installer"

    echo "--- [$label] Packaging ---"
    cp "$target_dir/release/installer" "$OUTPUT_DIR/KinetixInstaller-linux-$label"
    chmod +x "$OUTPUT_DIR/KinetixInstaller-linux-$label"
    rm -rf "$target_dir"
done

echo ""
echo "=== Done ==="
echo "Output: $OUTPUT_DIR/"
for label in "${ARCHES[@]}"; do
    echo "  KinetixInstaller-linux-$label"
done
