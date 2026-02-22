# Build Kinetix Installer (single-file, all-in-one)
# Builds kivm + kicomp first, then the installer embeds them via include_bytes!

Write-Host "=== Building Kinetix Installer ===" -ForegroundColor Cyan

# Resolve workspace root (parent of scripts/)
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $scriptDir

# Step 1: Build kivm and kicomp in release mode (the installer embeds these)
Write-Host "`n[1/3] Compiling kivm and kicomp..." -ForegroundColor Yellow
Push-Location $root

# Set build version for CLI/Comp to pick up
$env:KINETIX_BUILD = "12"

# Clean to ensure env var is picked up
cargo clean -p kinetix-cli -p kinetix-kicomp

cargo build --release -p kinetix-cli -p kinetix-kicomp
if ($LASTEXITCODE -ne 0) {
    Pop-Location
    Write-Host "Build failed." -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
}
Pop-Location
Write-Host "OK" -ForegroundColor Green

# Verify the built binary version
Write-Host "Verifying built binary version..." -ForegroundColor Cyan
& "$root\target\release\kivm.exe" version

# Check that the release binaries exist
$kivmExe = Join-Path $root "target\release\kivm.exe"
$kicompExe = Join-Path $root "target\release\kicomp.exe"
if (!(Test-Path $kivmExe)) {
    Write-Host "ERROR: kivm.exe not found at $kivmExe" -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
}
if (!(Test-Path $kicompExe)) {
    Write-Host "ERROR: kicomp.exe not found at $kicompExe" -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
}
Write-Host "  kivm.exe:   $kivmExe" -ForegroundColor DarkGray
Write-Host "  kicomp.exe: $kicompExe" -ForegroundColor DarkGray

# Step 2: Build the installer (it uses include_bytes! to embed the release binaries)
# The installer is excluded from the workspace, so we must build from its own directory.
Write-Host "`n[2/3] Compiling installer..." -ForegroundColor Yellow
Push-Location (Join-Path $root "crates\installer")

# We don't manually delete the target folder because Windows often holds locks momentarily,
# causing subsequent cargo builds to fail (os error 32 on zerocopy or others).
# Instead, we just rely on cargo's caching. include_bytes! tracks file changes.
Write-Host "  Building installer executable (UI)..." -ForegroundColor DarkGray

cargo build --release
if ($LASTEXITCODE -ne 0) {
    Pop-Location
    Write-Host "Installer build failed." -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
}
Pop-Location
Write-Host "OK" -ForegroundColor Green

# Step 3: Copy result to dist/
Write-Host "`n[3/3] Copying to dist..." -ForegroundColor Yellow
$dist = Join-Path $root "dist"
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Path $dist -Force | Out-Null

$installerSrc = Join-Path $root "crates\installer\target\release\installer.exe"
if (!(Test-Path $installerSrc)) {
    Write-Host "ERROR: installer.exe not found at $installerSrc" -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
}

Copy-Item $installerSrc (Join-Path $dist "installer.exe")

# Show the installed binary version for verification
$installerSize = (Get-Item (Join-Path $dist "installer.exe")).Length / 1MB
Write-Host "`n=== Done ===" -ForegroundColor Green
Write-Host "Output: $dist\installer.exe ($([math]::Round($installerSize, 1)) MB)"
Write-Host "  Embedded kivm.exe:   $((Get-Item $kivmExe).Length / 1MB | ForEach-Object { [math]::Round($_, 1) }) MB"
Write-Host "  Embedded kicomp.exe: $((Get-Item $kicompExe).Length / 1MB | ForEach-Object { [math]::Round($_, 1) }) MB"
Write-Host ""
Read-Host "Press Enter to exit"
