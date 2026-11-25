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
echo BANDO HUB egui - isolated TEST execution
echo Production SQLite is never selected by this launcher.
echo.
set "BANDO_HUB_GATEWAY=local"
set "BANDO_EGUI_BUSINESS_DB="
set "BANDO_EGUI_CLIENT_INDEX="
set "BANDO_EGUI_DEVELOPER_TEST=1"
rem New workspaces use only the bundled product seed, not production data.
rem Samples are opt-in for existing workspaces, automatic for a new workspace.
set "BANDO_EGUI_UI_SAMPLE_DATA=0"
if not exist "%APPDATA%\com.bandostand.bandohub.tauri\developer-test\bando-hub-test.sqlite" set "BANDO_EGUI_UI_SAMPLE_DATA=1"
if /I "%~1"=="samples" set "BANDO_EGUI_UI_SAMPLE_DATA=1"
set "BANDO_EGUI_ENABLE_CLIENT_WRITE=1"
set "BANDO_EGUI_ENABLE_PRODUCT_WRITE=1"
set "BANDO_EGUI_ENABLE_SHIPPING_WRITE=1"
set "BANDO_EGUI_ENABLE_SETTINGS_WRITE=1"
set "BANDO_EGUI_ENABLE_EXCEL_WRITE=1"
set "BANDO_EGUI_ENABLE_WORKING_COMMIT=1"

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
cargo run --manifest-path "egui-app\Cargo.toml" --bin bando-hub-egui
if errorlevel 1 (
  echo.
  echo [ERROR] egui preview build or startup failed.
  pause
  exit /b 1
)
endlocal
