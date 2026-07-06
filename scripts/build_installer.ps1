# Build Kinetix Installer (single-file, all-in-one)
# Builds kivm + kicomp first, then the installer embeds them via include_bytes!
# Produces one installer.exe per architecture (x86_64 and arm64) -- unlike
# macOS's Mach-O, a Windows PE binary can't be merged into one "universal" file.
#
# Building the arm64 installer requires the "ARM64 build tools" component
# for MSVC (Visual Studio Installer -> Individual Components -> MSVC v143 -
# VS 2022 C++ ARM64 build tools) in addition to the default x86_64 toolchain.

Write-Host "=== Building Kinetix Installer (x86_64 + arm64) ===" -ForegroundColor Cyan

# Resolve workspace root (parent of scripts/)
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $scriptDir

$env:KINETIX_BUILD = "36"

$targets = @{
    "x86_64-pc-windows-msvc" = "x86_64"
    "aarch64-pc-windows-msvc" = "arm64"
}

$dist = Join-Path $root "dist"
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Path $dist -Force | Out-Null

foreach ($target in $targets.Keys) {
    $label = $targets[$target]

    Write-Host "`n--- [$label] Ensuring target $target is installed ---" -ForegroundColor Yellow
    rustup target add $target | Out-Null

    Write-Host "--- [$label] Compiling kivm and kicomp ($target) ---" -ForegroundColor Yellow
    Push-Location $root
    cargo build --release --target $target -p kinetix-cli -p kinetix-kicomp
    if ($LASTEXITCODE -ne 0) {
        Pop-Location
        Write-Host "[$label] Build failed." -ForegroundColor Red
        Read-Host "Press Enter to exit"
        exit 1
    }
    Pop-Location

    # The installer crate embeds kivm.exe/kicomp.exe via a path hardcoded to
    # target/release/ (not target/<triple>/release/), so the binaries for the
    # architecture we're about to build the installer for must be staged
    # there first -- otherwise the installer would silently embed whichever
    # architecture happened to be built last.
    $targetRelease = Join-Path $root "target\release"
    New-Item -ItemType Directory -Path $targetRelease -Force | Out-Null
    Copy-Item (Join-Path $root "target\$target\release\kivm.exe") (Join-Path $targetRelease "kivm.exe") -Force
    Copy-Item (Join-Path $root "target\$target\release\kicomp.exe") (Join-Path $targetRelease "kicomp.exe") -Force

    Write-Host "--- [$label] Compiling installer ($target) ---" -ForegroundColor Yellow
    Push-Location (Join-Path $root "crates\installer")
    cargo build --release --target $target
    if ($LASTEXITCODE -ne 0) {
        Pop-Location
        Write-Host "[$label] Installer build failed." -ForegroundColor Red
        Read-Host "Press Enter to exit"
        exit 1
    }
    Pop-Location

    $installerSrc = Join-Path $root "target\$target\release\installer.exe"
    if (!(Test-Path $installerSrc)) {
        Write-Host "ERROR: installer.exe not found at $installerSrc" -ForegroundColor Red
        Read-Host "Press Enter to exit"
        exit 1
    }
    $installerOut = Join-Path $dist "KinetixInstaller-windows-$label.exe"
    Copy-Item $installerSrc $installerOut

    $installerSize = (Get-Item $installerOut).Length / 1MB
    Write-Host "[$label] OK -- $installerOut ($([math]::Round($installerSize, 1)) MB)" -ForegroundColor Green
}

Write-Host "`n=== Done ===" -ForegroundColor Green
Write-Host "Output: $dist"
foreach ($label in $targets.Values) {
    Write-Host "  KinetixInstaller-windows-$label.exe"
}
Read-Host "Press Enter to exit"
