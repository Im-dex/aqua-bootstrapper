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
    client: Client,
}

#[derive(Debug)]
struct Snapshot {
    state: Option<BootstrapState>,
    tracked_files: Vec<FileFingerprint>,
    aqua_executable_exists: bool,
}

impl Bootstrapper {
    pub fn new(root: PathBuf, config: Config) -> Self {
        Self {
            root,
            config,
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
                self.bootstrap(snapshot.tracked_files).await?;
            }
            drop(lock);
        }
    }

    async fn bootstrap(&self, tracked_files: Vec<FileFingerprint>) -> Result<()> {
        let aqua_root = self.aqua_root();
        let cache = self.bootstrap_cache();
        let aqua_executable = aqua::install::ensure_installed(
            &self.client,
            &self.config.aqua_version,
            &aqua_root,
            &cache,
        )
        .await?;

        aqua::exec::run_install(&aqua_executable, &self.aqua_config(), &aqua_root).await?;

        for command in &self.config.post_install {
            let aqua_config = self.aqua_config();
            let args = aqua::exec::exec_args(&command.command);
            let envs = aqua::exec::aqua_envs(&aqua_executable, &aqua_config, &aqua_root);
            crate::process::run_foreground(&command.name, &aqua_executable, &args, Some(&envs))
                .await?;
        }

        let state = BootstrapState::new(
            self.config.aqua_version.clone(),
            self.relative_to_root(&aqua_executable),
            tracked_files,
        );

        let cache_for_write = self.bootstrap_cache();
        tokio::task::spawn_blocking(move || state::write_atomic(&cache_for_write, &state))
            .await??;
        Ok(())
    }

    async fn launch_app(&self) -> Result<i32> {
        aqua::exec::run_exec(
            "application",
            &self.aqua_executable(),
            &self.aqua_root(),
            &self.aqua_config(),
            &self.config.app.command,
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
            tracked_paths
                .iter()
                .map(|path| fingerprint::fingerprint_tracked(&root, path))
                .collect::<Result<Vec<_>>>()
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

        state.schema == state::STATE_SCHEMA
            && state.aqua_version == self.config.aqua_version
            && state.aqua_executable == self.relative_to_root(&self.aqua_executable())
            && state.tracked_files == snapshot.tracked_files
            && snapshot.aqua_executable_exists
    }

    fn aqua_config(&self) -> PathBuf {
        self.root.join(&self.config.aqua_config)
    }

    fn aqua_root(&self) -> PathBuf {
        self.root.join(&self.config.aqua_root)
    }

    fn bootstrap_cache(&self) -> PathBuf {
        self.root.join(&self.config.bootstrap_cache)
    }

    fn aqua_executable(&self) -> PathBuf {
        aqua::executable_path(&self.aqua_root())
    }

    fn relative_to_root(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root).unwrap_or(path).to_path_buf()
    }
}
