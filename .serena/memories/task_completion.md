# Task completion

Run, in order:

1. `cargo fmt --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test`

For config/CLI contract changes, also update `README.md` and `bootstrap.example.json`, then exercise the relevant `cargo run -- ...` path when practical. Ensure `git diff --check` passes and do not disturb unrelated working-tree changes.