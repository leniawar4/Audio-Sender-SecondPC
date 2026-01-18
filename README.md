# LAN Audio Streamer

Professional-grade low-latency LAN multi-track audio streaming (Rust).

Overview
- Multi-track capture and playback
- Opus encoding/decoding for bandwidth-efficient transport
- UDP-based low-latency transport with simple packet header
- Web UI (HTTP + WebSocket) for control

Key Features
- Per-track capture/playback with independent configuration
- Opus codec presets for voice/music/low-latency use
- Ring buffers and jitter buffer support for smooth playback
- Web UI to create/manage tracks and monitor status

Prerequisites
- Rust toolchain (stable)
- For Windows: Visual Studio Build Tools (MSVC) and the `windows` crate features enabled
- Optional: audio devices available for capture/playback

Build
```bash
# From repository root
cargo build --release
```

Run
- Run sender (captures local devices and streams to remote):
```bash
cargo run --bin sender --release
```

- Run receiver (receives and plays streams):
```bash
cargo run --bin receiver --release
```

Configuration
- Application settings are read from `config.toml` / environment (see `src/config.rs`)
- UI configuration (bind address / port) is in the `UiConfig` struct in `src/config.rs`

Web UI
- Server exposes an HTTP API and WebSocket at `/ws`
- Static UI files (simple control panel) are served from `static/` when enabled

Development notes
- Code uses `tokio` async runtime and `axum` for the web server
- Opus codec handled via `opus` crate; encoder/decoder are managed in the audio pipeline (not stored in shared Track objects)
- Track management is in `src/tracks`

Testing
- Unit tests live next to modules (run with `cargo test`)

Next steps / suggestions
- Add CI (GitHub Actions) with `cargo test` and `cargo clippy`
- Add integration tests for end-to-end capture -> encode -> send -> receive -> decode
- Improve README with example `config.toml` and UI screenshots

License
- MIT

Contact
- For questions or contributions, open an issue or PR in the repository.
