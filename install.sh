#!/bin/sh
# rsipclient Linux/macOS single-line installer
set -e

echo "========================================="
echo "   Installing rsipclient (sip-client)    "
echo "========================================="

VERSION="v0.2.3"
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

# Create a basic config.toml if not exists
CONFIG_PATH="$HOME/.rsipclient/config.toml"
if [ ! -f "$CONFIG_PATH" ]; then
    cat << 'EOF' > "$CONFIG_PATH"
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
EOF
    echo "Created default configuration file at: $CONFIG_PATH"
fi

echo ""
echo "========================================="
echo " rsipclient successfully installed to:"
echo "   $INSTALL_DIR/sip-client"
echo "========================================="
echo "Config Path: $CONFIG_PATH"
echo ""
echo "To run the client in service mode manually:"
echo "  $INSTALL_DIR/sip-client -c $CONFIG_PATH service"
echo ""
if [ "$OS_NAME" = "linux" ]; then
    echo "To install and run rsipclient as a systemd service:"
    echo "1. Create a service file: /etc/systemd/system/rsipclient.service"
    echo "   with the following content (replace '$(whoami)' if needed):"
    echo ""
    echo "   [Unit]"
    echo "   Description=rsipclient SIP Client Service"
    echo "   After=network.target"
    echo ""
    echo "   [Service]"
    echo "   Type=simple"
    echo "   ExecStart=$INSTALL_DIR/sip-client -c $CONFIG_PATH service"
    echo "   Restart=on-failure"
    echo "   User=$(whoami)"
    echo ""
    echo "   [Install]"
    echo "   WantedBy=multi-user.target"
    echo ""
    echo "2. Reload systemd, enable and start the service:"
    echo "   sudo systemctl daemon-reload"
    echo "   sudo systemctl enable rsipclient"
    echo "   sudo systemctl start rsipclient"
elif [ "$OS_NAME" = "macos" ]; then
    echo "To run rsipclient in the background on macOS:"
    echo "  You can run it using screen, tmux, or launchd configuration."
fi
echo "========================================="
