@echo off
rem Explicit Windows Release EXE only. No portable OCR ZIP packaging.
call "%~dp0BUILD_WINDOWS_TAURI.cmd" --exe-only
exit /b %errorlevel%
