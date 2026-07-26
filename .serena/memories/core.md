# aqua-bootstrapper core

- Standalone Rust CLI/library. `src/main.rs` owns Clap parsing; `src/lib.rs` exposes bootstrap execution and platform setup.
- Bootstrap orchestration is in `src/bootstrap.rs`; JSON/template validation in `src/config.rs`; persisted cache contract in `src/state.rs`; shared child launching in `src/process.rs`; OS lifetime guarantees in `src/process_containment.rs`.
- Aqua download/install/which integration is under `src/aqua/`; filesystem, atomic-write, hashing, and progress helpers are under `src/util/`.
- Public configuration example and user contract: `bootstrap.example.json` and `README.md`.
- Integration tests: `tests/cli_tests.rs`; most focused tests are inline `#[cfg(test)]` modules.
- The first application command token is currently resolved through `aqua which`, cached in state, and then launched directly; application arguments after CLI `--` are appended.
- Read `mem:tech_stack` for pinned tooling, `mem:conventions` for design constraints, `mem:suggested_commands` for entrypoints, and `mem:task_completion` before handing off changes.