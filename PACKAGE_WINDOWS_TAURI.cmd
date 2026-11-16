@echo off
setlocal
chcp 65001 >nul
cd /d "%~dp0"
echo ============================================================
echo BANDO HUB - MANUAL RELEASE EXE + VERIFIED OFFLINE OCR ZIP
echo ============================================================
echo This launcher is for distribution ONLY. Daily development uses
echo DEV_WINDOWS_TAURI.cmd and never packages a release.
echo.
where powershell >nul 2>nul
if errorlevel 1 (
  echo ERROR: Windows PowerShell is required.
  exit /b 1
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\package-windows-tauri.ps1"
if errorlevel 1 (
  echo ERROR: Manual release packaging failed; previous distributable is preserved.
  if /I not "%GITHUB_ACTIONS%"=="true" pause
  exit /b 1
)
echo.
echo [RELEASE] Verified portable ZIP created.
exit /b 0
