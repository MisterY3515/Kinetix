@echo off
setlocal enabledelayedexpansion
title Kinetix build prerequisites

echo === Kinetix build prerequisites (Windows) ===
echo.
echo This installs: Rust (rustup), MSVC C++ Build Tools with the x86_64 +
echo ARM64 components (needed to link kivm/kicomp/the installer), and
echo LLVM/Clang (needed by the "ring" crypto crate specifically to build for
echo the ARM64 target -- MSVC's own compiler can't build it there).
echo Safe to re-run -- each step checks first and skips if already done.
echo Everything below runs back-to-back in this same window -- PATH is
echo re-read from the registry after each install, so there's no need to
echo close and reopen the terminal partway through.
echo.

where winget >nul 2>&1
if errorlevel 1 goto :no_winget

echo [1/4] Rust toolchain...
where rustc >nul 2>&1
if errorlevel 1 goto :install_rust
echo Rust already installed, skipping.
goto :check_msvc

:install_rust
echo Installing Rust via winget...
rem --source winget pins the lookup to the winget community repo and skips
rem the Microsoft Store source entirely -- on some networks (corporate
rem proxies/TLS inspection) msstore fails to search at all, which makes
rem winget report an ambiguous match instead of actually installing anything
rem (and, worse, still exits 0 when that happens).
winget install --id Rustlang.Rustup -e --source winget --accept-source-agreements --accept-package-agreements
if errorlevel 1 goto :rust_failed
call :refresh_path
goto :check_msvc

:rust_failed
echo.
echo ERROR: Rust install failed. Install manually from https://rustup.rs and re-run this script.
pause
exit /b 1

:check_msvc
echo.
echo [2/4] MSVC C++ Build Tools (x86_64 + ARM64)...
rem "where cl" is NOT a reliable presence check: cl.exe is only ever on PATH
rem inside a "Developer Command Prompt" (vcvars-loaded) session, never in a
rem plain terminal, regardless of whether the Build Tools are installed --
rem so this reported "missing" every single run and looped back into
rem :install_msvc forever, never reaching the LLVM step below. Ask vswhere
rem instead, the same tool cargo/rustc themselves use to locate MSVC.
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" goto :install_msvc
set "VSPATH="
for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -requires Microsoft.VisualStudio.Component.VC.Tools.ARM64 -property installationPath`) do set "VSPATH=%%i"
if defined VSPATH goto :msvc_present
goto :install_msvc

:msvc_present
echo MSVC Build Tools already installed (x86_64 + ARM64), skipping.
goto :check_llvm

:install_msvc
echo Installing Visual Studio Build Tools (C++ workload + ARM64 tools) via winget.
echo This downloads several GB and can take a while -- please wait, do not close this window.
rem --force makes winget re-invoke the VS bootstrapper with our --override
rem args even when it considers the package "already installed" with no
rem version upgrade available -- without it winget just no-ops (as seen on
rem this project: "Found an existing package... No available upgrade
rem found") and a component missing from the original install (e.g. the
rem ARM64 tools) would silently never get added.
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --source winget --accept-source-agreements --accept-package-agreements --force --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --add Microsoft.VisualStudio.Component.VC.Tools.ARM64 --includeRecommended"
if errorlevel 1 goto :msvc_failed
rem cl.exe/link.exe are never added to plain PATH regardless of install
rem state (see the comment above) -- rustc locates them itself via the
rem same vswhere mechanism, so unlike Rust and LLVM there's no PATH to
rem refresh here, and no restart is actually needed for this step.
goto :check_llvm

:msvc_failed
echo.
echo ERROR: Build Tools install failed. Install manually instead:
echo   https://visualstudio.microsoft.com/visual-cpp-build-tools/
echo Select "Desktop development with C++", plus the individual component
echo "MSVC v143 - VS 2022 C++ ARM64 build tools".
pause
exit /b 1

:check_llvm
echo.
echo [3/4] LLVM/Clang (required for the ARM64 target, optional native backend on x86_64)...
where clang-cl >nul 2>&1
if not errorlevel 1 goto :llvm_present
rem LLVM's installer doesn't reliably register itself on PATH even when
rem winget reports success (confirmed directly from a real build: clang-cl.exe
rem present on disk but absent from both Machine and User PATH) -- also probe
rem the standard install location before deciding we actually need to install.
if not exist "%ProgramFiles%\LLVM\bin\clang-cl.exe" goto :install_llvm
set "PATH=%ProgramFiles%\LLVM\bin;%PATH%"

:llvm_present
echo LLVM/Clang already installed, skipping.
goto :add_targets

:install_llvm
echo Installing LLVM via winget...
winget install --id LLVM.LLVM -e --source winget --accept-source-agreements --accept-package-agreements
if errorlevel 1 goto :llvm_failed
call :refresh_path
if exist "%ProgramFiles%\LLVM\bin\clang-cl.exe" set "PATH=%ProgramFiles%\LLVM\bin;%PATH%"
goto :add_targets

:llvm_failed
echo.
echo ERROR: LLVM install failed. Install manually instead:
echo   https://github.com/llvm/llvm-project/releases
echo (grab the Windows installer, e.g. LLVM-*-win64.exe) and re-run this script.
pause
exit /b 1

:add_targets
echo.
echo [4/4] Rust cross-compilation targets...
rustup target add x86_64-pc-windows-msvc >nul 2>&1
rustup target add aarch64-pc-windows-msvc >nul 2>&1

echo.
echo === Done. You can now run scripts\build_installer.ps1 in this same window. ===
pause
exit /b 0

:no_winget
echo.
echo ERROR: winget not found.
echo Install "App Installer" from the Microsoft Store, then re-run this script:
echo   https://apps.microsoft.com/detail/9nblggh4nns1
pause
exit /b 1

rem Re-reads PATH from the registry (Machine + User) into this process, so
rem a tool installed moments ago by winget (cargo/rustc, clang-cl) is
rem immediately usable without closing and reopening the terminal. Shells
rem out to PowerShell for this instead of parsing "reg query" text, since
rem .NET's GetEnvironmentVariable is unambiguous and rows in reg query's
rem output are easy to misparse.
:refresh_path
for /f "usebackq delims=" %%P in (`powershell -NoProfile -Command "[System.Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [System.Environment]::GetEnvironmentVariable('Path','User')"`) do set "PATH=%%P"
goto :eof
