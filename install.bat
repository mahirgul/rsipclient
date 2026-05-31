@echo off
REM rsipclient CMD/batch installer for Windows

echo =========================================
echo    Installing rsipclient (sip-client)    
echo =========================================

powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/mahirgul/rsipclient/master/install.ps1 | iex"
