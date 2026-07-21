# Raspberry Pi & Linux Headless (Console) Deployment Guide

This guide explains how to deploy `rsipclient` (`sip-client`) on Linux devices such as the Raspberry Pi, Orange Pi, Intel NUC, or Linux Servers without a graphical desktop (GUI / X11 / Wayland).

---

## 1. Features in Headless Mode

- **Zero GUI Dependencies**: Does not require X11, Wayland, GTK, or desktop environments.
- **ALSA Sound Card Access**: Directly binds to USB sound cards, 3.5mm audio jacks, or I2S Audio HATs at the kernel/ALSA level.
- **Background Daemon**: Can be managed as a standard `systemd` system service.
- **Syslog Forwarding**: Logs can be forwarded to local `/dev/log` or remote Syslog daemons (rsyslog, syslog-ng, systemd-journald).
- **Web Softphone & Dashboard**: Control accounts and stream bi-directional audio remotely from any web browser on the network.

---

## 2. Sound Card Setup (ALSA / Linux)

List available audio input/output devices on your Raspberry Pi:

```bash
# List capture devices (microphones)
arecord -l

# List playback devices (speakers/headphones)
aplay -l
```

### `config.toml` Audio Configuration

Specify hardware sound cards per account:

```toml
[[accounts]]
name = "pi-intercom"
username = "1001"
password = "mysecretpassword"
server = "192.168.1.10:5060"
domain = "192.168.1.10"

# Sound Card Devices (ALSA / System Default)
audio_input_device = "default"    # Or specific ALSA hw device, e.g., "hw:1,0"
audio_output_device = "default"   # Or specific ALSA hw device, e.g., "hw:0,0"

# Auto-answer as a door intercom / voice box
auto_answer = true
ivr_welcome = "audio/welcome.wav"
```

---

## 3. Syslog Integration

Forward service logs to local system log (`/dev/log`) or a remote Syslog server:

```toml
[syslog]
enabled = true
server = "/dev/log"       # Or remote server e.g. "192.168.1.50:514"
protocol = "unix"         # "unix" for local /dev/log socket, "udp" or "tcp" for network
facility = "user"         # "user", "local0", "daemon", etc.
app_name = "rsipclient"
```

Inspect logs using `journalctl` or `rsyslog`:

```bash
# View live logs via journalctl
journalctl -t rsipclient -f
```

---

## 4. Systemd Auto-Start Service

Create a systemd unit file to run `sip-client` automatically on boot:

```bash
sudo nano /etc/systemd/system/rsipclient.service
```

Paste the following configuration:

```ini
[Unit]
Description=RSIPClient SIP Daemon & IVR Engine
After=network.target sound.target syslog.target

[Service]
Type=simple
User=pi
WorkingDirectory=/home/pi/rsipclient
ExecStart=/home/pi/rsipclient/target/release/sip-client -c /home/pi/rsipclient/config.toml service
Restart=always
RestartSec=5
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

Enable and start the service:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now rsipclient
```

Check service status:

```bash
sudo systemctl status rsipclient
```

---

## 5. Controlling the Service via CLI / IPC

Even without a GUI, you can control the running daemon using CLI commands:

```bash
# Make an outbound call
sip-client call -a pi-intercom -t sip:2001@192.168.1.10

# Play a WAV file over an active call
sip-client play -a pi-intercom -f audio/announcement.wav

# Send DTMF tones
sip-client dtmf -a pi-intercom -d "1234#"

# Hang up
sip-client hangup -a pi-intercom

# Check account status
sip-client status
```
