# aqua-bootstrapper

Small standalone Rust bootstrapper for installing and verifying Aqua, running
`aqua install`, running post-install commands, and launching the main app through
`aqua exec`.

## Usage

```sh
aqua-bootstrapper --config bootstrap.json
```

If `--config` is omitted, `bootstrap.json` in the current directory is used.

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

## Configuration

```json
{
  "schema": 1,
  "aqua_version": "v2.59.2",
  "aqua_config": "aqua.yaml",
  "aqua_root": ".dv/aqua",
  "bootstrap_cache": ".dv/bootstrap",
  "tracked_files": [
    "aqua.yaml",
    "aqua-checksums.json",
    "pyproject.toml",
    "uv.lock"
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
atomically only after a complete successful bootstrap.

Only metadata fingerprints are tracked:

- file size
- modification time in nanoseconds since Unix epoch

Content hashes are intentionally not used for fast-path invalidation.
