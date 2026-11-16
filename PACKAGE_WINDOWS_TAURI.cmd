@echo off
rem Explicit distribution build: EXE + verified private OCR runtime + ZIP.
rem This is NEVER run by the daily development launcher.
call "%~dp0BUILD_WINDOWS_TAURI.cmd"
exit /b %errorlevel%
