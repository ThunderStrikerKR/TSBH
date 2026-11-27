@echo off
setlocal
cd /d "%~dp0"
title BANDO HUB - Windows Release Packaging
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\package-windows-egui.ps1"
set "RESULT=%ERRORLEVEL%"
if not "%RESULT%"=="0" echo [ERROR] Packaging failed. Previous ZIP preserved.
if /I not "%GITHUB_ACTIONS%"=="true" pause
exit /b %RESULT%
