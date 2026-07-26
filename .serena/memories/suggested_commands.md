# Suggested commands

From the repository root on Windows PowerShell:

- Build/debug CLI: `cargo build`
- Run with default config: `cargo run -- --config bootstrap.json`
- Forward app args: `cargo run -- --config bootstrap.json -- status --verbose`
- Run tests: `cargo test`
- Format/check formatting: `cargo fmt` / `cargo fmt --check`
- Lint all targets: `cargo clippy --all-targets -- -D warnings`
- Release build: `cargo build --release`

The installed binary entrypoint is `aqua-bootstrapper`; config defaults to `bootstrap.json` in the current directory.