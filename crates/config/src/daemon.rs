use crate::*;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Default, Debug, Clone, FromDynamic, ToDynamic)]
pub struct DaemonOptions {
    pub pid_file: Option<PathBuf>,
    pub stdout: Option<PathBuf>,
    pub stderr: Option<PathBuf>,
}

/// Set the sticky bit on path.
/// This is used in a couple of situations where we want files that
/// we create in the RUNTIME_DIR to not be removed by a potential
/// tmpwatch daemon.
pub fn set_sticky_bit(path: &Path) {
    // The sticky bit is a Unix filesystem permission concept with no
    // Windows equivalent, so there is nothing to do here.
    let _ = path;
}

fn open_log(path: PathBuf) -> anyhow::Result<File> {
    create_user_owned_dirs(
        path.parent()
            .ok_or_else(|| anyhow!("path {} has no parent dir!?", path.display()))?,
    )?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).append(true);
    options
        .open(&path)
        .map_err(|e| anyhow!("failed to open log stream: {}: {}", path.display(), e))
}

impl DaemonOptions {
    #[allow(dead_code)]
    pub fn pid_file(&self) -> PathBuf {
        self.pid_file
            .as_ref()
            .cloned()
            .unwrap_or_else(|| RUNTIME_DIR.join("pid"))
    }

    pub fn stdout(&self) -> PathBuf {
        self.stdout
            .as_ref()
            .cloned()
            .unwrap_or_else(|| RUNTIME_DIR.join("log"))
    }

    pub fn stderr(&self) -> PathBuf {
        self.stderr
            .as_ref()
            .cloned()
            .unwrap_or_else(|| RUNTIME_DIR.join("log"))
    }

    pub fn open_stdout(&self) -> anyhow::Result<File> {
        open_log(self.stdout())
    }

    pub fn open_stderr(&self) -> anyhow::Result<File> {
        open_log(self.stderr())
    }
}
