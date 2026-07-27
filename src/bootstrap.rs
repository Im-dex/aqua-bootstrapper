use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use reqwest::Client;
use tracing::{debug, info};

use crate::aqua;
use crate::config::{AppExecutable, Config};
use crate::error::{Error, Result};
use crate::fingerprint::{self, FileFingerprint};
use crate::lock::BootstrapLock;
use crate::state::{self, BootstrapState, BootstrappedTool, ResolvedAppExecutable};

#[derive(Debug)]
pub struct Bootstrapper {
    root: PathBuf,
    config: Config,
    app_args: Vec<String>,
}

#[derive(Debug)]
struct Snapshot {
    state: Option<BootstrapState>,
    tracked_files: Vec<FileFingerprint>,
    aqua_executable_exists: bool,
}

impl Bootstrapper {
    pub fn new(root: PathBuf, config: Config, app_args: Vec<String>) -> Self {
        Self {
            root,
            config,
            app_args,
        }
    }

    pub async fn run(&self) -> Result<i32> {
        loop {
            let lock = self.acquire_shared_lock()?;
            debug!(path = %lock.path().display(), "bootstrap shared lock acquired");

            let snapshot = self.snapshot()?;
            if self.is_valid(&snapshot) {
                debug!("bootstrap fast path hit");
                return self
                    .launch_app(snapshot.state.as_ref().expect("valid state is present"))
                    .await;
            }
            drop(lock);

            info!("bootstrap cache miss; acquiring exclusive lock");
            let lock = self.acquire_exclusive_lock()?;
            debug!(path = %lock.path().display(), "bootstrap exclusive lock acquired");

            let snapshot = self.snapshot()?;
            if !self.is_valid(&snapshot) {
                self.bootstrap(snapshot).await?;
            }
            drop(lock);
        }
    }

    async fn bootstrap(&self, snapshot: Snapshot) -> Result<()> {
        let aqua_root = self.aqua_root();
        let aqua_executable = if self.is_aqua_binary_cached(&snapshot) {
            info!("aqua binary cache hit; skipping Aqua download and verification");
            self.aqua_executable()
        } else {
            let client = Client::new();
            let cache = self.bootstrap_cache();
            let aqua_executable = aqua::install::ensure_installed(
                &client,
                &self.config.aqua.version,
                self.config.aqua.sha.for_current_platform()?,
                &aqua_root,
                &cache,
            )
            .await?;

            self.write_state(snapshot.tracked_files.clone(), false, BTreeMap::new(), None)
                .await?;

            aqua_executable
        };

        aqua::exec::run_install(&aqua_executable, &self.aqua_config(), &aqua_root).await?;

        let bootstrapped_tools = self.resolve_bootstrapped_tools(&aqua_executable).await?;

        for command in &self.config.post_install {
            let aqua_config = self.aqua_config();
            let envs = aqua::exec::aqua_envs(&aqua_executable, &aqua_config, &aqua_root);
            let args = aqua::exec::post_install_args(&command.command);
            crate::process::run_foreground(&command.name, &aqua_executable, &args, Some(&envs))
                .await?;
        }

        let resolved_app_executable = self.resolve_app_executable(&aqua_executable).await?;

        self.write_state(
            snapshot.tracked_files,
            true,
            bootstrapped_tools,
            Some(resolved_app_executable),
        )
        .await?;
        Ok(())
    }

    async fn write_state(
        &self,
        tracked_files: Vec<FileFingerprint>,
        post_install_completed: bool,
        bootstrapped_tools: BTreeMap<String, BootstrappedTool>,
        resolved_app_executable: Option<ResolvedAppExecutable>,
    ) -> Result<()> {
        let state = BootstrapState::new(
            self.config.aqua.version.clone(),
            self.config.aqua.sha.for_current_platform()?.to_string(),
            self.relative_to_root(&self.aqua_executable()),
            tracked_files,
            post_install_completed,
            bootstrapped_tools,
            resolved_app_executable,
        );

        let cache_for_write = self.bootstrap_cache();
        tokio::task::spawn_blocking(move || state::write_atomic(&cache_for_write, &state))
            .await??;
        Ok(())
    }

    async fn launch_app(&self, state: &BootstrapState) -> Result<i32> {
        let executable = state
            .resolved_app_executable
            .as_ref()
            .expect("valid state contains a resolved app executable");
        let args = self.app_command_args();
        let envs = self.app_envs(state)?;

        crate::process::run_app("application", executable.path(), &args, Some(&envs)).await
    }

    fn app_command_args(&self) -> Vec<String> {
        let mut args = self.config.app.command.clone();
        args.extend(self.app_args.iter().cloned());
        args
    }

    async fn resolve_bootstrapped_tools(
        &self,
        aqua_executable: &Path,
    ) -> Result<BTreeMap<String, BootstrappedTool>> {
        let aqua_config = self.aqua_config();
        let aqua_root = self.aqua_root();
        let mut tools = BTreeMap::new();

        for (env_name, tool) in &self.config.bootstrapped_tools {
            let path =
                aqua::exec::resolve_tool(aqua_executable, &aqua_config, &aqua_root, tool).await?;
            tools.insert(
                env_name.clone(),
                BootstrappedTool {
                    tool: tool.clone(),
                    path,
                },
            );
        }

        Ok(tools)
    }

    async fn resolve_app_executable(
        &self,
        aqua_executable: &Path,
    ) -> Result<ResolvedAppExecutable> {
        match &self.config.app.executable {
            AppExecutable::Aqua { name } => {
                let path = aqua::exec::resolve_tool(
                    aqua_executable,
                    &self.aqua_config(),
                    &self.aqua_root(),
                    name,
                )
                .await?;
                Ok(ResolvedAppExecutable::Aqua {
                    name: name.clone(),
                    path,
                })
            }
            AppExecutable::Path { path } => {
                validate_application_executable(path)?;
                Ok(ResolvedAppExecutable::Path { path: path.clone() })
            }
        }
    }

    fn app_envs(&self, state: &BootstrapState) -> Result<Vec<(String, String)>> {
        let mut envs = self
            .config
            .app
            .environment
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<Vec<_>>();
        envs.extend(aqua::exec::aqua_envs(
            &self.aqua_executable(),
            &self.aqua_config(),
            &self.aqua_root(),
        ));
        envs.extend(state.bootstrapped_tools.iter().map(|(name, tool)| {
            (
                format!("BOOTSTRAPPED_{name}"),
                tool.path.display().to_string(),
            )
        }));
        envs.push((
            crate::process_containment::PROCESS_TEMPLATE_ENV.to_string(),
            crate::process_containment::command_template_json()?.to_string(),
        ));
        Ok(envs)
    }

    fn acquire_shared_lock(&self) -> Result<BootstrapLock> {
        BootstrapLock::acquire_shared(&self.config.bootstrap_cache)
    }

    fn acquire_exclusive_lock(&self) -> Result<BootstrapLock> {
        BootstrapLock::acquire_exclusive(&self.config.bootstrap_cache)
    }

    fn snapshot(&self) -> Result<Snapshot> {
        let state = state::read(&self.config.bootstrap_cache)?;
        let tracked_files =
            fingerprint::fingerprint_tracked_files(&self.root, &self.config.tracked_files)?;
        let aqua_executable = self.aqua_executable();
        let aqua_executable_exists = fingerprint::executable_exists(&aqua_executable)?;

        Ok(Snapshot {
            state,
            tracked_files,
            aqua_executable_exists,
        })
    }

    fn is_valid(&self, snapshot: &Snapshot) -> bool {
        let Some(state) = &snapshot.state else {
            return false;
        };

        self.is_aqua_binary_cached(snapshot)
            && state.tracked_files == snapshot.tracked_files
            && state.post_install_completed
            && self.bootstrapped_tools_match(state)
            && self.app_executable_matches(state)
    }

    fn is_aqua_binary_cached(&self, snapshot: &Snapshot) -> bool {
        let Some(state) = &snapshot.state else {
            return false;
        };

        state.schema == state::STATE_SCHEMA
            && state.aqua_version == self.config.aqua.version
            && self
                .config
                .aqua
                .sha
                .for_current_platform()
                .is_ok_and(|sha| state.aqua_sha256 == sha)
            && state.aqua_executable == self.relative_to_root(&self.aqua_executable())
            && snapshot.aqua_executable_exists
    }

    fn bootstrapped_tools_match(&self, state: &BootstrapState) -> bool {
        state.bootstrapped_tools.len() == self.config.bootstrapped_tools.len()
            && state.bootstrapped_tools.iter().all(|(env_name, tool)| {
                self.config.bootstrapped_tools.get(env_name) == Some(&tool.tool)
            })
    }

    fn app_executable_matches(&self, state: &BootstrapState) -> bool {
        match (
            &self.config.app.executable,
            state.resolved_app_executable.as_ref(),
        ) {
            (
                AppExecutable::Aqua { name },
                Some(ResolvedAppExecutable::Aqua {
                    name: resolved_name,
                    ..
                }),
            ) => name == resolved_name,
            (
                AppExecutable::Path { path },
                Some(ResolvedAppExecutable::Path {
                    path: resolved_path,
                }),
            ) => path == resolved_path && validate_application_executable(path).is_ok(),
            _ => false,
        }
    }

    fn aqua_config(&self) -> PathBuf {
        self.config.aqua.config.clone()
    }

    fn aqua_root(&self) -> PathBuf {
        self.config.aqua.root.clone()
    }

    fn bootstrap_cache(&self) -> PathBuf {
        self.config.bootstrap_cache.clone()
    }

    fn aqua_executable(&self) -> PathBuf {
        aqua::executable_path(&self.aqua_root())
    }

    fn relative_to_root(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root).unwrap_or(path).to_path_buf()
    }
}

fn validate_application_executable(path: &Path) -> Result<()> {
    let metadata =
        std::fs::metadata(path).map_err(|source| Error::ApplicationExecutableInaccessible {
            path: path.to_path_buf(),
            source,
        })?;
    if !metadata.is_file() {
        return Err(Error::ApplicationExecutableNotRegularFile {
            path: path.to_path_buf(),
        });
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(Error::ApplicationExecutableNotExecutable {
                path: path.to_path_buf(),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Bootstrapper, Snapshot};
    use crate::config::{AppCommand, AppExecutable, AquaConfig, AquaSha, Config};
    use crate::error::Error;
    use crate::fingerprint::FileFingerprint;
    use crate::state::{BootstrapState, BootstrappedTool, ResolvedAppExecutable};
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    const EXPECTED_AQUA_EXE_ENV: &str = "AQUA_BOOTSTRAPPER_EXPECTED_AQUA_EXE";

    #[test]
    fn aqua_binary_cache_ignores_tracked_files() {
        let bootstrapper = bootstrapper();
        let state = state(&bootstrapper, vec![fingerprint("aqua.yaml", 1)]);
        let snapshot = Snapshot {
            state: Some(state),
            tracked_files: vec![fingerprint("aqua.yaml", 2)],
            aqua_executable_exists: true,
        };

        assert!(bootstrapper.is_aqua_binary_cached(&snapshot));
    }

    #[test]
    fn full_bootstrap_state_requires_current_tracked_files() {
        let bootstrapper = bootstrapper();
        let state = state(&bootstrapper, vec![fingerprint("aqua.yaml", 1)]);
        let snapshot = Snapshot {
            state: Some(state),
            tracked_files: vec![fingerprint("aqua.yaml", 2)],
            aqua_executable_exists: true,
        };

        assert!(!bootstrapper.is_valid(&snapshot));
    }

    #[test]
    fn incomplete_bootstrap_state_requires_retry() {
        let bootstrapper = bootstrapper();
        let tracked_files = vec![fingerprint("aqua.yaml", 1)];
        let mut state = state(&bootstrapper, tracked_files.clone());
        state.post_install_completed = false;
        let snapshot = Snapshot {
            state: Some(state),
            tracked_files,
            aqua_executable_exists: true,
        };

        assert!(bootstrapper.is_aqua_binary_cached(&snapshot));
        assert!(!bootstrapper.is_valid(&snapshot));
    }

    #[test]
    fn full_bootstrap_state_requires_current_bootstrapped_tools() {
        let mut bootstrapper = bootstrapper();
        bootstrapper
            .config
            .bootstrapped_tools
            .insert("NODE_EXE".to_string(), "node".to_string());
        let snapshot = Snapshot {
            state: Some(state(&bootstrapper, vec![fingerprint("aqua.yaml", 1)])),
            tracked_files: vec![fingerprint("aqua.yaml", 1)],
            aqua_executable_exists: true,
        };

        assert!(!bootstrapper.is_valid(&snapshot));
    }

    #[test]
    fn full_bootstrap_state_requires_resolved_app_executable() {
        let bootstrapper = bootstrapper();
        let mut state = state(&bootstrapper, vec![fingerprint("aqua.yaml", 1)]);
        state.resolved_app_executable = None;
        let snapshot = Snapshot {
            state: Some(state),
            tracked_files: vec![fingerprint("aqua.yaml", 1)],
            aqua_executable_exists: true,
        };

        assert!(!bootstrapper.is_valid(&snapshot));
    }

    #[test]
    fn full_bootstrap_state_requires_current_app_executable() {
        let mut bootstrapper = bootstrapper();
        let state = state(&bootstrapper, vec![fingerprint("aqua.yaml", 1)]);
        bootstrapper.config.app.executable = AppExecutable::Aqua {
            name: "uvx".to_string(),
        };
        let snapshot = Snapshot {
            state: Some(state),
            tracked_files: vec![fingerprint("aqua.yaml", 1)],
            aqua_executable_exists: true,
        };

        assert!(!bootstrapper.is_valid(&snapshot));
    }

    #[test]
    fn app_command_keeps_entire_command_as_arguments() {
        let mut bootstrapper = bootstrapper();
        bootstrapper.app_args = vec!["status".to_string()];

        assert_eq!(bootstrapper.app_command_args(), ["--version", "status"]);
    }

    #[tokio::test]
    async fn absolute_app_path_is_resolved_without_aqua() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join(executable_filename("dv"));
        write_executable(&executable);
        let mut bootstrapper = bootstrapper();
        bootstrapper.config.app.executable = AppExecutable::Path {
            path: executable.clone(),
        };
        bootstrapper.config.app.command.clear();

        let resolved = bootstrapper
            .resolve_app_executable(Path::new("aqua-is-not-used"))
            .await
            .unwrap();

        assert_eq!(resolved, ResolvedAppExecutable::Path { path: executable });
    }

    #[tokio::test]
    async fn missing_absolute_app_path_is_reported_clearly() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("missing");
        let mut bootstrapper = bootstrapper();
        bootstrapper.config.app.executable = AppExecutable::Path {
            path: executable.clone(),
        };
        bootstrapper.config.app.command.clear();

        let error = bootstrapper
            .resolve_app_executable(Path::new("aqua-is-not-used"))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            Error::ApplicationExecutableInaccessible { path, source }
                if path == executable && source.kind() == std::io::ErrorKind::NotFound
        ));
    }

    #[tokio::test]
    async fn directory_is_not_accepted_as_absolute_app_path() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("directory");
        std::fs::create_dir(&executable).unwrap();
        let mut bootstrapper = bootstrapper();
        bootstrapper.config.app.executable = AppExecutable::Path {
            path: executable.clone(),
        };
        bootstrapper.config.app.command.clear();

        let error = bootstrapper
            .resolve_app_executable(Path::new("aqua-is-not-used"))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            Error::ApplicationExecutableNotRegularFile { path } if path == executable
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn absolute_app_path_requires_executable_permission() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("dv");
        std::fs::write(&executable, []).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o644)).unwrap();
        let mut bootstrapper = bootstrapper();
        bootstrapper.config.app.executable = AppExecutable::Path {
            path: executable.clone(),
        };
        bootstrapper.config.app.command.clear();

        let error = bootstrapper
            .resolve_app_executable(Path::new("aqua-is-not-used"))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            Error::ApplicationExecutableNotExecutable { path } if path == executable
        ));
    }

    #[test]
    fn missing_cached_absolute_app_path_requires_retry() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join(executable_filename("dv"));
        write_executable(&executable);
        let mut bootstrapper = bootstrapper();
        bootstrapper.config.app.executable = AppExecutable::Path {
            path: executable.clone(),
        };
        bootstrapper.config.app.command.clear();
        let tracked_files = vec![fingerprint("aqua.yaml", 1)];
        let mut state = state(&bootstrapper, tracked_files.clone());
        state.resolved_app_executable = Some(ResolvedAppExecutable::Path {
            path: executable.clone(),
        });
        let snapshot = Snapshot {
            state: Some(state),
            tracked_files,
            aqua_executable_exists: true,
        };

        assert!(bootstrapper.is_valid(&snapshot));

        std::fs::remove_file(executable).unwrap();

        assert!(!bootstrapper.is_valid(&snapshot));
    }

    #[test]
    fn state_with_different_checksum_requires_aqua_redownload() {
        let bootstrapper = bootstrapper();
        let tracked_files = vec![fingerprint("aqua.yaml", 1)];
        let mut state = state(&bootstrapper, tracked_files.clone());
        state.aqua_sha256 =
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
        let snapshot = Snapshot {
            state: Some(state),
            tracked_files,
            aqua_executable_exists: true,
        };

        assert!(!bootstrapper.is_aqua_binary_cached(&snapshot));
    }

    #[test]
    fn app_envs_include_configured_values_and_cached_bootstrapped_tool_paths() {
        let mut bootstrapper = bootstrapper();
        bootstrapper
            .config
            .app
            .environment
            .insert("APP_MODE".to_string(), "development".to_string());
        let mut state = state(&bootstrapper, vec![]);
        state.bootstrapped_tools.insert(
            "NODE_EXE".to_string(),
            BootstrappedTool {
                tool: "node".to_string(),
                path: absolute_root().join(".dv/aqua/bin/node"),
            },
        );

        assert_eq!(
            bootstrapper.app_envs(&state).unwrap(),
            [
                ("APP_MODE".to_string(), "development".to_string()),
                (
                    "AQUA_EXE".to_string(),
                    bootstrapper.aqua_executable().display().to_string(),
                ),
                (
                    "AQUA_ROOT_DIR".to_string(),
                    bootstrapper.aqua_root().display().to_string(),
                ),
                (
                    "AQUA_CONFIG".to_string(),
                    bootstrapper.aqua_config().display().to_string(),
                ),
                (
                    "BOOTSTRAPPED_NODE_EXE".to_string(),
                    absolute_root()
                        .join(".dv/aqua/bin/node")
                        .display()
                        .to_string(),
                ),
                (
                    "PROCESS_CONTAINMENT_TEMPLATE_JSON".to_string(),
                    crate::process_containment::command_template_json()
                        .unwrap()
                        .to_string(),
                ),
            ]
        );
    }

    #[tokio::test]
    async fn managed_app_environment_takes_precedence_over_configured_values() {
        let mut bootstrapper = bootstrapper();
        let expected_aqua_executable = bootstrapper.aqua_executable().display().to_string();
        bootstrapper
            .config
            .app
            .environment
            .insert("AQUA_EXE".to_string(), "configured-value".to_string());
        bootstrapper
            .config
            .app
            .environment
            .insert(EXPECTED_AQUA_EXE_ENV.to_string(), expected_aqua_executable);
        let envs = bootstrapper
            .app_envs(&state(&bootstrapper, vec![]))
            .unwrap();
        let executable = std::env::current_exe().unwrap();
        let args = vec![
            "--exact".to_string(),
            "bootstrap::tests::managed_app_environment_priority_child".to_string(),
            "--quiet".to_string(),
        ];

        let exit_code = crate::process::run_app(
            "app environment priority test",
            &executable,
            &args,
            Some(&envs),
        )
        .await
        .unwrap();

        assert_eq!(exit_code, 0);
    }

    #[test]
    fn managed_app_environment_priority_child() {
        let Some(expected_aqua_executable) = std::env::var_os(EXPECTED_AQUA_EXE_ENV) else {
            return;
        };

        assert_eq!(
            std::env::var_os("AQUA_EXE").as_deref(),
            Some(expected_aqua_executable.as_os_str())
        );
    }

    fn bootstrapper() -> Bootstrapper {
        let root = absolute_root();
        Bootstrapper::new(
            root.clone(),
            Config {
                schema: 4,
                aqua: AquaConfig {
                    version: "v2.59.2".to_string(),
                    sha: AquaSha {
                        windows: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                            .to_string(),
                        linux: "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                            .to_string(),
                    },
                    config: root.join("aqua.yaml"),
                    root: root.join(".dv").join("aqua"),
                },
                bootstrap_cache: root.join(".dv").join("bootstrap"),
                tracked_files: vec![root.join("aqua.yaml")],
                post_install: vec![],
                bootstrapped_tools: BTreeMap::new(),
                app: AppCommand {
                    executable: AppExecutable::Aqua {
                        name: "aqua".to_string(),
                    },
                    command: vec!["--version".to_string()],
                    environment: BTreeMap::new(),
                },
            },
            vec![],
        )
    }

    fn state(bootstrapper: &Bootstrapper, tracked_files: Vec<FileFingerprint>) -> BootstrapState {
        BootstrapState::new(
            bootstrapper.config.aqua.version.clone(),
            bootstrapper
                .config
                .aqua
                .sha
                .for_current_platform()
                .unwrap()
                .to_string(),
            bootstrapper.relative_to_root(&bootstrapper.aqua_executable()),
            tracked_files,
            true,
            BTreeMap::new(),
            Some(ResolvedAppExecutable::Aqua {
                name: "aqua".to_string(),
                path: absolute_root().join(".dv/aqua/bin/aqua"),
            }),
        )
    }

    fn fingerprint(path: &str, size: u64) -> FileFingerprint {
        FileFingerprint {
            path: path.into(),
            size,
            mtime_ns: 7,
        }
    }

    fn write_executable(path: &Path) {
        #[cfg(windows)]
        std::fs::copy(std::env::current_exe().unwrap(), path).unwrap();

        #[cfg(unix)]
        {
            std::fs::write(path, []).unwrap();

            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn executable_filename(name: &str) -> String {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }

    fn absolute_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:/project")
        } else {
            PathBuf::from("/project")
        }
    }
}
