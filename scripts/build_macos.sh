#!/bin/bash
# Build script for macOS (creates .app and .pkg)
set -e

APP_NAME="Kinetix"
VERSION="0.0.1"
IDENTIFIER="com.mistery3515.kinetix"
OUTPUT_DIR="dist_mac"

echo "=== Building Kinetix for macOS ==="

# 1. Build release binaries
echo "[1/3] Compiling..."
# Build binaries (CLI tools)
cargo build --release --package kinetix-cli --package kinetix-kicomp
# Build Installer (GUI)
cargo build --release --package kinetix-installer

# 2. Create .app Bundle for Installer
echo "[2/3] Creating Installer.app..."
rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR/Kinetix Installer.app/Contents/MacOS"
mkdir -p "$OUTPUT_DIR/Kinetix Installer.app/Contents/Resources"

# Copy binary
cp target/release/installer "$OUTPUT_DIR/Kinetix Installer.app/Contents/MacOS/KinetixInstaller"
chmod +x "$OUTPUT_DIR/Kinetix Installer.app/Contents/MacOS/KinetixInstaller"

# Copy resources (data folder logic from Windows installer needs to be adapted for .app)
# The installer looks for `data/` next to executable.
# In .app, it's next to binary in MacOS/ folder.
mkdir -p "$OUTPUT_DIR/Kinetix Installer.app/Contents/MacOS/data"
cp target/release/kivm "$OUTPUT_DIR/Kinetix Installer.app/Contents/MacOS/data/"
cp target/release/kicomp "$OUTPUT_DIR/Kinetix Installer.app/Contents/MacOS/data/"

# Create Info.plist
cat > "$OUTPUT_DIR/Kinetix Installer.app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>KinetixInstaller</string>
    <key>CFBundleIdentifier</key>
    <string>$IDENTIFIER.installer</string>
    <key>CFBundleName</key>
    <string>Kinetix Installer</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
</dict>
</plist>
EOF

# 3. Create PKG (Java-style installer)
echo "[3/3] Creating .pkg..."
# This requires 'pkgbuild' (part of Xcode tools)
if command -v pkgbuild &> /dev/null; then
    pkgbuild --root "$OUTPUT_DIR/Kinetix Installer.app" \
             --identifier "$IDENTIFIER.pkg" \
             --version "$VERSION" \
             --install-location "/Applications/Kinetix Installer.app" \
             "$OUTPUT_DIR/KinetixInstaller.pkg"
    
    echo "Created $OUTPUT_DIR/KinetixInstaller.pkg"
else
    echo "Warning: pkgbuild not found. Skipping .pkg creation."
fi

echo "=== Done ==="
echo "Output: $OUTPUT_DIR/"
