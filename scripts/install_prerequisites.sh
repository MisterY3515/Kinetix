#!/bin/bash
# Installs the prerequisites needed to build Kinetix from source (macOS and
# Linux), so cargo build / build_macos.sh / build_linux.sh don't fail on a
# missing toolchain. Safe to re-run -- every step checks first and skips if
# already satisfied.
set -e

echo "=== Kinetix build prerequisites ==="
echo

OS="$(uname -s)"

echo "[1/2] Rust toolchain..."
if command -v rustc &> /dev/null; then
    echo "Rust already installed, skipping."
else
    echo "Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
fi

if [ "$OS" = "Darwin" ]; then
    echo
    echo "[2/2] Xcode Command Line Tools (cc, lipo, pkgbuild, osacompile...)..."
    if xcode-select -p &> /dev/null; then
        echo "Xcode Command Line Tools already installed, skipping."
    else
        echo "Installing Xcode Command Line Tools (this opens a system dialog -- click Install)..."
        xcode-select --install
        echo "Re-run this script after the Xcode Command Line Tools install finishes."
        exit 0
    fi
    echo
    echo "For building Linux artifacts too (scripts/build_linux.sh), you'll also need Docker:"
    echo "  https://www.docker.com/products/docker-desktop/"
elif [ "$OS" = "Linux" ]; then
    echo
    echo "[2/2] System libraries for the GUI installer (GTK3, ALSA, X11/Wayland, GL)..."
    if command -v apt-get &> /dev/null; then
        sudo apt-get update -qq
        sudo apt-get install -y pkg-config libasound2-dev libgtk-3-dev libxcb1-dev \
            libxkbcommon-dev libgl1-mesa-dev libxrandr-dev libxi-dev libxcursor-dev
    else
        echo "No apt-get found -- install the equivalent of these packages manually with your"
        echo "distro's package manager: pkg-config, ALSA dev headers, GTK3 dev headers,"
        echo "libxcb, libxkbcommon, libgl1/mesa, libxrandr, libxi, libxcursor (dev packages)."
    fi
    echo
    if command -v docker &> /dev/null; then
        echo "Docker already installed."
    else
        echo "Docker not found -- needed by scripts/build_linux.sh (cross-builds both"
        echo "architectures via containers). Install it from:"
        echo "  https://docs.docker.com/engine/install/"
    fi
else
    echo "Unrecognized OS '$OS' -- install Rust manually and check README.md for the rest."
fi

echo
echo "Optional: for the native LLVM backend (--features llvm), you also need"
echo "LLVM 21 -- see README.md. Not required for the default build."
echo
echo "=== Done ==="
