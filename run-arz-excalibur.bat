@echo off
cd /d "%~dp0"
echo Installing ARZ-Excalibur into Adobe Premiere Pro (No Windows background startup)...
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-premiere-bridge.ps1"
echo.
echo Done! Open Adobe Premiere Pro and press Alt+Space (or open Window ^> Extensions ^> ARZ-Excalibur Settings).
pause
