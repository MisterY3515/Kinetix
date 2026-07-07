#!/bin/bash
# Build script for Linux. Builds installer binaries using Docker, so this
# works from any host (including non-Linux hosts, and Apple Silicon/Intel
# Macs) without a local cross-toolchain. ELF has no "universal binary"
# equivalent, so -- unlike build_macos.sh -- this produces one binary per
# architecture, not a merged one.
#
# By default this only builds for the host's own architecture (each arch is
# a full from-scratch compile inside its own container, with no cache --
# building both takes roughly twice as long). Pass "both" to produce both
# release artifacts.
#
# Usage:
#   ./scripts/build_linux.sh            # host's own architecture (fast, default)
#   ./scripts/build_linux.sh x86_64      # x86_64 only
#   ./scripts/build_linux.sh aarch64     # aarch64 only
#   ./scripts/build_linux.sh both        # both (release prep)
set -e

case "$(uname -m)" in
    x86_64|amd64) HOST_ARCH="x86_64" ;;
    arm64|aarch64) HOST_ARCH="aarch64" ;;
    *) HOST_ARCH="x86_64" ;;
esac
ARCH="${1:-$HOST_ARCH}"
if [[ "$ARCH" != "x86_64" && "$ARCH" != "aarch64" && "$ARCH" != "both" ]]; then
    echo "Error: unknown arch '$ARCH' (expected x86_64, aarch64, or both)"
    exit 1
fi

export KINETIX_BUILD="37"

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
ALL_PLATFORMS=("linux/amd64" "linux/arm64")
ALL_LABELS=("x86_64" "aarch64")
PLATFORMS=()
LABELS=()
for i in "${!ALL_LABELS[@]}"; do
    if [[ "$ARCH" == "both" || "$ARCH" == "${ALL_LABELS[$i]}" ]]; then
        PLATFORMS+=("${ALL_PLATFORMS[$i]}")
        LABELS+=("${ALL_LABELS[$i]}")
    fi
done

echo "=== Building Kinetix for Linux ($ARCH, via Docker) ==="

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
for label in "${LABELS[@]}"; do
    echo "  KinetixInstaller-linux-$label"
done
