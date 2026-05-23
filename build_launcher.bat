@echo off
title DynamicLauncher - Build
color 0A

echo ============================================
echo   DynamicLauncher - Rust Build
echo ============================================
echo.

where cargo >nul 2>&1
if %errorlevel% neq 0 (
    echo ERROR: Rust not found. Install from https://rustup.rs
    pause
    exit /b 1
)

echo [1/2] Compiling DynamicLauncher in release mode...
echo       (First build takes 5-10 mins, future builds will be fast)
echo.

cargo build --release

if %errorlevel% neq 0 (
    echo.
    echo ERROR: Build failed. Paste the error above to Claude or ChatGPT.
    pause
    exit /b 1
)

echo.
echo [2/2] Copying dynamiclauncher.exe here...
copy /Y target\release\dynamiclauncher.exe dynamiclauncher.exe

echo.
echo ============================================
echo   DynamicLauncher is ready!
echo   Run: dynamiclauncher.exe
echo   Hotkey: Ctrl + Space
echo   DONE !
echo ============================================
pause
