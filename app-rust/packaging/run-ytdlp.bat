@echo off
setlocal
"%~dp0ytdlp.exe" %*
if errorlevel 1 pause
endlocal
