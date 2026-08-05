use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};
use serde::Serialize;

use crate::config::Config;
use crate::paths::Paths;
use crate::session::{self, PaneState, SessionInfo};

mod herdr;
mod tmux;

pub use herdr::HerdrBackend;
pub use tmux::TmuxBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Tmux,
    Herdr,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tmux => "tmux",
            Self::Herdr => "herdr",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateSpec {
    pub name: String,
    pub dir: Option<String>,
    pub command: Vec<String>,
    pub on_exit: Option<String>,
    pub parent_pane: Option<String>,
    pub watch: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct CaptureSpec {
    pub lines: Option<u32>,
    pub escape: bool,
    pub all_history: bool,
}

#[derive(Debug, Clone)]
pub struct PaneLocation {
    pub pane_id: String,
    pub session: String,
    pub window: String,
    pub pane: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoundPane {
    pub name: String,
    pub role: String,
    pub pane_id: String,
    pub location: String,
    pub session: String,
    pub window: String,
    pub pane: String,
    pub status: String,
}

pub trait Backend {
    fn kind(&self) -> BackendKind;
    fn namespace(&self) -> Option<String>;
    fn selector_args(&self) -> Vec<String>;
    fn target_description(&self) -> String;
    fn exists(&self, name: &str) -> bool;
    fn list(&self) -> Result<Vec<SessionInfo>>;
    fn create(&self, spec: CreateSpec) -> Result<String>;
    fn remove(&self, name: &str) -> Result<()>;
    fn rename(&self, old: &str, new: &str) -> Result<()>;
    fn focus(&self, name: &str) -> Result<()>;
    fn origin_pane(&self, name: &str) -> Option<String>;
    fn parent_pane(&self, name: &str) -> Option<String>;
    fn session_dir(&self, name: &str) -> Option<String>;
    fn current_pane(&self) -> Option<String>;
    fn current_session(&self) -> Option<String>;
    fn canonical_pane(&self, target: &str) -> Option<String>;
    fn session_for_pane(&self, pane: &str) -> Option<String>;
    fn pane_exists(&self, target: &str) -> bool;
    fn pane_state(&self, target: &str) -> Option<PaneState>;
    fn capture(&self, target: &str, spec: CaptureSpec) -> Result<String>;
    fn send_text(&self, target: &str, body: &str, bracketed: bool) -> Result<()>;
    fn send_keys(&self, target: &str, keys: &[String]) -> Result<()>;
    fn submit_text(&self, target: &str, body: &str, bracketed: bool) -> Result<()> {
        self.send_text(target, body, bracketed)?;
        self.send_keys(target, &["Enter".to_string()])
    }
    fn set_watch_armed(&self, name: &str, armed: bool) -> Result<()>;
    fn watch_armed(&self, name: &str) -> bool;
    fn inspect_pane(&self, target: &str) -> Result<PaneLocation>;
    fn list_bindings(&self) -> Result<Vec<BoundPane>>;
    fn bind_pane(&self, name: &str, role: &str, pane: &str) -> Result<Vec<BoundPane>>;
    fn unbind_pane(&self, name: &str) -> Result<Vec<BoundPane>>;
    fn live_activity_supported(&self) -> bool {
        true
    }
    fn compat_run(&self, _args: Vec<String>) -> Result<String> {
        bail!(
            "tmux compatibility commands are unavailable with {} backend",
            self.kind().as_str()
        )
    }
    fn compat_ok(&self, _args: Vec<String>) -> bool {
        false
    }
    fn compat_exec(&self, _args: Vec<String>) -> Result<()> {
        bail!(
            "tmux compatibility commands are unavailable with {} backend",
            self.kind().as_str()
        )
    }

    fn resolve_name(&self, cfg: &Config, name: &str) -> String {
        let prefixed = session::prefixed_name(cfg, name);
        if self.exists(&prefixed) {
            return prefixed;
        }
        let raw = name.trim().trim_start_matches('=').to_string();
        if raw != prefixed && self.exists(&raw) {
            raw
        } else {
            prefixed
        }
    }

    fn is_alive(&self, name: &str) -> bool {
        self.exists(name) && self.pane_state(name).is_some_and(|pane| !pane.dead)
    }
}

pub type BackendRef = Arc<dyn Backend>;

pub fn select(
    cfg: &Config,
    paths: &Paths,
    config_path: PathBuf,
    socket: Option<String>,
) -> BackendRef {
    if cfg.herdr_mode {
        Arc::new(HerdrBackend::new(cfg.clone(), paths.clone(), config_path))
    } else {
        Arc::new(TmuxBackend::new(cfg.clone(), paths.clone(), socket))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{select, BackendKind};
    use crate::config::Config;
    use crate::paths::Paths;

    fn paths() -> Paths {
        Paths {
            config_dir: PathBuf::from("/config"),
            state_dir: PathBuf::from("/state"),
        }
    }

    #[test]
    fn backend_selection_defaults_to_tmux() {
        let backend = select(
            &Config::default(),
            &paths(),
            PathBuf::from("/config/tpp.toml"),
            None,
        );

        assert_eq!(backend.kind(), BackendKind::Tmux);
    }

    #[test]
    fn backend_selection_honors_herdr_mode() {
        let config = Config {
            herdr_mode: true,
            ..Config::default()
        };
        let backend = select(&config, &paths(), PathBuf::from("/config/tpp.toml"), None);

        assert_eq!(backend.kind(), BackendKind::Herdr);
    }
}
