@echo off
setlocal
color 1F
python "%~dp0src\ytdlp.py" %*
if errorlevel 1 pause
endlocal
