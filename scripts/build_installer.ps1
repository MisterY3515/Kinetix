# Build Kinetix Installer (single-file, all-in-one)
# Builds kivm + kicomp first, then the installer embeds them via include_bytes!
# Produces one installer.exe per architecture (x86_64 and arm64) -- unlike
# macOS's Mach-O, a Windows PE binary can't be merged into one "universal" file.
#
# Builds both architectures by default -- pass -Arch x64/arm64 to build only
# one (e.g. for a quick local test after a prerequisite-install rerun).
# Building the arm64 installer requires the "ARM64 build tools" component
# for MSVC (Visual Studio Installer -> Individual Components -> MSVC v143 -
# VS 2022 C++ ARM64 build tools) *and* Clang/LLVM (the `ring` crate needs
# clang-cl specifically to build for aarch64-pc-windows-msvc -- MSVC's own
# compiler can't). scripts\install_prerequisites.bat installs both.
#
# Usage:
#   .\scripts\build_installer.ps1                # both x86_64 and arm64 (default)
#   .\scripts\build_installer.ps1 -Arch x64       # x86_64 only
#   .\scripts\build_installer.ps1 -Arch arm64     # arm64 only

param(
    [ValidateSet("x64", "arm64", "both")]
    [string]$Arch = "both"
)

# Safety net: PowerShell cmdlet errors (a missing file for Copy-Item, a
# locked .exe, etc.) are non-terminating by default -- the script would
# otherwise print a red error and silently carry on to the next step
# instead of stopping, which is indistinguishable from "it crashed with no
# message" once the real failure has scrolled off screen. $ErrorActionPreference
# makes every such error terminating so the trap below always catches it.
$ErrorActionPreference = "Stop"

trap {
    Write-Host "`nFATAL ERROR: $_" -ForegroundColor Red
    Write-Host $_.ScriptStackTrace -ForegroundColor DarkGray
    Read-Host "Press Enter to exit"
    exit 1
}

# winget-installed tools (rustup, MSVC Build Tools, LLVM/clang-cl) only
# update the registry-level PATH -- an already-open PowerShell session keeps
# its own stale copy of PATH for its entire lifetime, so "clang not found"
# right after install_prerequisites.bat reports LLVM installed successfully
# (in the very same window) is expected unless we re-read the authoritative
# value ourselves here, instead of relying on the terminal being reopened.
$machinePath = [System.Environment]::GetEnvironmentVariable("Path", "Machine")
$userPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
$env:Path = "$machinePath;$userPath"

# Some LLVM releases don't register themselves on PATH at all even on a
# successful install -- confirmed directly from cargo's own build-script
# diagnostic dump: clang-cl.exe present on disk, but its directory absent
# from both the Machine and User PATH above. Probe the standard install
# location directly rather than trusting PATH registration.
$llvmBin = Join-Path $env:ProgramFiles "LLVM\bin"
if ((Test-Path (Join-Path $llvmBin "clang-cl.exe")) -and ($env:Path -notlike "*$llvmBin*")) {
    $env:Path = "$llvmBin;$env:Path"
}

Write-Host "=== Building Kinetix Installer ($Arch) ===" -ForegroundColor Cyan

# Resolve workspace root (parent of scripts/)
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $scriptDir

$env:KINETIX_BUILD = "36"

# Redirect cargo's own build output to a local disk. Network/shared-VM
# drives (SMB-style semantics, e.g. a Parallels shared-folder drive letter)
# don't reliably support the file-locking and rename operations rustc needs
# when writing its build cache and .rlib archives, causing sporadic "failed
# to remove temporary directory" (os error 87) failures. Only the two
# staged binaries below -- which the installer's include_bytes! embeds via
# a path hardcoded relative to this repo, so they can't be redirected --
# still get written under $root itself.
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "KinetixBuild"
Write-Host "Using local build cache: $($env:CARGO_TARGET_DIR)" -ForegroundColor DarkGray

# Filename labels stay "x86_64"/"arm64" (matching the KinetixInstaller-<os>-<arch>
# convention used for all platforms), independent of the shorter "x64" the
# -Arch parameter accepts for convenience.
$allTargets = [ordered]@{
    "x86_64-pc-windows-msvc" = "x86_64"
    "aarch64-pc-windows-msvc" = "arm64"
}
$wantedLabel = @{ "x64" = "x86_64"; "arm64" = "arm64" }[$Arch]
$targets = [ordered]@{}
foreach ($t in $allTargets.Keys) {
    $label = $allTargets[$t]
    if ($Arch -eq "both" -or $label -eq $wantedLabel) {
        $targets[$t] = $label
    }
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
    Copy-Item (Join-Path $env:CARGO_TARGET_DIR "$target\release\kivm.exe") (Join-Path $targetRelease "kivm.exe") -Force
    Copy-Item (Join-Path $env:CARGO_TARGET_DIR "$target\release\kicomp.exe") (Join-Path $targetRelease "kicomp.exe") -Force

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

    $installerSrc = Join-Path $env:CARGO_TARGET_DIR "$target\release\installer.exe"
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
