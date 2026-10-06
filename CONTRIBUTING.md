# Contributing to rsipclient

Thanks for your interest in contributing!

## Getting Started

1. **Fork** the repository
2. **Clone** your fork: `git clone https://github.com/YOUR_USER/rsipclient.git`
3. **Build**: `cargo build`
4. **Test**: `cargo test`

## Development

```bash
# Build (Opus support is always compiled in)
cargo build

# Run unit + integration tests
cargo test --all-targets

# Check + lint
cargo clippy --all-targets -- -D warnings
cargo fmt --check

# Format
cargo fmt
```

## Testing

- **Unit tests** live next to the code in `#[cfg(test)]` modules.
- **Integration tests** in `tests/` use the library crate (`src/lib.rs`):
  - `tests/sip_flows.rs` — REGISTER/INVITE/ACK/BYE against a scripted UDP SIP
    peer that verifies digest responses like a real registrar
  - `tests/media.rs` — WAV → RTP → receiver → WAV round trips per codec
  - `tests/robustness.rs` — seeded random mutations of SIP, SDP, RTP and WAV
    input; every parser must reject garbage without panicking
- **Fuzzing** with [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)
  (nightly toolchain) for longer runs:

```bash
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz list
cargo +nightly fuzz run sip_message -- -max_total_time=300
```

CI runs every fuzz target for 30 seconds as a smoke test.

## Code structure

- `src/lib.rs` — library root; `src/main.rs` is the thin CLI binary
- `src/sip/` — SIP protocol (signalling, messages, transport)
- `src/rtp/` — RTP media (codecs, WAV, DTMF detection)
- `src/service.rs` — Multi-account service, web dashboard + TCP IPC
- `src/ivr/` — Auto-attendant / IVR engine
- `src/config.rs` — TOML config parsing
- `fuzz/` — cargo-fuzz targets

## Conventions

- **Rust edition 2021**
- Format with `cargo fmt` before commit
- No clippy warnings
- Use `anyhow::Result` for error handling
- Log via the `log` crate (use `pretty_env_logger`)
- Async I/O via `tokio`
- Keep files under ~250 lines; split into submodules when needed

## Adding a feature

1. Open an issue to discuss the feature
2. Implement with tests
3. Update `docs/` if config changes
4. Run `cargo test --all-targets` and `cargo clippy`
5. Submit a PR

## Code of Conduct

Be respectful. Keep discussions constructive.
