# Contributing to rsipclient

Thanks for your interest in contributing!

## Getting Started

1. **Fork** the repository
2. **Clone** your fork: `git clone https://github.com/YOUR_USER/rsipclient.git`
3. **Build**: `cargo build`
4. **Test**: `cargo test`

## Development

```bash
# Build with all features
cargo build --features opus

# Run tests
cargo test
cargo test --features opus

# Check + lint
cargo clippy --all-targets -- -D warnings
cargo fmt --check

# Format
cargo fmt
```

## Code structure

- `src/sip/` — SIP protocol (signalling, messages, transport)
- `src/rtp/` — RTP media (codecs, WAV, DTMF detection)
- `src/service.rs` — Multi-account service + TCP IPC
- `src/ivr.rs` — Auto-attendant / IVR engine
- `src/config.rs` — TOML config parsing

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
