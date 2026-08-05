use anyhow::{bail, Result};

use super::{Backend, BackendKind, BoundPane, CaptureSpec, CreateSpec, PaneLocation};
use crate::config::Config;
use crate::paths::Paths;
use crate::session::{self, NewOpts, OnExitHook, PaneState, SessionInfo};
use crate::tmux::{exact, tgt, Tmux, TmuxError};

const SEP: char = '\u{1f}';
const PANE_NAME_OPT: &str = "@tpp_name";
const PANE_ROLE_OPT: &str = "@tpp_role";

pub struct TmuxBackend {
    tmux: Tmux,
    cfg: Config,
    paths: Paths,
}

impl TmuxBackend {
    pub fn new(cfg: Config, paths: Paths, socket: Option<String>) -> Self {
        Self {
            tmux: Tmux::new(socket),
            cfg,
            paths,
        }
    }

    fn pane_target(&self, name: &str) -> Option<String> {
        let target = tgt(name);
        if target.starts_with(['%', '@', '$', '{', '!']) {
            return Some(target);
        }
        let Some(pane) = session::origin_pane(&self.tmux, name) else {
            let managed = self
                .tmux
                .run(["show-option", "-qv", "-t", &target, "@tpp"])
                .is_ok_and(|value| value.trim() == "1");
            return (!managed || !session::exists(&self.tmux, name)).then_some(target);
        };
        let resolves = self
            .tmux
            .run(["display-message", "-p", "-t", &pane, "#{pane_id}"])
            .is_ok_and(|resolved| resolved.trim() == pane);
        (resolves || !session::exists(&self.tmux, name)).then_some(pane)
    }

    fn clear_binding(&self, name: &str) -> Result<Vec<BoundPane>> {
        let existing = self
            .list_bindings()?
            .into_iter()
            .filter(|pane| pane.name == name)
            .collect::<Vec<_>>();
        for pane in &existing {
            self.tmux
                .run(["set-option", "-p", "-u", "-t", &pane.pane_id, PANE_NAME_OPT])?;
            let _ = self
                .tmux
                .run(["set-option", "-p", "-u", "-t", &pane.pane_id, PANE_ROLE_OPT]);
        }
        Ok(existing)
    }
}

impl Backend for TmuxBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Tmux
    }

    fn namespace(&self) -> Option<String> {
        self.tmux.store_socket()
    }

    fn selector_args(&self) -> Vec<String> {
        self.tmux
            .socket()
            .map(|socket| vec!["-L".to_string(), socket.to_string()])
            .unwrap_or_default()
    }

    fn target_description(&self) -> String {
        self.tmux
            .socket()
            .map(|socket| format!("tmux socket {socket}"))
            .unwrap_or_else(|| "default tmux server".to_string())
    }

    fn exists(&self, name: &str) -> bool {
        session::exists(&self.tmux, name)
    }

    fn list(&self) -> Result<Vec<SessionInfo>> {
        session::list(&self.tmux)
    }

    fn create(&self, spec: CreateSpec) -> Result<String> {
        let on_exit = spec
            .on_exit
            .map(|command| {
                OnExitHook::new(
                    &self.paths,
                    self.namespace().as_deref(),
                    &spec.name,
                    command,
                )
            })
            .transpose()?;
        session::create(
            &self.tmux,
            &self.cfg,
            NewOpts {
                name: spec.name,
                dir: spec.dir,
                command: spec.command,
                width: None,
                height: None,
                on_exit,
                parent_pane: spec.parent_pane,
                watch: spec.watch,
            },
        )
    }

    fn remove(&self, name: &str) -> Result<()> {
        let hook = session::prepare_on_exit_hook(&self.tmux, name);
        if let Some(hook) = &hook {
            hook.disable_session_closed_hook(&self.tmux);
        }
        self.tmux.run(["kill-session", "-t", &exact(name)])?;
        if let Some(hook) = hook {
            hook.fire(name);
        }
        Ok(())
    }

    fn rename(&self, old: &str, new: &str) -> Result<()> {
        self.tmux.run(["rename-session", "-t", &exact(old), new])?;
        Ok(())
    }

    fn focus(&self, name: &str) -> Result<()> {
        if std::env::var_os("TMUX").is_some() {
            self.tmux.run(["switch-client", "-t", &exact(name)])?;
            return Ok(());
        }
        self.tmux.exec(["attach-session", "-t", &exact(name)])
    }

    fn origin_pane(&self, name: &str) -> Option<String> {
        session::origin_pane(&self.tmux, name)
    }

    fn parent_pane(&self, name: &str) -> Option<String> {
        session::parent_pane(&self.tmux, name)
    }

    fn session_dir(&self, name: &str) -> Option<String> {
        session::session_dir(&self.tmux, name)
    }

    fn current_pane(&self) -> Option<String> {
        std::env::var_os("TMUX")?;
        let pane = std::env::var("TMUX_PANE").ok()?;
        self.canonical_pane(&pane)
    }

    fn current_session(&self) -> Option<String> {
        std::env::var_os("TMUX")?;
        self.tmux
            .run(["display-message", "-p", "#{session_name}"])
            .ok()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
    }

    fn canonical_pane(&self, target: &str) -> Option<String> {
        self.tmux
            .run(["display-message", "-p", "-t", target, "#{pane_id}"])
            .ok()
            .map(|pane| pane.trim().to_string())
            .filter(|pane| !pane.is_empty())
    }

    fn session_for_pane(&self, pane: &str) -> Option<String> {
        self.tmux
            .run(["display-message", "-p", "-t", pane, "#{session_name}"])
            .ok()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
    }

    fn pane_exists(&self, target: &str) -> bool {
        self.canonical_pane(target).is_some()
    }

    fn pane_state(&self, target: &str) -> Option<PaneState> {
        let target = self.pane_target(target)?;
        session::pane_state_for_target(&self.tmux, &target)
    }

    fn capture(&self, target: &str, spec: CaptureSpec) -> Result<String> {
        let target = self
            .pane_target(target)
            .ok_or_else(|| anyhow::anyhow!("origin pane gone"))?;
        let mut args = vec![
            "capture-pane".to_string(),
            "-p".to_string(),
            "-J".to_string(),
            "-t".to_string(),
            target,
        ];
        if spec.escape {
            args.push("-e".to_string());
        }
        if spec.all_history {
            args.extend(["-S".to_string(), "-".to_string()]);
        } else if let Some(lines) = spec.lines.filter(|lines| *lines > 0) {
            args.extend(["-S".to_string(), format!("-{lines}")]);
        }
        self.tmux
            .run(args)
            .map(strip_dead_pane_overlay)
            .map_err(Into::into)
    }

    fn send_text(&self, target: &str, body: &str, bracketed: bool) -> Result<()> {
        let target = self
            .pane_target(target)
            .ok_or_else(|| anyhow::anyhow!("origin pane gone"))?;
        if bracketed {
            let buffer = format!("tpp-{}", std::process::id());
            self.tmux
                .run_stdin(["load-buffer", "-b", &buffer, "-"], body)?;
            self.tmux.run([
                "paste-buffer",
                "-t",
                &tgt(&target),
                "-b",
                &buffer,
                "-p",
                "-d",
            ])?;
        } else {
            self.tmux
                .run(["send-keys", "-t", &tgt(&target), "-l", "--", body])?;
        }
        Ok(())
    }

    fn send_keys(&self, target: &str, keys: &[String]) -> Result<()> {
        let target = self
            .pane_target(target)
            .ok_or_else(|| anyhow::anyhow!("origin pane gone"))?;
        let mut args = vec!["send-keys".to_string(), "-t".to_string(), tgt(&target)];
        args.extend(keys.iter().cloned());
        self.tmux.run(args)?;
        Ok(())
    }

    fn set_watch_armed(&self, name: &str, armed: bool) -> Result<()> {
        session::set_watch_armed(&self.tmux, name, armed);
        Ok(())
    }

    fn watch_armed(&self, name: &str) -> bool {
        session::watch_armed(&self.tmux, name)
    }

    fn inspect_pane(&self, target: &str) -> Result<PaneLocation> {
        let fmt = [
            "#{pane_id}",
            "#{session_name}",
            "#{window_index}",
            "#{pane_index}",
        ]
        .join(&SEP.to_string());
        let raw = self
            .tmux
            .run(["display-message", "-p", "-t", &tgt(target), &fmt])?;
        let fields = raw.split(SEP).collect::<Vec<_>>();
        if fields.len() < 4 || fields[0].trim().is_empty() {
            bail!("tmux did not return pane metadata for {target}");
        }
        Ok(PaneLocation {
            pane_id: fields[0].to_string(),
            session: fields[1].to_string(),
            window: fields[2].to_string(),
            pane: fields[3].to_string(),
            location: format!("{}:{}.{}", fields[1], fields[2], fields[3]),
        })
    }

    fn list_bindings(&self) -> Result<Vec<BoundPane>> {
        let fmt = [
            "#{pane_id}",
            "#{session_name}",
            "#{window_index}",
            "#{pane_index}",
            "#{pane_dead}",
            "#{@tpp_name}",
            "#{@tpp_role}",
        ]
        .join(&SEP.to_string());
        let raw = match self.tmux.run(["list-panes", "-a", "-F", &fmt]) {
            Ok(raw) => raw,
            Err(TmuxError::NoServer) => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        Ok(raw
            .lines()
            .filter_map(|line| {
                let fields = line.split(SEP).collect::<Vec<_>>();
                if fields.len() < 7 || fields[5].trim().is_empty() {
                    return None;
                }
                Some(BoundPane {
                    name: fields[5].trim().to_string(),
                    role: fields[6].trim().to_string(),
                    pane_id: fields[0].to_string(),
                    location: format!("{}:{}.{}", fields[1], fields[2], fields[3]),
                    session: fields[1].to_string(),
                    window: fields[2].to_string(),
                    pane: fields[3].to_string(),
                    status: if fields[4].trim() == "1" {
                        "dead"
                    } else {
                        "live"
                    }
                    .to_string(),
                })
            })
            .collect())
    }

    fn bind_pane(&self, name: &str, role: &str, pane: &str) -> Result<Vec<BoundPane>> {
        let previous = self.clear_binding(name)?;
        self.tmux
            .run(["set-option", "-p", "-t", pane, PANE_NAME_OPT, name])?;
        self.tmux
            .run(["set-option", "-p", "-t", pane, PANE_ROLE_OPT, role])?;
        Ok(previous)
    }

    fn unbind_pane(&self, name: &str) -> Result<Vec<BoundPane>> {
        self.clear_binding(name)
    }

    fn compat_run(&self, args: Vec<String>) -> Result<String> {
        self.tmux.run(args).map_err(Into::into)
    }

    fn compat_ok(&self, args: Vec<String>) -> bool {
        self.tmux.ok(args)
    }

    fn compat_exec(&self, args: Vec<String>) -> Result<()> {
        self.tmux.exec(args)
    }
}

fn strip_dead_pane_overlay(raw: String) -> String {
    let lines = raw.lines().collect::<Vec<_>>();
    let Some(last) = lines.iter().rposition(|line| !line.trim().is_empty()) else {
        return raw;
    };
    let stripped = crate::commands::io::strip_ansi(lines[last]);
    let value = stripped.trim();
    if value != "Pane is dead" && !(value.starts_with("Pane is dead (") && value.ends_with(')')) {
        return raw;
    }
    lines[..last].join("\n")
}
