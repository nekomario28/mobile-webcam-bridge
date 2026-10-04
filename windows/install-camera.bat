@echo off
fltmc >nul 2>&1
if errorlevel 1 (
    echo Right-click this file and choose Run as administrator.
    pause
    exit /b 1
)
"%SystemRoot%\System32\regsvr32.exe" /s "%~dp0UnityCaptureFilter64.dll"
if errorlevel 1 goto failed
"%SystemRoot%\SysWOW64\regsvr32.exe" /s "%~dp0UnityCaptureFilter32.dll"
if errorlevel 1 goto failed
echo Installed Unity Video Capture. Keep this folder in its current location.
pause
exit /b 0
:failed
echo Camera registration failed.
pause
exit /b 1
