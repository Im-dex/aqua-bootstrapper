# aqua-bootstrapper

Small standalone Rust bootstrapper for installing and verifying Aqua, running
`aqua install`, running post-install commands, and launching the main app
directly.

## Usage

```sh
aqua-bootstrapper --config bootstrap.json
```

If `--config` is omitted, `bootstrap.json` in the current directory is used.

Application arguments can be passed after `--`:

```sh
aqua-bootstrapper --config bootstrap.json -- status --verbose
```

These arguments are appended to `app.command` from the configuration. For
example, with `app.command` set to `["uv", "run", "dv"]`, the command above
runs `uv run dv status --verbose` directly.

The application exit code is returned unchanged.

## Application environment

The launched application receives the Aqua paths used by the bootstrapper:

- `AQUA_EXE`: path to the managed Aqua executable
- `AQUA_ROOT_DIR`: path to the managed Aqua root directory
- `AQUA_CONFIG`: path to the Aqua config file

Use these variables when the application needs to call tools through the same
Aqua installation and config. `AQUA_ROOT_DIR` and `AQUA_CONFIG` are also read by
Aqua itself, so callers can use `AQUA_EXE` without repeating `--root-dir` or
`--config`.

`bootstrapped_tools` can additionally expose direct paths to specific
Aqua-managed tools. Each key must contain only uppercase ASCII letters and
underscores. After `aqua install`, the bootstrapper resolves every tool with
`aqua which` and launches the app with `BOOTSTRAPPED_<KEY>` set to the cached
absolute path. For example, `"NODE_EXE": "node"` sets
`BOOTSTRAPPED_NODE_EXE`.

The first element of `app.command` must be an Aqua-managed tool. After
`aqua install`, the bootstrapper resolves it with `aqua which`, stores the
result in state, and starts that executable directly on later launches.

The configuration file supports `${VAR}` substitutions before JSON parsing.
Values are read from the bootstrapper process environment. Missing variables
fail the bootstrap with an invalid configuration error.

## Configuration

```json
{
  "schema": 2,
  "aqua": {
    "version": "v2.60.1",
    "sha": {
      "windows": "fc0a9f4087297ec16b62a709b4cfffafef321d39250787957e9953c5e1fe9316",
      "linux": "d6f920201c71fb42881af51f8f63c3f06da778b38399248b2c777a288ebe3884"
    },
    "config": "${PROJECT_ROOT}/aqua.yaml",
    "root": "${PROJECT_ROOT}/.dv/aqua"
  },
  "bootstrap_cache": "${PROJECT_ROOT}/.dv/bootstrap",
  "tracked_files": [
    "${PROJECT_ROOT}/aqua.yaml",
    "${PROJECT_ROOT}/aqua-checksums.json",
    "${PROJECT_ROOT}/pyproject.toml",
    "${PROJECT_ROOT}/uv.lock",
    "${PROJECT_ROOT}/config/**/*.toml"
  ],
  "post_install": [
    {
      "name": "Python environment",
      "command": ["uv", "sync", "--locked"]
    }
  ],
  "bootstrapped_tools": {
    "NODE_EXE": "node",
    "PYTHON_EXE": "python"
  },
  "app": {
    "command": ["uv", "run", "dv"]
  }
}
```

After environment substitution, all path fields in the configuration must be
absolute. On Windows, use escaped backslashes or forward slashes in JSON, for
example `C:/work/project/.dv/aqua`.

## Security

The bootstrapper owns Aqua distribution details. The configuration pins the Aqua
version and SHA-256 digest for each supported operating system. A downloaded
release asset is extracted only when its SHA-256 matches `aqua.sha.windows` or
`aqua.sha.linux` for the current platform.

The current schema supports `x86_64` Linux and Windows. Update both the version
and the corresponding pinned hashes together when upgrading Aqua.

## State

State is stored as JSON under `bootstrap_cache/state.json` and is updated
atomically. Aqua download, verification, and extraction are recorded before
`aqua install` starts, so a failed `aqua install` or `post_install` step is
retried without downloading and verifying Aqua again. `aqua install` still runs
on every bootstrap retry because Aqua tracks its own package cache freshness.
The application is only launched after `post_install` has completed
successfully.

The state records the pinned Aqua SHA-256. Changing either `aqua.version` or
the current platform's hash invalidates the cached binary.

The downloaded Aqua archive is temporary: it is removed after successful
extraction, and any leftover download workspace is cleared before the next
download attempt.

Only metadata fingerprints are tracked:

- file size
- modification time in nanoseconds since Unix epoch

`tracked_files` entries must include every input that should invalidate the
bootstrap state, must resolve to absolute paths, and may use glob patterns such
as `${PROJECT_ROOT}/config/**/*.toml`. Glob patterns must match at least one
file. Matched files are sorted and deduplicated before they are stored in state.
The `bootstrapped_tools` mapping is also compared with state so changing an
environment-variable name or tool name refreshes the cached path. Changing the
first element of `app.command` also refreshes its cached executable path.

Content hashes are intentionally not used for fast-path invalidation.
