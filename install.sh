#!/bin/sh
# rsipclient Linux/macOS single-line installer
set -e

echo "========================================="
echo "   Installing rsipclient (sip-client)    "
echo "========================================="

# Check for Rust/Cargo
if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust/Cargo was not found. Installing Rust toolchain via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    
    # Load Cargo environment
    if [ -f "$HOME/.cargo/env" ]; then
        . "$HOME/.cargo/env"
    else
        export PATH="$HOME/.cargo/bin:$PATH"
    fi
fi

# Confirm cargo is now available
if ! command -v cargo >/dev/null 2>&1; then
    echo "Error: Rust toolchain installation failed or Cargo is not in PATH."
    exit 1
fi

echo "Compiling and installing rsipclient from GitHub..."
cargo install --git https://github.com/mahirgul/rsipclient.git --force

echo ""
echo "========================================="
echo " rsipclient successfully installed!"
echo " Run 'sip-client --help' to get started."
echo "========================================="
