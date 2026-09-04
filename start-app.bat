@echo off
setlocal enabledelayedexpansion

:: Kill any process occupying the Vite dev port (1420)
netstat -ano | findstr /r ":1420.*LISTENING" > "%TEMP%\port1420.txt"
if exist "%TEMP%\port1420.txt" (
    for /f "tokens=5" %%a in (%TEMP%\port1420.txt) do (
        echo [start-app] Port 1420 in use by PID %%a - terminating...
        taskkill /F /PID %%a >nul 2>&1
        if !errorlevel! equ 0 (
            echo [start-app] Successfully killed PID %%a
        ) else (
            echo [start-app] Failed to kill PID %%a - will try to start anyway
        )
    )
    del "%TEMP%\port1420.txt" >nul 2>&1
)

:: Give the socket a moment to release
timeout /t 1 /nobreak >nul

:: Kill any stale app instance left over from a previous dev run. A leftover
:: process would hold its in-memory refresh token while the new instance
:: rotates it, causing mutual invalidation and repeated re-login prompts.
taskkill /F /IM share-one-list.exe >nul 2>&1
if !errorlevel! equ 0 (
    echo [start-app] Terminated stale share-one-list.exe instance
)

cd /d "%~dp0tauri-app"
echo [start-app] Starting Tauri dev server...
npm start
pause
