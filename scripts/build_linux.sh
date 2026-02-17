#!/bin/bash
# Build script for Linux
set -e

echo "=== Building Kinetix for Linux ==="

# 1. Build release binaries
echo "[1/2] Compiling..."
cargo build --release --package kinetix-cli --package kinetix-kicomp

# 2. Create distribution
echo "[2/2] Creating dist..."
rm -rf dist
mkdir -p dist/bin

# Copy binaries
cp target/release/kivm dist/bin/kivm
cp target/release/kicomp dist/bin/kicomp

# Create convenience script/symlink
ln -sf ./bin/kivm dist/kinetix

# Optional: Create .desktop file or similar if needed for GUI apps (Installer is GUI)
# But standard distribution is CLI tools usually.
# If building the installer:
# cargo build --release --package kinetix-installer
# cp target/release/installer dist/install_kinetix

echo "=== Done ==="
echo "Output: dist/"
echo "  bin/kivm"
echo "  bin/kicomp"
