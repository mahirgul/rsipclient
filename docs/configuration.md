# Configuration Reference

Complete reference for `config.toml`.

## Top-level structure

```toml
[[accounts]]
name = "my-account"
# ... per-account fields ...
```

## Account fields

### Required

| Field | Type | Example | Description |
|-------|------|---------|-------------|
| `name` | string | `"alice"` | Unique account identifier |
| `username` | string | `"alice"` | SIP username |
| `password` | string | `"secret"` | SIP password |
| `domain` | string | `"sip.example.com"` | SIP domain |
| `server` | string | `"192.168.1.1:5060"` | SIP server `host:port` |

### Optional — Network

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `sip_port` | u16 | `0` (auto) | Local SIP port |
| `rtp_port_start` | u16 | — | RTP port range start |
| `rtp_port_end` | u16 | — | RTP port range end |
| `proxy` | string | — | Outbound proxy `host:port` |

### Optional — Identity

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `display_name` | string | — | From header display name |
| `asserted_id` | URI | — | P-Asserted-Identity header |
| `preferred_id` | URI | — | P-Preferred-Identity header |

### Optional — Protocol

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `auth_method` | `"md5"` / `"none"` | `"md5"` | Authentication |
| `codec` | `"pcmu"` / `"pcma"` / `"opus"` | `"pcmu"` | Audio codec |
| `transport` | `"udp"` / `"tcp"` / `"tls"` | `"udp"` | SIP transport protocol |
| `register_expiry` | u32 | `3600` | REGISTER expiry (seconds) |
| `user_agent` | string | — | Custom User-Agent |
| `dtmf_mode` | `"rfc2833"` / `"inband"` / `"info"` | — | DTMF signalling |
| `early_media` | bool | `true` | 183 Session Progress |
| `session_timers` | bool | `false` | RFC 4028 |

### Optional — Audio Hardware & Sound Cards

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `audio_input_device` | string | `"default"` | ALSA/Hardware microphone device |
| `audio_output_device` | string | `"default"` | ALSA/Hardware speaker/headset device |

### Optional — Syslog Logging

```toml
[syslog]
enabled = true            # Enable Syslog forwarding
server = "127.0.0.1:514"  # Host:port or "/dev/log" on Linux
protocol = "udp"          # "udp", "tcp", or "unix"
facility = "user"         # "user", "local0", "daemon", etc.
app_name = "rsipclient"   # Application tag
```

### Optional — IVR / Auto-Attendant

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `auto_answer` | bool | `false` | Auto-answer incoming INVITEs |
| `ivr_welcome` | path | — | WAV file to play on answer |
| `ivr_timeout` | u64 | `10` | DTMF timeout (seconds) |
| `ivr_menu` | table | — | Digit → action map |
| `ivr_default` | string | — | Default action if no input |

### IVR menu actions

| Action | Syntax | Example |
|--------|--------|---------|
| Transfer | `transfer:URI` | `transfer:sip:bob@example.com` |
| Playback | `playback:path` | `playback:menu2.wav` |
| Record | `record:path:seconds` | `record:msg.wav:30` |
| Hold | `hold` | `hold` |
| Hangup | `hangup` | `hangup` |

## Complete example

```toml
[[accounts]]
name = "reception"
username = "reception"
password = "secret123"
domain = "pbx.company.com"
server = "10.0.0.50:5060"
sip_port = 5060
rtp_port_start = 8000
rtp_port_end = 8010
auth_method = "md5"
codec = "pcmu"

# Identity
display_name = "Reception"
asserted_id = "sip:+441234567890@company.com"

# Protocol
transport = "tcp"
register_expiry = 1800
user_agent = "MyPBX/3.0"
dtmf_mode = "rfc2833"
early_media = true
session_timers = false

# IVR
auto_answer = true
ivr_welcome = "audio/welcome.wav"
ivr_timeout = 10

[accounts.ivr_menu]
"1" = "transfer:sip:1001@pbx.company.com"
"2" = "transfer:sip:1002@pbx.company.com"
"3" = "playback:audio/directions.wav"
"4" = "record:voicemail/msg.wav:60"
"5" = "hold"
"*" = "hangup"

ivr_default = "transfer:sip:operator@pbx.company.com"
```

---

## License & Disclaimer

> 🤖 **AI Project Notice:** This software was developed using Artificial Intelligence (AI) assistants (**Antigravity**, **Gemini**, **DeepSeek**).
> 
> 📜 **MIT License:** Published under the open-source [MIT License](../LICENSE).
> 
> ⚠️ **Disclaimer of Liability:** THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED. THE AUTHORS AND CONTRIBUTORS ACCEPT NO RESPONSIBILITY OR LIABILITY FOR ANY CLAIMS, DAMAGES, SYSTEM FAILURES, OR LOSSES ARISING FROM THE USE OF THIS SOFTWARE.

