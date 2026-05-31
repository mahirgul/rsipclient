# rsipclient PowerShell single-line installer for Windows
$ErrorActionPreference = "Stop"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "   Installing rsipclient (sip-client)    " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

$version = "v0.2.2"
$arch = "x86_64"
if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64" -or $env:PROCESSOR_ARCHITEW6432 -eq "ARM64") {
    $arch = "aarch64"
}

$installDir = "$HOME\.rsipclient\bin"
if (!(Test-Path $installDir)) {
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
}

$binaryName = "sip-client.exe"
$destPath = Join-Path $installDir $binaryName

# Download URL
$url = "https://github.com/mahirgul/rsipclient/releases/download/$version/sip-client-windows-$arch.exe"

Write-Host "Downloading pre-compiled binary from $url..." -ForegroundColor Yellow

# Download file
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Invoke-WebRequest -Uri $url -OutFile $destPath

# Add to PATH
$userPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath.Split(';') -notcontains $installDir) {
    [System.Environment]::SetEnvironmentVariable("Path", "$userPath;$installDir", "User")
    $env:Path = "$env:Path;$installDir"
    Write-Host "Added $installDir to User PATH." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "=========================================" -ForegroundColor Green
Write-Host " rsipclient successfully installed!" -ForegroundColor Green
Write-Host " Run 'sip-client --help' in a new window." -ForegroundColor Green
Write-Host "=========================================" -ForegroundColor Green
