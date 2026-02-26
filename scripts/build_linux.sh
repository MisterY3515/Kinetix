#!/bin/bash
# Build script for Linux
set -e
export KINETIX_BUILD="17"

# Ensure we are running from the workspace root
cd "$(dirname "$0")/.."

echo "=== Building Kinetix for Linux ==="

# 1. Build release binaries
echo "[1/2] Compiling..."
cargo build --release --package kinetix-cli --package kinetix-kicomp

# 2. Build the GUI Installer
echo "[2/2] Building GUI Installer and Creating dist..."
# The installer uses include_bytes! to embed kivm and kicomp from target/release/
cargo build --release --package kinetix-installer

rm -rf dist
mkdir -p dist

cp target/release/installer dist/KinetixInstaller

echo "=== Done ==="
echo "Output: dist/"
echo "  KinetixInstaller"
