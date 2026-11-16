@echo off
setlocal
chcp 65001 >nul
cd /d "%~dp0"

echo ============================================================
echo BANDO HUB - DAILY DEVELOPMENT (NO RELEASE EXE OR ZIP)
echo ============================================================
echo This window runs Tauri development mode with React hot reload.
echo All development orders use the separate developer TEST database.
echo No release or portable ZIP files will be created.
echo.

where node >nul 2>nul
if errorlevel 1 goto missing_node
where npm >nul 2>nul
if errorlevel 1 goto missing_node
where cargo >nul 2>nul
if errorlevel 1 goto missing_rust
where powershell >nul 2>nul
if errorlevel 1 goto failed

powershell -NoProfile -ExecutionPolicy Bypass -File "scripts\ensure-node-deps.ps1"
if errorlevel 1 goto failed

rem OCR can be tested with the ALREADY PACKAGED runtime; no downloads or
rem repackaging on each development launch. A Windows directory junction
rem allows target\debug\bando_hub_tauri.exe to find its adjacent OCR files.
if exist "release\ocr\runtime\python.exe" (
  if not exist "src-tauri\target\debug" mkdir "src-tauri\target\debug"
  if not exist "src-tauri\target\debug\ocr\runtime\python.exe" (
    if not exist "src-tauri\target\debug\ocr" (
      mklink /J "src-tauri\target\debug\ocr" "%CD%\release\ocr" >nul
      if errorlevel 1 echo [DEV] Could not link existing OCR runtime; OCR capture may be unavailable.
    )
  )
) else (
  echo [DEV] No existing portable OCR runtime. Text orders work; OCR capture needs release\ocr.
)

echo.
echo [DEV] Starting Tauri + Vite in isolated developer TEST mode...
echo [DEV] React edits update automatically; Rust edits rebuild incrementally.
echo [DEV] Close the app and press Ctrl+C here to stop the dev server.
echo.
rem Tauri 2: first "--" separates runner args; second "--" passes
rem --bando-developer-test directly to the application.
if /I "%GITHUB_ACTIONS%"=="true" if /I "%BANDO_DEV_GUI_AUDIT%"=="true" (
  echo [DEV] CI-only WebView2 inspection enabled for real business workflow GUI tests.
  call "node_modules\.bin\tauri.cmd" dev --config src-tauri/tauri.ci-cdp.conf.json -- -- --bando-developer-test
) else (
  call "node_modules\.bin\tauri.cmd" dev -- -- --bando-developer-test
)
if errorlevel 1 goto failed
exit /b 0

:missing_node
echo ERROR: Node.js/npm is not installed or not on PATH.
goto failed
:missing_rust
echo ERROR: Rust/cargo is not installed or not on PATH.
goto failed
:failed
echo.
echo Development launch failed. Inspect the error above.
rem CI cannot answer "Press any key to continue". Exit with a nonzero status
rem so the real Windows developer test reports its actual startup failure.
if /I "%GITHUB_ACTIONS%"=="true" exit /b 1
pause
exit /b 1
