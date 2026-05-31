# rsipclient PowerShell single-line installer for Windows
$ErrorActionPreference = "Stop"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "   Installing rsipclient (sip-client)    " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

$version = "v0.2.3"
$arch = "x86_64"
if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64" -or $env:PROCESSOR_ARCHITEW6432 -eq "ARM64") {
    $arch = "aarch64"
}

$installDir = "$HOME\.rsipclient"
$binDir = "$installDir\bin"
if (!(Test-Path $binDir)) {
    New-Item -ItemType Directory -Path $binDir -Force | Out-Null
}

$binaryName = "sip-client.exe"
$destPath = Join-Path $binDir $binaryName

# Download URL
$url = "https://github.com/mahirgul/rsipclient/releases/download/$version/sip-client-windows-$arch.exe"

Write-Host "Downloading pre-compiled binary from $url..." -ForegroundColor Yellow

# Download file
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Invoke-WebRequest -Uri $url -OutFile $destPath

# Add to PATH
$userPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath.Split(';') -notcontains $binDir) {
    [System.Environment]::SetEnvironmentVariable("Path", "$userPath;$binDir", "User")
    $env:Path = "$env:Path;$binDir"
    Write-Host "Added $binDir to User PATH." -ForegroundColor Yellow
}

# Create a basic config.toml if not exists
$configPath = "$installDir\config.toml"
if (!(Test-Path $configPath)) {
    $configContent = @"
# rsipclient Basic Configuration File

[web]
port = 9090
username = "admin"
password = "admin" # Change this password!

[commands_api]
port = 9099

[[accounts]]
name = "default"
username = "your_username"
password = "your_password"
domain = "sip.yourprovider.com"
server = "sip.yourprovider.com:5060"
sip_port = 5060
rtp_port_start = 8000
rtp_port_end = 8010
auth_method = "md5"
codec = "pcmu"
auto_answer = true
"@
    Set-Content -Path $configPath -Value $configContent
    Write-Host "Created default configuration file at: $configPath" -ForegroundColor Green
}

Write-Host ""
Write-Host "=========================================" -ForegroundColor Green
Write-Host " rsipclient successfully installed!" -ForegroundColor Green
Write-Host "=========================================" -ForegroundColor Green
Write-Host "Config Path: $configPath" -ForegroundColor Yellow
Write-Host ""
Write-Host "To run the client in service mode manually:" -ForegroundColor Cyan
Write-Host "  sip-client -c `"$configPath`" service"
Write-Host ""
Write-Host "To install and run rsipclient as a Windows Service:" -ForegroundColor Cyan
Write-Host "1. Download NSSM (Non-Sucking Service Manager) from https://nssm.cc/"
Write-Host "2. Run the following command in an Administrator command prompt:"
Write-Host "     nssm install rsipclient `"$binDir\sip-client.exe`" -c `"$configPath`" service"
Write-Host "3. Start the service:"
Write-Host "     nssm start rsipclient"
Write-Host ""
Write-Host "Alternatively, to run in background via Task Scheduler (without NSSM):" -ForegroundColor Cyan
Write-Host "  Register a basic Task Scheduler job that triggers on startup and executes:"
Write-Host "  `"$binDir\sip-client.exe`" with arguments `"-c `"$configPath`" service`""
Write-Host "=========================================" -ForegroundColor Green
