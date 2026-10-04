@echo off
fltmc >nul 2>&1
if errorlevel 1 (
    echo Right-click this file and choose Run as administrator.
    pause
    exit /b 1
)
"%SystemRoot%\System32\regsvr32.exe" /s /u "%~dp0UnityCaptureFilter64.dll"
if errorlevel 1 goto failed
"%SystemRoot%\SysWOW64\regsvr32.exe" /s /u "%~dp0UnityCaptureFilter32.dll"
if errorlevel 1 goto failed
echo Uninstalled Unity Video Capture. You can now move or delete this folder.
pause
exit /b 0
:failed
echo Camera unregistration failed. Keep the DLL files until this is resolved.
pause
exit /b 1
