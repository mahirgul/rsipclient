# rsipclient PowerShell single-line installer for Windows
$ErrorActionPreference = "Stop"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "   Installing rsipclient (sip-client)    " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

# Check for Rust/Cargo
if ((Get-Command "cargo" -ErrorAction SilentlyContinue) -eq $null) {
    Write-Host "Rust/Cargo was not found. Installing Rust toolchain..." -ForegroundColor Yellow
    
    $url = "https://win.rustup.rs/x86_64"
    $output = "$env:TEMP\rustup-init.exe"
    
    Write-Host "Downloading rustup-init..."
    Invoke-WebRequest -Uri $url -OutFile $output
    
    Write-Host "Running rustup-init (automatic installation)..."
    Start-Process -FilePath $output -ArgumentList "-y" -Wait
    Remove-Item $output
    
    # Refresh Path variable in the current session
    $env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path","User")
}

# Verify cargo is now available
if ((Get-Command "cargo" -ErrorAction SilentlyContinue) -eq $null) {
    Write-Warning "Cargo is still not in your PATH. Please restart your PowerShell session and run this script again."
    exit 1
}

Write-Host "Compiling and installing rsipclient from GitHub..." -ForegroundColor Cyan
cargo install --git https://github.com/mahirgul/rsipclient.git --force

Write-Host ""
Write-Host "=========================================" -ForegroundColor Green
Write-Host " rsipclient successfully installed!" -ForegroundColor Green
Write-Host " Run 'sip-client --help' to get started." -ForegroundColor Green
Write-Host "=========================================" -ForegroundColor Green
