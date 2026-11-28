@echo off
setlocal
cd /d "%~dp0"
rem Optional production launcher: the EXE initializes missing write flags after DB validation.
set "BANDO_HUB_GATEWAY=local"
set "BANDO_EGUI_BUSINESS_DB="
set "BANDO_EGUI_CLIENT_INDEX="
set "BANDO_EGUI_DEVELOPER_TEST="
set "BANDO_EGUI_UI_SAMPLE_DATA=0"
set "BANDO_EGUI_ENABLE_WORKING_COMMIT="
set "BANDO_OCR_RUNTIME_DIR=%~dp0ocr"
if not exist "%~dp0BANDO-HUB.exe" (
    echo [ERROR] Extract the entire release ZIP before running.
    pause
    exit /b 1
)
if not exist "%APPDATA%\com.bandostand.bandohub.tauri\bando-hub-v001.sqlite" (
    echo [ERROR] Existing production database not found. No database was created.
    pause
    exit /b 1
)
start "" "%~dp0BANDO-HUB.exe"
exit /b %ERRORLEVEL%
