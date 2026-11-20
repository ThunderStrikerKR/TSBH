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
echo BANDO HUB egui - isolated Tauri developer-test connection
echo Existing React/Tauri code and production SQLite data are NOT modified.
echo.

rem The egui developer launcher only reads the EXISTING Tauri developer-test
rem DB. Do not inherit an unrelated operating DB or JSON client index from
rem another command prompt. Direct cargo invocations may still use overrides.
rem Never create/reset/attach the production DB from this developer launcher.
set "BANDO_EGUI_BUSINESS_DB="
set "BANDO_EGUI_CLIENT_INDEX="
set "BANDO_EGUI_DEVELOPER_TEST=1"
rem Freight writes use the shared in-process Rust service: no helper EXE.
rem This developer launcher enables writes ONLY for an existing isolated TEST DB.
rem Backups, restores and audit archive use the in-process Rust service.
rem Unconfirmed reception writes also use the in-process Rust CAS service.
rem Client and product edits use the native shared writer, NEVER egui preview DB.
set "BANDO_EGUI_ENABLE_CLIENT_WRITE=0"
set "BANDO_EGUI_ENABLE_PRODUCT_WRITE=0"
set "BANDO_EGUI_ENABLE_SHIPPING_WRITE=0"
set "BANDO_EGUI_ENABLE_SETTINGS_WRITE=0"
set "BANDO_EGUI_ENABLE_EXCEL_WRITE=0"
set "BANDO_EGUI_ENABLE_WORKING_COMMIT=0"
rem Explicit one-time UI-density fixtures ONLY in the pre-existing Tauri developer TEST DB.
rem Production, HTTP and standalone preview modes never receive synthetic data.
set "BANDO_EGUI_UI_SAMPLE_DATA=0"
if exist "%APPDATA%\com.bandostand.bandohub.tauri\developer-test\bando-hub-test.sqlite" (
  set "BANDO_EGUI_ENABLE_SHIPPING_WRITE=1"
  set "BANDO_EGUI_ENABLE_CLIENT_WRITE=1"
  set "BANDO_EGUI_ENABLE_PRODUCT_WRITE=1"
  set "BANDO_EGUI_ENABLE_SETTINGS_WRITE=1"
  set "BANDO_EGUI_ENABLE_EXCEL_WRITE=1"
  set "BANDO_EGUI_ENABLE_WORKING_COMMIT=1"
  set "BANDO_EGUI_UI_SAMPLE_DATA=1"
  echo [FREIGHT/CLIENT/PRODUCT/SETTINGS/RECEPTION] Native Rust direct save enabled for the existing isolated TEST DB.
  echo [UI SAMPLE] Synthetic groups, aliases, brands, clients, combinations and rate history: one-time insert.
) else (
  echo [FREIGHT] No developer-test SQLite found. The production DB will NOT be used.
  echo [UI SAMPLE] Create/open the isolated test workspace in DEV_WINDOWS_TAURI.cmd first.
)

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
