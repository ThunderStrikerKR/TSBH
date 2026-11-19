@echo off
setlocal
cd /d "%~dp0"
title BANDO HUB - egui UI Preview

where cargo >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Rust Cargo was not found in PATH.
  echo Install the Rust toolchain and reopen this command window.
  pause
  exit /b 1
)

echo.
echo BANDO HUB egui UI Preview - Isolated from the production database
echo Existing React/Tauri code and SQLite data are NOT modified.
echo.
cargo run --manifest-path "egui-app\Cargo.toml"
if errorlevel 1 (
  echo.
  echo [ERROR] egui preview build or startup failed.
  pause
  exit /b 1
)
endlocal
