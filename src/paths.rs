//! Filesystem locations for tpp config and persisted data.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const DEFAULT_CONFIG_SUFFIX: &[&str] = &[".config", "tpp"];
const DEFAULT_STATE_SUFFIX: &[&str] = &[".tpp", "data"];

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .context("HOME is not set")?;
        let config_dir = resolve("TPP_CONFIG_DIR", &home, DEFAULT_CONFIG_SUFFIX);
        let state_dir = resolve("TPP_STATE_DIR", &home, DEFAULT_STATE_SUFFIX);
        Ok(Self {
            config_dir,
            state_dir,
        })
    }

    pub fn config_file(&self) -> PathBuf {
        std::env::var_os("TPP_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.config_dir.join("config.toml"))
    }

    pub fn exited_dir(&self) -> PathBuf {
        self.state_dir.join("exited")
    }
}

fn resolve(env: &str, home: &Path, suffix: &[&str]) -> PathBuf {
    if let Some(dir) = std::env::var_os(env) {
        return PathBuf::from(dir);
    }
    default_path(home, suffix)
}

fn default_path(home: &Path, suffix: &[&str]) -> PathBuf {
    let mut p = home.to_path_buf();
    for s in suffix {
        p.push(s);
    }
    p
}

/// Create a directory (and parents) readable only by the owner — these hold session
/// transcripts that can contain whatever scrolled past in an agent's terminal.
pub fn create_private_dir_all(dir: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("creating {}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_dir_is_tpp_data() {
        let home = PathBuf::from("/Users/shadowfax");

        assert_eq!(
            default_path(&home, DEFAULT_STATE_SUFFIX),
            PathBuf::from("/Users/shadowfax/.tpp/data")
        );
    }
}
