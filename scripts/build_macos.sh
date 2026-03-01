#!/bin/bash
# Build script for macOS (.pkg Installer)
set -e

export KINETIX_BUILD="23"

# Ensure we are running from the workspace root
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 'version = ' crates/cli/Cargo.toml | sed 's/.*"\(.*\)"/\1/')
IDENTIFIER="com.mistery3515.kinetix"
OUTPUT_DIR="dist_mac"
STAGING="$OUTPUT_DIR/staging"
SCRIPTS="$OUTPUT_DIR/scripts"

echo "=== Building Kinetix for macOS (v$VERSION) ==="

# 1. Build release binaries
echo "[1/4] Compiling..."
cargo build --release --package kinetix-cli --package kinetix-kicomp

# 2. Build the GUI Installer (self-contained with embedded binaries)
echo "[2/4] Building GUI Installer..."
cargo build --release --package kinetix-installer

# 3. Stage the payload for pkgbuild
echo "[3/4] Staging payload..."
rm -rf "$OUTPUT_DIR"
mkdir -p "$STAGING/usr/local/bin"
mkdir -p "$STAGING/usr/local/share/kinetix/assets"
mkdir -p "$STAGING/Library/PreferencePanes"
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

# Create the handler .app bundle (for file associations)
APP_DIR="$STAGING/Applications/Kinetix.app"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# Launcher script
cat > "$APP_DIR/Contents/MacOS/KinetixEnv" <<'LAUNCHER'
#!/bin/bash
if [ -n "$1" ]; then
  /usr/local/bin/kivm exec "$1"
else
  /usr/local/bin/kivm shell
fi
LAUNCHER
chmod +x "$APP_DIR/Contents/MacOS/KinetixEnv"

# Copy icon
if [ -f assets/logo/KiFile.png ]; then
    cp assets/logo/KiFile.png "$APP_DIR/Contents/Resources/KiFile.icns"
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

# Create System Preferences pane stub (like Java)
PANE_DIR="$STAGING/Library/PreferencePanes/Kinetix.prefPane"
mkdir -p "$PANE_DIR/Contents/MacOS"
mkdir -p "$PANE_DIR/Contents/Resources"

# Copy icon for prefpane
if [ -f assets/logo/KiFile.png ]; then
    cp assets/logo/KiFile.png "$PANE_DIR/Contents/Resources/KiFile.icns"
fi

# Launcher: opens the Kinetix Installer GUI (or a status window)
cat > "$PANE_DIR/Contents/MacOS/Kinetix" <<'PREFPANE_LAUNCHER'
#!/bin/bash
# Opens the Kinetix Installer/Manager GUI
if [ -x /usr/local/bin/KinetixInstaller ]; then
    /usr/local/bin/KinetixInstaller "$@"
else
    osascript -e 'display dialog "Kinetix is not installed. Please reinstall." buttons {"OK"} default button "OK" with icon caution with title "Kinetix"'
fi
PREFPANE_LAUNCHER
chmod +x "$PANE_DIR/Contents/MacOS/Kinetix"

# Info.plist for the prefpane
cat > "$PANE_DIR/Contents/Info.plist" <<'PREFPLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>Kinetix</string>
    <key>CFBundleIdentifier</key>
    <string>com.mistery3515.kinetix.prefpane</string>
    <key>CFBundleName</key>
    <string>Kinetix</string>
    <key>CFBundlePackageType</key>
    <string>BNDL</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>NSPrefPaneIconFile</key>
    <string>KiFile.icns</string>
    <key>NSPrefPaneIconLabel</key>
    <string>Kinetix</string>
    <key>NSPrincipalClass</key>
    <string>NSPrefPane</string>
</dict>
</plist>
PREFPLIST

# Post-install script: refresh LaunchServices for file associations
cat > "$SCRIPTS/postinstall" <<'POSTINSTALL'
#!/bin/bash
# Register file associations
/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister -f /Applications/Kinetix.app 2>/dev/null || true
echo "Kinetix installation complete."
POSTINSTALL
chmod +x "$SCRIPTS/postinstall"

# 4. Build .pkg
echo "[4/4] Creating .pkg..."
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
