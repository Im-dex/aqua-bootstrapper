use std::path::{Path, PathBuf};

use reqwest::Client;
use tracing::{debug, info};

use crate::aqua;
use crate::config::Config;
use crate::error::Result;
use crate::fingerprint::{self, FileFingerprint};
use crate::lock::BootstrapLock;
use crate::state::{self, BootstrapState};

#[derive(Debug)]
pub struct Bootstrapper {
    root: PathBuf,
    config: Config,
    app_args: Vec<String>,
    client: Client,
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
            client: Client::new(),
        }
    }

    pub async fn run(&self) -> Result<i32> {
        loop {
            let lock = self.acquire_shared_lock().await?;
            debug!(path = %lock.path().display(), "bootstrap shared lock acquired");

            let snapshot = self.snapshot().await?;
            if self.is_valid(&snapshot) {
                debug!("bootstrap fast path hit");
                return self.launch_app().await;
            }
            drop(lock);

            info!("bootstrap cache miss; acquiring exclusive lock");
            let lock = self.acquire_exclusive_lock().await?;
            debug!(path = %lock.path().display(), "bootstrap exclusive lock acquired");

            let snapshot = self.snapshot().await?;
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
            let cache = self.bootstrap_cache();
            let aqua_executable = aqua::install::ensure_installed(
                &self.client,
                &self.config.aqua_version,
                &aqua_root,
                &cache,
            )
            .await?;

            self.write_state(snapshot.tracked_files.clone(), false)
                .await?;

            aqua_executable
        };

        aqua::exec::run_install(&aqua_executable, &self.aqua_config(), &aqua_root).await?;

        for command in &self.config.post_install {
            let aqua_config = self.aqua_config();
            let envs = aqua::exec::aqua_envs(&aqua_executable, &aqua_config, &aqua_root);
            let args = aqua::exec::exec_args(&command.command);
            crate::process::run_foreground(&command.name, &aqua_executable, &args, Some(&envs))
                .await?;
        }

        self.write_state(snapshot.tracked_files, true).await?;
        Ok(())
    }

    async fn write_state(
        &self,
        tracked_files: Vec<FileFingerprint>,
        post_install_completed: bool,
    ) -> Result<()> {
        let state = BootstrapState::new(
            self.config.aqua_version.clone(),
            self.relative_to_root(&self.aqua_executable()),
            tracked_files,
            post_install_completed,
        );

        let cache_for_write = self.bootstrap_cache();
        tokio::task::spawn_blocking(move || state::write_atomic(&cache_for_write, &state))
            .await??;
        Ok(())
    }

    async fn launch_app(&self) -> Result<i32> {
        let mut command = self.config.app.command.clone();
        command.extend(self.app_args.iter().cloned());

        aqua::exec::run_exec(
            "application",
            &self.aqua_executable(),
            &self.aqua_root(),
            &self.aqua_config(),
            &command,
        )
        .await
    }

    async fn acquire_shared_lock(&self) -> Result<BootstrapLock> {
        let cache = self.bootstrap_cache();
        tokio::task::spawn_blocking(move || BootstrapLock::acquire_shared(&cache)).await?
    }

    async fn acquire_exclusive_lock(&self) -> Result<BootstrapLock> {
        let cache = self.bootstrap_cache();
        tokio::task::spawn_blocking(move || BootstrapLock::acquire_exclusive(&cache)).await?
    }

    async fn snapshot(&self) -> Result<Snapshot> {
        let root = self.root.clone();
        let cache = self.bootstrap_cache();
        let tracked_paths = self.config.tracked_files.clone();
        let aqua_executable = self.aqua_executable();

        let state_task = tokio::task::spawn_blocking(move || state::read(&cache));
        let tracked_task = tokio::task::spawn_blocking(move || {
            fingerprint::fingerprint_tracked_patterns(&root, &tracked_paths)
        });
        let executable_task =
            tokio::task::spawn_blocking(move || fingerprint::executable_exists(&aqua_executable));

        let (state, tracked_files, aqua_executable_exists) =
            tokio::try_join!(state_task, tracked_task, executable_task)?;

        Ok(Snapshot {
            state: state?,
            tracked_files: tracked_files?,
            aqua_executable_exists: aqua_executable_exists?,
        })
    }

    fn is_valid(&self, snapshot: &Snapshot) -> bool {
        let Some(state) = &snapshot.state else {
            return false;
        };

        self.is_aqua_binary_cached(snapshot)
            && state.tracked_files == snapshot.tracked_files
            && state.post_install_completed
    }

    fn is_aqua_binary_cached(&self, snapshot: &Snapshot) -> bool {
        let Some(state) = &snapshot.state else {
            return false;
        };

        state.schema == state::STATE_SCHEMA
            && state.aqua_version == self.config.aqua_version
            && state.aqua_executable == self.relative_to_root(&self.aqua_executable())
            && snapshot.aqua_executable_exists
    }

    fn aqua_config(&self) -> PathBuf {
        self.config.aqua_config.clone()
    }

    fn aqua_root(&self) -> PathBuf {
        self.config.aqua_root.clone()
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

#[cfg(test)]
mod tests {
    use super::{Bootstrapper, Snapshot};
    use crate::config::{AppCommand, Config};
    use crate::fingerprint::FileFingerprint;
    use crate::state::BootstrapState;
    use std::path::PathBuf;

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

    fn bootstrapper() -> Bootstrapper {
        let root = absolute_root();
        Bootstrapper::new(
            root.clone(),
            Config {
                schema: 1,
                aqua_version: "v2.59.2".to_string(),
                aqua_config: root.join("aqua.yaml"),
                aqua_root: root.join(".dv").join("aqua"),
                bootstrap_cache: root.join(".dv").join("bootstrap"),
                tracked_files: vec![root.join("aqua.yaml")],
                post_install: vec![],
                app: AppCommand {
                    command: vec!["aqua".to_string(), "--version".to_string()],
                },
            },
            vec![],
        )
    }

    fn state(bootstrapper: &Bootstrapper, tracked_files: Vec<FileFingerprint>) -> BootstrapState {
        BootstrapState::new(
            bootstrapper.config.aqua_version.clone(),
            bootstrapper.relative_to_root(&bootstrapper.aqua_executable()),
            tracked_files,
            true,
        )
    }

    fn fingerprint(path: &str, size: u64) -> FileFingerprint {
        FileFingerprint {
            path: path.into(),
            size,
            mtime_ns: 7,
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
