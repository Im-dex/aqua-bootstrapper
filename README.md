# aqua-bootstrapper

Small standalone Rust bootstrapper for installing and verifying Aqua, running
`aqua install`, running post-install commands, and launching the main app through
`aqua exec`.

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
runs `uv run dv status --verbose` through `aqua exec`.

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

The configuration file supports `${VAR}` substitutions before JSON parsing.
Values are read from the bootstrapper process environment. Missing variables
fail the bootstrap with an invalid configuration error.

## Configuration

```json
{
  "schema": 1,
  "aqua_version": "v2.59.2",
  "aqua_config": "${PROJECT_ROOT}/aqua.yaml",
  "aqua_root": "${PROJECT_ROOT}/.dv/aqua",
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
  "app": {
    "command": ["uv", "run", "dv"]
  }
}
```

After environment substitution, all path fields in the configuration must be
absolute. On Windows, use escaped backslashes or forward slashes in JSON, for
example `C:/work/project/.dv/aqua`.

## Security

The bootstrapper owns Aqua distribution details. The config only selects the Aqua
version. Downloaded Aqua release assets are accepted only after GitHub Artifact
Attestation verification through `sigstore-verification`.

The policy is fixed in code:

- repository: `aquaproj/aqua`
- issuer: GitHub Actions OIDC
- ref: `refs/tags/<aqua_version>`
- subject digest: SHA-256 of the downloaded release asset

No fallback verification mode exists.

## State

State is stored as JSON under `bootstrap_cache/state.json` and is updated
atomically. Aqua download, verification, and extraction are recorded before
`aqua install` starts, so a failed `aqua install` or `post_install` step is
retried without downloading and verifying Aqua again. `aqua install` still runs
on every bootstrap retry because Aqua tracks its own package cache freshness.
The application is only launched after `post_install` has completed
successfully.

Only metadata fingerprints are tracked:

- file size
- modification time in nanoseconds since Unix epoch

`tracked_files` entries must include every input that should invalidate the
bootstrap state, must resolve to absolute paths, and may use glob patterns such
as `${PROJECT_ROOT}/config/**/*.toml`. Glob patterns must match at least one
file. Matched files are sorted and deduplicated before they are stored in state.

Content hashes are intentionally not used for fast-path invalidation.
