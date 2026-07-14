@echo off
setlocal
cd /d "%~dp0"

cargo build --release
if errorlevel 1 exit /b 1

set OUT=%~dp0sdcs-grpc

if exist "%OUT%" rmdir /S /Q "%OUT%"
mkdir "%OUT%\Mods\tech\DCS-gRPC"
mkdir "%OUT%\Scripts\DCS-gRPC"
mkdir "%OUT%\Scripts\Hooks"

copy /Y "target\release\dcs_grpc.dll" "%OUT%\Mods\tech\DCS-gRPC\dcs_grpc.dll"
if errorlevel 1 exit /b 1

xcopy /YSQ "lua\DCS-gRPC\*" "%OUT%\Scripts\DCS-gRPC\"
if errorlevel 1 exit /b 1

xcopy /YSQ "lua\Hooks\*" "%OUT%\Scripts\Hooks\"
if errorlevel 1 exit /b 1

echo.
echo Packaged to: %OUT%
endlocal
