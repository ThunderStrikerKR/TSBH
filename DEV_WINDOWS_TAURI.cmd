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

echo.
echo [DEV] Starting Tauri + Vite in isolated developer TEST mode...
echo [DEV] React edits update automatically; Rust edits rebuild incrementally.
echo [DEV] Close the app and press Ctrl+C here to stop the dev server.
echo.
rem Tauri 2: first "--" separates runner args; second "--" passes
rem --bando-developer-test directly to the application.
call "node_modules\.bin\tauri.cmd" dev -- -- --bando-developer-test
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
pause
exit /b 1
