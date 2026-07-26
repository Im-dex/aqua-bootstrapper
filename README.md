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

These arguments are appended after the configured `app.command`.

The application exit code is returned unchanged. On Unix, termination by signal
`N` is reported with the conventional shell exit code `128 + N`.

## Diagnostics

Diagnostic logging is disabled by default and is not required for normal use.
Set `RUST_LOG` in the bootstrapper process environment when troubleshooting.
Prefer the crate-specific filter to avoid enabling logs from dependencies:

PowerShell:

```powershell
$env:RUST_LOG = "aqua_bootstrapper=debug"
.\aqua-bootstrapper.exe --config bootstrap.json
Remove-Item Env:RUST_LOG
```

Bash:

```sh
RUST_LOG=aqua_bootstrapper=debug ./aqua-bootstrapper --config bootstrap.json
```

`RUST_LOG=debug` also enables diagnostic output from dependencies. Environment
variables are inherited by child processes, so scope `RUST_LOG` to a diagnostic
invocation unless the launched application should receive it as well.

## Process lifetime

On Windows, the bootstrapper places itself in a Job Object configured with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. If the bootstrapper exits or is terminated
unexpectedly, Windows terminates all processes in that job, including descendants
of the launched application.

This guarantee is tied to the lifetime of the bootstrapper, not to each
intermediate parent process. If a child process exits while the bootstrapper
remains running, that child's descendants remain in the bootstrapper's Job Object
and may continue running until the bootstrapper exits.

On Linux, every command started directly by the bootstrapper uses
`PR_SET_PDEATHSIG` and receives `SIGTERM` if the bootstrapper dies. Linux does not
propagate this setting to descendants. Applications that start processes which
must not outlive them should launch those processes through the bootstrapper's
`pdeathsig` command:

```sh
aqua-bootstrapper pdeathsig --parent-pid "$$" -- <command> [args...]
```

Repeating this rule at each process boundary keeps the parent-death behavior
throughout the application process tree.

## Application environment

The launched application receives the Aqua paths used by the bootstrapper:

- `AQUA_EXE`: path to the managed Aqua executable
- `AQUA_ROOT_DIR`: path to the managed Aqua root directory
- `AQUA_CONFIG`: path to the Aqua config file

It also receives `PROCESS_CONTAINMENT_TEMPLATE_JSON`, a JSON command template
for starting child processes. On Linux it contains the absolute path to the
current bootstrapper followed by `"pdeathsig"`, `"--parent-pid"`, the
`"{parent_pid}"` placeholder, and `"--"`; on Windows it is an empty array because
the Job Object already contains descendant processes.

The application's central process-launch helper must parse the template, replace
`"{parent_pid}"` with its own PID immediately before spawning the child, and
prepend the resolved arguments to the child argv. Individual call sites should
only pass the executable and its arguments to that helper. Do not treat the
template as a shell command. In pseudocode:

```text
resolved_prefix = resolve_parent_pid(template, current_pid)
resolved_prefix + [executable] + args
```

Passing the PID explicitly lets the wrapper detect that the launching process
exited before `PR_SET_PDEATHSIG` was configured and prevents the child command
from executing in that case.

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

`app.executable` selects and names the executable:

- `{"source": "aqua", "name": "<tool>"}` resolves the named tool with
  `aqua which`.
- `{"source": "path", "path": "<absolute path>"}` uses the path directly.

`app.command` contains only arguments passed to the selected executable. An
absolute path is checked after all post-install commands have completed, then
stored in state and launched directly.

Explicit Aqua selector:

```json
{
  "app": {
    "executable": {
      "source": "aqua",
      "name": "uv"
    },
    "command": ["run", "dv"]
  }
}
```

Absolute path selector:

```json
{
  "app": {
    "executable": {
      "source": "path",
      "path": "{% if os == 'windows' %}{{ env.PROJECT_ROOT }}/.venv/Scripts/dv.exe{% else %}{{ env.PROJECT_ROOT }}/.venv/bin/dv{% endif %}"
    },
    "command": []
  }
}
```

An absolute application path must not contain `.` or `..` components. Before it
is saved in state, the bootstrapper verifies that it points to a regular file
and, on Linux, that at least one executable permission bit is set. Windows has
no equivalent preflight format validation: an incompatible file is saved as a
regular file and the operating system reports the error when the application is
launched. If the cached file later stops satisfying the checks performed by the
bootstrapper, the bootstrap state is invalidated and `post_install` is retried.

The configuration file is rendered as a [MiniJinja](https://docs.rs/minijinja/)
template before JSON parsing. Environment variables are available through the
`env` object, for example `{{ env.PROJECT_ROOT }}`. Substitutions must remain
inside a JSON string. Missing variables fail the bootstrap with an invalid
configuration error. The global `os` contains the current platform identifier,
such as `windows` or `linux`, and can be used in conditional blocks.

## Configuration

```json
{
  "schema": 4,
  "aqua": {
    "version": "v2.60.1",
    "sha": {
      "windows": "fc0a9f4087297ec16b62a709b4cfffafef321d39250787957e9953c5e1fe9316",
      "linux": "d6f920201c71fb42881af51f8f63c3f06da778b38399248b2c777a288ebe3884"
    },
    "config": "{{ env.PROJECT_ROOT }}/aqua.yaml",
    "root": "{{ env.PROJECT_ROOT }}/.dv/aqua"
  },
  "bootstrap_cache": "{{ env.PROJECT_ROOT }}/.dv/bootstrap",
  "tracked_files": [
    "{{ env.PROJECT_ROOT }}/aqua.yaml",
    "{{ env.PROJECT_ROOT }}/aqua-checksums.json",
    "{{ env.PROJECT_ROOT }}/pyproject.toml",
    "{{ env.PROJECT_ROOT }}/uv.lock",
    "{{ env.PROJECT_ROOT }}/config/project.toml"
  ],
  "post_install": [
    {
      "name": "{% if os == 'windows' %}Python environment on Windows{% else %}Python environment on Linux{% endif %}",
      "command": ["uv", "sync", "--all-groups", "--locked", "--project", "{{ env.PROJECT_ROOT }}"]
    }
  ],
  "bootstrapped_tools": {
    "NODE_EXE": "node",
    "PYTHON_EXE": "python"
  },
  "app": {
    "executable": {
      "source": "path",
      "path": "{% if os == 'windows' %}{{ env.PROJECT_ROOT }}/.venv/Scripts/dv.exe{% else %}{{ env.PROJECT_ROOT }}/.venv/bin/dv{% endif %}"
    },
    "command": []
  }
}
```

After environment substitution, all path fields in the configuration must be
absolute and must not contain `.` or `..` components. On Windows, use escaped
backslashes or forward slashes in JSON, for example
`C:/work/project/.dv/aqua`.

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

State schema 2 requires the complete current state shape. State written by
earlier bootstrapper versions is not accepted.

The state records the pinned Aqua SHA-256. Changing either `aqua.version` or
the current platform's hash invalidates the cached binary.

The downloaded Aqua archive is temporary: it is removed after successful
extraction, and any leftover download workspace is cleared before the next
download attempt.

Only metadata fingerprints are tracked:

- file size
- modification time in nanoseconds since Unix epoch

`tracked_files` entries must include every input that should invalidate the
bootstrap state. Each entry must resolve to the absolute path of one existing
regular file. Wildcard characters have no special meaning. Tracked files are
sorted and deduplicated before they are stored in state.
The `bootstrapped_tools` mapping is also compared with state so changing an
environment-variable name or tool name refreshes the cached path. Changing the
`app.executable` selector also refreshes the cached executable path. A missing
absolute application executable invalidates the cached state as well.

The resolved application executable is stored as one tagged state value:
`aqua` contains the Aqua tool name and resolved path, while `path` contains the
validated absolute path.

Content hashes are intentionally not used for fast-path invalidation.
