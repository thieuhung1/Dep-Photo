@echo off
title DEP_PHOTO Launcher
cd /d "%~dp0"
set "PATH=C:\Users\thieu\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin;%PATH%"
echo ==============================================
echo           DEP_PHOTO - Photo Editor
echo ==============================================
echo Dang khoi dong ung dung...

if exist "target\release\dep_photo.exe" (
    start "" "target\release\dep_photo.exe"
    exit /b 0
)

if exist "target\debug\dep_photo.exe" (
    start "" "target\debug\dep_photo.exe"
    exit /b 0
)

echo Chua tim thay file .exe san, dang bien dich va chay qua cargo...
cargo run --bin dep_photo
