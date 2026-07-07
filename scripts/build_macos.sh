#!/bin/bash
# Build script for macOS (.pkg Installer) -- produces a single Universal
# (x86_64 + arm64) binary via lipo, so one download runs natively on both
# Intel and Apple Silicon Macs.
set -e

# Load cargo into PATH if installed via rustup
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

export KINETIX_BUILD="37"

# Ensure we are running from the workspace root
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 'version = ' crates/cli/Cargo.toml | sed 's/.*"\(.*\)"/\1/')
IDENTIFIER="com.mistery3515.kinetix"
OUTPUT_DIR="dist_mac"
STAGING="$OUTPUT_DIR/staging"
SCRIPTS="$OUTPUT_DIR/scripts"
UNIVERSAL="$OUTPUT_DIR/universal"

TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin")

echo "=== Building Kinetix for macOS (v$VERSION, Universal) ==="

rm -rf "$OUTPUT_DIR"
mkdir -p "$UNIVERSAL" "$STAGING/usr/local/bin" "$STAGING/usr/local/share/kinetix/assets" "$SCRIPTS"

# 1. Ensure both target toolchains are installed (no-op if already present).
echo "[1/7] Ensuring cross-compilation targets..."
for target in "${TARGETS[@]}"; do
    rustup target add "$target" > /dev/null
done

# 2. Build kivm/kicomp for each architecture natively (macOS's own linker
#    cross-links both Apple Silicon and Intel targets without Docker/extra SDKs).
echo "[2/7] Compiling kivm/kicomp for ${TARGETS[*]}..."
for target in "${TARGETS[@]}"; do
    cargo build --release --target "$target" --package kinetix-cli --package kinetix-kicomp
done

# 3. Merge each binary into a Universal (fat) binary with lipo, then stage
#    the merged binaries at target/release/ -- the installer crate's
#    include_bytes! paths are hardcoded to target/release/{kivm,kicomp}
#    (not target-triple-aware), so the installer must see the Universal
#    binaries there *before* it is built, or it will silently embed
#    whichever single-arch binary happened to be built last.
echo "[3/7] Merging into Universal binaries (lipo)..."
for bin in kivm kicomp; do
    lipo -create -output "$UNIVERSAL/$bin" \
        "target/${TARGETS[0]}/release/$bin" \
        "target/${TARGETS[1]}/release/$bin"
    lipo -info "$UNIVERSAL/$bin"
done
mkdir -p target/release
cp "$UNIVERSAL/kivm" target/release/kivm
cp "$UNIVERSAL/kicomp" target/release/kicomp

# 4. Build the GUI Installer for each architecture (embeds the Universal
#    kivm/kicomp bytes staged above), then merge the installer itself into
#    a Universal binary too, so the installer app runs natively everywhere.
echo "[4/7] Building GUI Installer for ${TARGETS[*]}..."
for target in "${TARGETS[@]}"; do
    cargo build --release --target "$target" --package kinetix-installer
done
lipo -create -output "$UNIVERSAL/installer" \
    "target/${TARGETS[0]}/release/installer" \
    "target/${TARGETS[1]}/release/installer"
lipo -info "$UNIVERSAL/installer"

# 5. Stage the payload for pkgbuild
echo "[5/7] Staging payload..."

# Copy main binaries (all Universal)
cp "$UNIVERSAL/kivm" "$STAGING/usr/local/bin/kivm"
cp "$UNIVERSAL/kicomp" "$STAGING/usr/local/bin/kicomp"
cp "$UNIVERSAL/installer" "$STAGING/usr/local/bin/KinetixInstaller"
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

# Create the handler .app bundle (for file associations).
#
# Built via `osacompile` (a compiled AppleScript applet), NOT a plain bash
# executable. macOS delivers file-open events (double-click, "Open With") as
# Apple Events (kAEOpenDocuments) -- only a bundle with a real Apple Event
# handler receives these. A bash script set as CFBundleExecutable never gets
# the file path this way (verified: invoking it directly with an argv file
# path does not trigger it either -- Apple Events are LaunchServices-mediated,
# not argv-based), so double-clicking a .kix/.exki always launched with an
# empty argv and fell through to the "no file -> open a bare shell" branch
# instead of running the file. osacompile produces a proper applet whose
# `on open` handler does receive the Apple Event with the real file path.
APP_DIR="$STAGING/Applications/Kinetix.app"
APPLESCRIPT_SRC="$OUTPUT_DIR/KinetixEnv.applescript"
cat > "$APPLESCRIPT_SRC" <<'APPLESCRIPT'
on open theFiles
    repeat with aFile in theFiles
        set posixPath to POSIX path of aFile
        tell application "Terminal"
            activate
            do script "/usr/local/bin/kivm exec " & quoted form of posixPath
        end tell
    end repeat
end open

on run
    tell application "Terminal"
        activate
        do script "/usr/local/bin/kivm shell"
    end tell
end run
APPLESCRIPT

rm -rf "$APP_DIR"
mkdir -p "$STAGING/Applications"
osacompile -o "$APP_DIR" "$APPLESCRIPT_SRC"

# Copy icon and point the bundle at it (osacompile ships its own default
# "droplet" icon via both a legacy .icns and a modern Assets.car catalog --
# drop both so nothing stale is left behind).
if [ -n "$ICNS_FILE" ]; then
    rm -f "$APP_DIR/Contents/Resources/droplet.icns" "$APP_DIR/Contents/Resources/Assets.car"
    cp "$ICNS_FILE" "$APP_DIR/Contents/Resources/KiFile.icns"
fi

# Patch in our custom Info.plist keys. osacompile's own required keys
# (CFBundleExecutable, NSAppleScriptEnabled, the Apple Events usage
# description, etc.) are left untouched.
INFO_PLIST="$APP_DIR/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleIdentifier string $IDENTIFIER" "$INFO_PLIST"
/usr/libexec/PlistBuddy -c "Set :CFBundleName Kinetix" "$INFO_PLIST"
if [ -n "$ICNS_FILE" ]; then
    /usr/libexec/PlistBuddy -c "Delete :CFBundleIconName" "$INFO_PLIST"
    /usr/libexec/PlistBuddy -c "Set :CFBundleIconFile KiFile" "$INFO_PLIST"
fi
/usr/libexec/PlistBuddy -c "Delete :CFBundleDocumentTypes" "$INFO_PLIST"
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes array" "$INFO_PLIST"
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0 dict" "$INFO_PLIST"
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:CFBundleTypeExtensions array" "$INFO_PLIST"
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:CFBundleTypeExtensions:0 string kix" "$INFO_PLIST"
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:CFBundleTypeExtensions:1 string ki" "$INFO_PLIST"
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:CFBundleTypeExtensions:2 string exki" "$INFO_PLIST"
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:CFBundleTypeIconFile string KiFile" "$INFO_PLIST"
/usr/libexec/PlistBuddy -c 'Add :CFBundleDocumentTypes:0:CFBundleTypeName string "Kinetix Source File"' "$INFO_PLIST"
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:CFBundleTypeRole string Viewer" "$INFO_PLIST"
/usr/libexec/PlistBuddy -c "Add :CFBundleDocumentTypes:0:LSHandlerRank string Owner" "$INFO_PLIST"

# Post-install script: refresh LaunchServices for file associations
cat > "$SCRIPTS/postinstall" <<'POSTINSTALL'
#!/bin/bash
# Register file associations
/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister -f /Applications/Kinetix.app 2>/dev/null || true
echo "Kinetix installation complete."
POSTINSTALL
chmod +x "$SCRIPTS/postinstall"

# 6. Ad-hoc code sign (no Apple Developer ID configured yet).
# This stops "no usable signature" warnings but does NOT satisfy Gatekeeper
# for apps downloaded over the internet (quarantine flag) on other Macs --
# that requires a paid Developer ID cert plus notarization via `notarytool`.
echo "[6/7] Ad-hoc signing..."
if command -v codesign &> /dev/null; then
    codesign --force --sign - "$APP_DIR"
    codesign --force --sign - "$STAGING/usr/local/bin/kivm"
    codesign --force --sign - "$STAGING/usr/local/bin/kicomp"
    codesign --force --sign - "$STAGING/usr/local/bin/KinetixInstaller"
fi

# 7. Build .pkg
# Named consistently with the Windows/Linux artifacts (KinetixInstaller-<os>-<arch>)
# so a GitHub Release upload is unambiguous and scriptable -- "universal" here
# since one file runs natively on both Intel and Apple Silicon.
echo "[7/7] Creating .pkg..."
PKG_NAME="KinetixInstaller-macos-universal.pkg"
if command -v pkgbuild &> /dev/null; then
    pkgbuild --root "$STAGING" \
             --identifier "$IDENTIFIER" \
             --version "$VERSION" \
             --scripts "$SCRIPTS" \
             --install-location "/" \
             "$OUTPUT_DIR/$PKG_NAME"

    echo "=== Done ==="
    echo "Output: $OUTPUT_DIR/$PKG_NAME (Universal: ${TARGETS[*]})"
else
    echo "Warning: pkgbuild not found (requires Xcode Command Line Tools)."
    echo "Falling back to standalone binary..."
    cp "$UNIVERSAL/installer" "$OUTPUT_DIR/KinetixInstaller-macos-universal"
    chmod +x "$OUTPUT_DIR/KinetixInstaller-macos-universal"
    echo "=== Done ==="
    echo "Output: $OUTPUT_DIR/KinetixInstaller-macos-universal (Universal: ${TARGETS[*]})"
fi
