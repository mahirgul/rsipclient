@echo off
REM rsipclient CMD/batch installer for Windows

echo =========================================
echo    Installing rsipclient (sip-client)    
echo =========================================

where cargo >nul 2>nul
if %errorlevel% neq 0 (
    echo Rust/Cargo was not found. Installing Rust toolchain...
    
    powershell -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile '%TEMP%\rustup-init.exe'"
    
    echo Running rustup-init (automatic installation)...
    "%TEMP%\rustup-init.exe" -y
    del "%TEMP%\rustup-init.exe"
    
    echo Please restart your Command Prompt or run 'refreshenv' if you have Chocolatey installed, then run this installer again.
    echo Cargo must be available in your PATH.
    exit /b 1
)

echo Compiling and installing rsipclient from GitHub...
cargo install --git https://github.com/mahirgul/rsipclient.git --force

echo.
echo =========================================
echo  rsipclient successfully installed!
echo  Run 'sip-client --help' to get started.
echo =========================================
