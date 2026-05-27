# rsipclient — Rust SIP Client & IVR Engine

[![CI](https://github.com/USER/rsipclient/actions/workflows/ci.yml/badge.svg)](https://github.com/USER/rsipclient/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable%201.70%2B-orange.svg)](https://rust-lang.org)

**A multi-account SIP client with a built-in auto-attendant, written in pure async Rust.**

Place calls, play audio, detect DTMF, transfer callers, record voicemail — everything
over a lightweight JSON TCP interface. No heavy frameworks, no JVM, just a single
binary that speaks SIP + RTP.

Ideal for:
- ☎️ **Automated call routing** — IVR menus with DTMF-driven transfers
- 🤖 **Voice bots** — script call flows via TCP commands
- 🧪 **SIP testing** — generate and inspect SIP signalling from CLI
- 📞 **Softphone backend** — embed SIP capabilities into your own app

## Features

- **Multi-account** — manage multiple SIP registrations simultaneously
- **SIP signalling** — REGISTER, INVITE, BYE, CANCEL, ACK, REFER
- **MD5 digest** authentication (RFC 2617)
- **RTP streaming** — G.711 μ-law, A-law, Opus codecs
- **IVR / Auto-attendant** — answer, play menus, collect DTMF, transfer, hold, record
- **RFC 2833 DTMF** — in-band telephone-event detection
- **RFC 3325** — P-Asserted-Identity, P-Preferred-Identity headers
- **RFC 4028** — Session-Expires / session timers
- **JSON TCP IPC** — control the service from any language
- **Zero-copy RTP** — efficient G.711 en/decoding
- **Single binary** — ~2 MB release build (no Opus), ~3 MB with Opus
- **IVR / Auto-attendant** — answer incoming calls, play menus, collect DTMF
- **Call transfer** via REFER (blind transfer)
- **Call hold / resume** via re-INVITE
- **Recording** — capture caller audio to WAV file
- **Identity headers** — P-Asserted-Identity, P-Preferred-Identity (RFC 3325)
- **Session timers** — RFC 4028 support
- **Custom User-Agent**, display name, proxy routing
- **TCP control interface** — send JSON commands to manage calls

## Quick Start

### Install

```bash
# From source
git clone https://github.com/USER/rsipclient.git
cd rsipclient
cargo build --release

# With Opus support (requires libopus-dev on Linux)
cargo build --release --features opus
```

### Configure

Create `config.toml`:

```toml
[[accounts]]
name = "alice"
username = "alice"
password = "secret123"
domain = "sip.example.com"
server = "192.168.1.1:5060"
sip_port = 5060
rtp_port_start = 8000
rtp_port_end = 8010
auth_method = "md5"
codec = "pcmu"
display_name = "Alice Smith"
```

### Run

```bash
# List all configured accounts
sip-client -c config.toml list

# Start the service (TCP control on 127.0.0.1:5090)
sip-client -c config.toml service
```

### Control via TCP

The service listens on `127.0.0.1:5090`. Send one JSON command per line:

```json
{"cmd":"register","account":"alice"}
{"cmd":"call","account":"alice","target":"sip:bob@sip.example.com"}
{"cmd":"play","account":"alice","target":"audio.wav"}
{"cmd":"hangup","account":"alice"}
{"cmd":"status"}
{"cmd":"shutdown"}
```

## IVR / Auto-Attendant

Auto-answer incoming calls and run a DTMF-driven menu:

```toml
[[accounts]]
name = "reception"
# ...
auto_answer = true
ivr_welcome = "welcome.wav"
ivr_timeout = 10

[accounts.ivr_menu]
"1" = "transfer:sip:1001@sip.example.com"
"2" = "transfer:sip:1002@sip.example.com"
"3" = "playback:info.wav"
"4" = "record:voicemail.wav:30"
"5" = "hold"
"*" = "hangup"

ivr_default = "transfer:sip:operator@sip.example.com"
```

### Menu actions

| Action | Format | Description |
|--------|--------|-------------|
| Transfer | `transfer:sip:target@host` | Blind transfer via REFER |
| Playback | `playback:path/file.wav` | Play audio, return to menu |
| Record | `record:output.wav:30` | Record N seconds, return to menu |
| Hold | `hold` | Hold, press any DTMF to resume |
| Hangup | `hangup` | End the call |

## Configuration Reference

See [docs/configuration.md](docs/configuration.md) for the full list of options.

### Per-account settings

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | — | Account identifier |
| `username` | string | — | SIP username |
| `password` | string | — | SIP password |
| `domain` | string | — | SIP domain |
| `server` | host:port | — | SIP server address |
| `sip_port` | u16 | 0 (auto) | Local SIP port |
| `rtp_port_start` | u16 | — | RTP port range start |
| `rtp_port_end` | u16 | — | RTP port range end |
| `auth_method` | md5/none | md5 | Authentication method |
| `codec` | pcmu/pcma/opus | pcmu | Audio codec |
| `display_name` | string | — | From header display name |
| `asserted_id` | URI | — | P-Asserted-Identity |
| `preferred_id` | URI | — | P-Preferred-Identity |
| `proxy` | host:port | — | Outbound proxy |
| `register_expiry` | u32 | 3600 | REGISTER expiry (seconds) |
| `user_agent` | string | — | Custom User-Agent header |
| `dtmf_mode` | rfc2833/inband/info | — | DTMF signalling mode |
| `early_media` | bool | true | 183 Session Progress |
| `session_timers` | bool | false | RFC 4028 Session-Expires |
| `auto_answer` | bool | false | Auto-answer incoming INVITEs |
| `ivr_welcome` | path | — | IVR welcome WAV file |
| `ivr_timeout` | u64 | 10 | DTMF timeout (seconds) |
| `ivr_menu` | map | — | DTMF → action mappings |
| `ivr_default` | string | — | Default action on timeout |

## Architecture

```
                    ┌──────────────────┐
TCP (5090)  ───────▶│    Service       │
  JSON IPC          │  ┌────────────┐  │
                    │  │ SipClient 1 │──▶ UDP (SIP + RTP)
                    │  │ SipClient 2 │──▶ UDP (SIP + RTP)
                    │  │ IVR Watcher │──▶ RTP Receiver
                    │  └────────────┘  │
                    └──────────────────┘
```

```
src/
├── main.rs            Entry point
├── cli.rs             CLI parsing (clap)
├── config.rs          Config file + validation
├── service.rs         Multi-account service + TCP IPC
├── ivr.rs             IVR engine (answer, menu, DTMF)
├── ipc.rs             JSON request/response types
├── ipc_client.rs      TCP client for CLI commands
├── rtp/
│   ├── mod.rs         RTP sender + resampler
│   ├── codec.rs       G.711 / Opus codecs
│   ├── wav.rs         WAV file parser
│   └── receiver.rs    RTP receiver + DTMF detector
├── sip/
│   ├── auth.rs        MD5 digest auth
│   ├── client.rs      SipClient struct
│   ├── messages.rs    SIP request builders
│   ├── operations.rs  Register, invite, bye, cancel
│   ├── transfer.rs    REFER + hold/resume
│   ├── sdp.rs         SDP builder
│   ├── settings.rs    Per-account SIP settings
│   ├── transport.rs   UDP transport
│   └── utils.rs       Header parsers, ID gen
└── service/
    └── handlers.rs    Command dispatcher
```

## License

MIT — see [LICENSE](LICENSE) for details.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). PRs welcome!
