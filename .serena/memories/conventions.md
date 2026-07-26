# Conventions

- Keep process creation centralized in `src/process.rs` so `process_containment::configure_child` applies to every direct child.
- Configuration path fields must be absolute after MiniJinja rendering; environment variables are exposed as `env`, platform as `os`.
- Persist bootstrap state atomically and invalidate via metadata fingerprints plus explicit configuration-derived state; do not introduce content hashing casually.
- Commands are argv arrays, never shell strings. Inherit stdio for foreground/application launches and preserve the application exit code.
- Aqua distribution version and platform SHA-256 pins move together.
- Use typed `Error` variants with actionable context; avoid panics except internal invariants after validation.
- Unit tests live beside implementation; reserve `tests/cli_tests.rs` for observable CLI/process behavior.