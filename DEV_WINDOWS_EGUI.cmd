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
rem Fail closed: never inherit an arbitrary/stale helper or freight-write
rem permission. An old bando_hub_tauri.exe may ignore the shipping CLI flag
rem and open a second Tauri WebView at 127.0.0.1:5173 instead of saving.
set "BANDO_EGUI_TAURI_HELPER="
set "BANDO_EGUI_ENABLE_SHIPPING_WRITE=0"
rem Build the CURRENT Rust CLI before enabling the isolated developer DB
rem bridge. Cargo uses its incremental cache; it only rebuilds changed code.
rem A failed build leaves the UI usable for reads, but prevents spawning the
rem stale executable. Do not launch a web server or touch production SQLite.
if exist "%APPDATA%\com.bandostand.bandohub.tauri\developer-test\bando-hub-test.sqlite" (
  echo [FREIGHT] Checking native shipping helper build...
  cargo build --quiet --manifest-path "src-tauri\Cargo.toml" --bin bando_hub_tauri
  if errorlevel 1 (
    echo [FREIGHT] Native helper build failed. Shipping changes are disabled.
    echo [FREIGHT] Old helper will NOT be launched.
  ) else (
    if exist "%~dp0src-tauri\target\debug\bando_hub_tauri.exe" (
      set "BANDO_EGUI_TAURI_HELPER=%~dp0src-tauri\target\debug\bando_hub_tauri.exe"
      set "BANDO_EGUI_ENABLE_SHIPPING_WRITE=1"
      echo [FREIGHT] Current native helper ready for isolated TEST SQLite.
    ) else (
      echo [FREIGHT] Compiled helper missing. Shipping changes are disabled.
    )
  )
) else (
  echo [FREIGHT] No developer-test SQLite found. Production DB will NOT be used.
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
