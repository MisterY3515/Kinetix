# Build Kinetix Installer (single-file, all-in-one)
# Builds kivm + kicomp first, then the installer embeds them via include_bytes!

Write-Host "=== Building Kinetix Installer ===" -ForegroundColor Cyan

# Step 1: Build kivm and kicomp in release mode FIRST (the installer embeds these)
Write-Host "`n[1/2] Compiling kivm and kicomp..." -ForegroundColor Yellow
cargo build --release -p kinetix-cli -p kinetix-kicomp
if ($LASTEXITCODE -ne 0) {
    Write-Host "Build failed." -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
}
Write-Host "OK" -ForegroundColor Green

# Step 2: Build the installer (embeds the release binaries automatically)
Write-Host "`n[2/2] Compiling installer..." -ForegroundColor Yellow
cargo build --release -p kinetix-installer
if ($LASTEXITCODE -ne 0) {
    Write-Host "Build failed." -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
}

# Copy result to dist/
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$dist = Join-Path $root "dist"
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Path $dist -Force | Out-Null
Copy-Item (Join-Path $root "target\release\installer.exe") (Join-Path $dist "installer.exe")

Write-Host "`n=== Done ===" -ForegroundColor Green
Write-Host "Output: $dist\installer.exe"
Write-Host ""
Read-Host "Press Enter to exit"
