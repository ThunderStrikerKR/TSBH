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

rem Reuse an EXISTING, packaged BANDO OCR runtime without copying or
rem modifying any React/Tauri files. An explicit user override wins.
if not defined BANDO_OCR_RUNTIME_DIR (
  for %%D in (
    "%~dp0ocr"
    "%~dp0src-tauri\target\release\ocr"
    "%~dp0src-tauri\target\debug\ocr"
    "%~dp0release\win-unpacked\ocr"
  ) do (
    if not defined BANDO_OCR_RUNTIME_DIR (
      if exist "%%~fD\runtime\python.exe" (
        if exist "%%~fD\models\PP-OCRv5_mobile_det" (
          if exist "%%~fD\models\korean_PP-OCRv5_mobile_rec" (
            set "BANDO_OCR_RUNTIME_DIR=%%~fD"
          )
        )
      )
    )
  )
)
if defined BANDO_OCR_RUNTIME_DIR (
  echo [OCR] Using existing runtime: %BANDO_OCR_RUNTIME_DIR%
) else (
  echo [OCR] No packaged OCR runtime found. Text clipboard intake works;
  echo       image intake needs the existing BANDO OCR runtime/models.
)
echo.
cargo run --manifest-path "egui-app\Cargo.toml"
if errorlevel 1 (
  echo.
  echo [ERROR] egui preview build or startup failed.
  pause
  exit /b 1
)
endlocal
