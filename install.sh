#!/bin/sh
# rsipclient Linux/macOS single-line installer
set -e

echo "========================================="
echo "   Installing rsipclient (sip-client)    "
echo "========================================="

VERSION="v0.2.2"
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
    darwin)
        OS_NAME="macos"
        ;;
    linux)
        OS_NAME="linux"
        ;;
    *)
        echo "Unsupported OS: $OS"
        exit 1
        ;;
esac

case "$ARCH" in
    x86_64|amd64)
        ARCH_NAME="x86_64"
        ;;
    arm64|aarch64)
        ARCH_NAME="aarch64"
        ;;
    *)
        echo "Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

BINARY_NAME="sip-client-${OS_NAME}-${ARCH_NAME}"
URL="https://github.com/mahirgul/rsipclient/releases/download/${VERSION}/${BINARY_NAME}"
INSTALL_DIR="$HOME/.rsipclient/bin"
mkdir -p "$INSTALL_DIR"

echo "Downloading ${BINARY_NAME} from ${URL}..."
curl -L -o "$INSTALL_DIR/sip-client" "${URL}"
chmod +x "$INSTALL_DIR/sip-client"

echo ""
echo "========================================="
echo " rsipclient successfully installed to:"
echo "   $INSTALL_DIR/sip-client"
echo ""
echo " Please make sure this directory is in your PATH."
echo " For example, add the following to your shell profile (~/.bashrc or ~/.zshrc):"
echo "   export PATH=\"\$PATH:\$HOME/.rsipclient/bin\""
echo "========================================="
