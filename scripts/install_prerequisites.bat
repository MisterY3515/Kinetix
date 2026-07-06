@echo off
setlocal enabledelayedexpansion
title Kinetix build prerequisites

echo === Kinetix build prerequisites (Windows) ===
echo.
echo This installs: Rust (rustup), and MSVC C++ Build Tools with the
echo x86_64 + ARM64 components (needed to link kivm/kicomp/the installer).
echo Safe to re-run -- each step checks first and skips if already done.
echo.

where winget >nul 2>&1
if errorlevel 1 goto :no_winget

echo [1/3] Rust toolchain...
where rustc >nul 2>&1
if errorlevel 1 goto :install_rust
echo Rust already installed, skipping.
goto :check_msvc

:install_rust
echo Installing Rust via winget...
winget install --id Rustlang.Rustup -e --accept-source-agreements --accept-package-agreements
if errorlevel 1 goto :rust_failed
echo.
echo IMPORTANT: close and reopen this terminal so PATH picks up cargo/rustc,
echo then re-run this script to continue with the MSVC Build Tools step.
pause
exit /b 0

:rust_failed
echo.
echo ERROR: Rust install failed. Install manually from https://rustup.rs and re-run this script.
pause
exit /b 1

:check_msvc
echo.
echo [2/3] MSVC C++ Build Tools (x86_64 + ARM64)...
where cl >nul 2>&1
if errorlevel 1 goto :install_msvc
echo MSVC Build Tools already installed, skipping.
goto :add_targets

:install_msvc
echo Installing Visual Studio Build Tools (C++ workload + ARM64 tools) via winget.
echo This downloads several GB and can take a while -- please wait, do not close this window.
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --accept-source-agreements --accept-package-agreements --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --add Microsoft.VisualStudio.Component.VC.Tools.ARM64 --includeRecommended"
if errorlevel 1 goto :msvc_failed
echo.
echo IMPORTANT: close and reopen this terminal (or restart) so the build tools
echo are on PATH, then re-run this script to finish setting up cross targets.
pause
exit /b 0

:msvc_failed
echo.
echo ERROR: Build Tools install failed. Install manually instead:
echo   https://visualstudio.microsoft.com/visual-cpp-build-tools/
echo Select "Desktop development with C++", plus the individual component
echo "MSVC v143 - VS 2022 C++ ARM64 build tools".
pause
exit /b 1

:add_targets
echo.
echo [3/3] Rust cross-compilation targets...
rustup target add x86_64-pc-windows-msvc >nul 2>&1
rustup target add aarch64-pc-windows-msvc >nul 2>&1

echo.
echo Optional: for the native LLVM backend (--features llvm), you also need
echo LLVM 21 -- see README.md for the LLVM_SYS_210_PREFIX setup. Not required
echo for the default build or for scripts\build_installer.ps1.
echo.
echo === Done. You can now run scripts\build_installer.ps1 ===
pause
exit /b 0

:no_winget
echo.
echo ERROR: winget not found.
echo Install "App Installer" from the Microsoft Store, then re-run this script:
echo   https://apps.microsoft.com/detail/9nblggh4nns1
pause
exit /b 1
