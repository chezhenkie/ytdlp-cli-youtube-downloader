@echo off
setlocal
python "%~dp0src\ytdlp.py" %*
if errorlevel 1 pause
endlocal
