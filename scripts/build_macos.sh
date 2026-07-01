#!/bin/bash
# Build script for macOS (.pkg Installer)
set -e

# Load cargo into PATH if installed via rustup
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

export KINETIX_BUILD="35"

# Ensure we are running from the workspace root
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 'version = ' crates/cli/Cargo.toml | sed 's/.*"\(.*\)"/\1/')
IDENTIFIER="com.mistery3515.kinetix"
OUTPUT_DIR="dist_mac"
STAGING="$OUTPUT_DIR/staging"
SCRIPTS="$OUTPUT_DIR/scripts"

echo "=== Building Kinetix for macOS (v$VERSION) ==="

# 1. Build release binaries
echo "[1/5] Compiling..."
cargo build --release --package kinetix-cli --package kinetix-kicomp

# 2. Build the GUI Installer (self-contained with embedded binaries)
echo "[2/5] Building GUI Installer..."
cargo build --release --package kinetix-installer

# 3. Stage the payload for pkgbuild
echo "[3/5] Staging payload..."
rm -rf "$OUTPUT_DIR"
mkdir -p "$STAGING/usr/local/bin"
mkdir -p "$STAGING/usr/local/share/kinetix/assets"
mkdir -p "$SCRIPTS"

# Copy main binaries
cp target/release/kivm "$STAGING/usr/local/bin/kivm"
cp target/release/kicomp "$STAGING/usr/local/bin/kicomp"
cp target/release/installer "$STAGING/usr/local/bin/KinetixInstaller"
chmod +x "$STAGING/usr/local/bin/kivm"
chmod +x "$STAGING/usr/local/bin/kicomp"
chmod +x "$STAGING/usr/local/bin/KinetixInstaller"

# Copy assets
if [ -f assets/logo/KiFile.png ]; then
    cp assets/logo/KiFile.png "$STAGING/usr/local/share/kinetix/assets/"
fi

# Build a real .icns from the source PNG (a PNG merely renamed .icns is not
# a valid icon and Finder/LaunchServices will refuse to render it)
ICNS_FILE=""
if [ -f assets/logo/KiFile.png ] && command -v iconutil &> /dev/null && command -v sips &> /dev/null; then
    ICONSET="$OUTPUT_DIR/KiFile.iconset"
    mkdir -p "$ICONSET"
    for size in 16 32 128 256 512; do
        sips -z "$size" "$size" assets/logo/KiFile.png --out "$ICONSET/icon_${size}x${size}.png" > /dev/null
        double=$((size * 2))
        sips -z "$double" "$double" assets/logo/KiFile.png --out "$ICONSET/icon_${size}x${size}@2x.png" > /dev/null
    done
    iconutil -c icns "$ICONSET" -o "$OUTPUT_DIR/KiFile.icns"
    rm -rf "$ICONSET"
    ICNS_FILE="$OUTPUT_DIR/KiFile.icns"
fi

# Create the handler .app bundle (for file associations)
APP_DIR="$STAGING/Applications/Kinetix.app"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# Launcher script. Runs in Terminal.app: a bundle launched from Finder has
# no TTY attached, so exec'ing kivm directly produces a process with no
# visible window and no way to read stdin.
cat > "$APP_DIR/Contents/MacOS/KinetixEnv" <<'LAUNCHER'
#!/bin/bash
if [ -n "$1" ]; then
  ESCAPED_PATH=$(printf '%s' "$1" | sed 's/[\\"]/\\&/g')
  osascript -e "tell application \"Terminal\" to do script \"/usr/local/bin/kivm exec \\\"$ESCAPED_PATH\\\"\""
else
  osascript -e 'tell application "Terminal" to do script "/usr/local/bin/kivm shell"'
fi
LAUNCHER
chmod +x "$APP_DIR/Contents/MacOS/KinetixEnv"

# Copy icon
if [ -n "$ICNS_FILE" ]; then
    cp "$ICNS_FILE" "$APP_DIR/Contents/Resources/KiFile.icns"
fi

# Info.plist with file associations
cat > "$APP_DIR/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>KinetixEnv</string>
    <key>CFBundleIdentifier</key>
    <string>com.mistery3515.kinetix</string>
    <key>CFBundleName</key>
    <string>Kinetix</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleIconFile</key>
    <string>KiFile</string>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>kix</string>
                <string>ki</string>
                <string>exki</string>
            </array>
            <key>CFBundleTypeIconFile</key>
            <string>KiFile</string>
            <key>CFBundleTypeName</key>
            <string>Kinetix Source File</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Owner</string>
        </dict>
    </array>
</dict>
</plist>
PLIST

# Post-install script: refresh LaunchServices for file associations
cat > "$SCRIPTS/postinstall" <<'POSTINSTALL'
#!/bin/bash
# Register file associations
/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister -f /Applications/Kinetix.app 2>/dev/null || true
echo "Kinetix installation complete."
POSTINSTALL
chmod +x "$SCRIPTS/postinstall"

# 4. Ad-hoc code sign (no Apple Developer ID configured yet).
# This stops "no usable signature" warnings but does NOT satisfy Gatekeeper
# for apps downloaded over the internet (quarantine flag) on other Macs --
# that requires a paid Developer ID cert plus notarization via `notarytool`.
echo "[4/5] Ad-hoc signing..."
if command -v codesign &> /dev/null; then
    codesign --force --sign - "$APP_DIR"
    codesign --force --sign - "$STAGING/usr/local/bin/kivm"
    codesign --force --sign - "$STAGING/usr/local/bin/kicomp"
    codesign --force --sign - "$STAGING/usr/local/bin/KinetixInstaller"
fi

# 5. Build .pkg
echo "[5/5] Creating .pkg..."
if command -v pkgbuild &> /dev/null; then
    pkgbuild --root "$STAGING" \
             --identifier "$IDENTIFIER" \
             --version "$VERSION" \
             --scripts "$SCRIPTS" \
             --install-location "/" \
             "$OUTPUT_DIR/KinetixInstaller.pkg"
    
    echo "=== Done ==="
    echo "Output: $OUTPUT_DIR/KinetixInstaller.pkg"
else
    echo "Warning: pkgbuild not found (requires Xcode Command Line Tools)."
    echo "Falling back to standalone binary..."
    cp target/release/installer "$OUTPUT_DIR/KinetixInstaller"
    chmod +x "$OUTPUT_DIR/KinetixInstaller"
    echo "=== Done ==="
    echo "Output: $OUTPUT_DIR/KinetixInstaller"
fi
