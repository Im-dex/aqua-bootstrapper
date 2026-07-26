# Tech stack

- Rust 2024 edition; MSRV/toolchain pinned to 1.96.1 in `Cargo.toml` and `rust-toolchain.toml`.
- Async runtime/process/filesystem: Tokio. CLI: Clap derive. Config serialization: Serde/serde_json. Config templating: MiniJinja.
- HTTP/download: Reqwest with rustls. Integrity: SHA-256. Archives: zip on Windows, tar+flate2 elsewhere.
- Platform containment: `windows-sys` Job Objects on Windows; parent-death signal path on Linux.
- `rustfmt` and Clippy are installed by the pinned minimal toolchain; Clippy project thresholds live in `clippy.toml`.