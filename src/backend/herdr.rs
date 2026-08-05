use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Backend, BackendKind, BoundPane, CaptureSpec, CreateSpec, PaneLocation};
use crate::config::Config;
use crate::paths::{create_private_dir_all, encode_state_component, Paths};
use crate::session::{now_epoch, storage_prefix, PaneState, SessionInfo};

const NAMESPACE: &str = "herdr:default";
const WORKSPACE_LABEL: &str = "tpp";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HookRecord {
    command_file: PathBuf,
    marker: PathBuf,
    log: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionRecord {
    dir: String,
    command: String,
    created: i64,
    tab_id: String,
    pane_id: String,
    parent_pane: Option<String>,
    watch: bool,
    shell: bool,
    status_file: PathBuf,
    name_file: PathBuf,
    runner_file: Option<PathBuf>,
    hook: Option<HookRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BindingRecord {
    role: String,
    pane_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Registry {
    #[serde(default = "registry_version")]
    version: u32,
    workspace_id: Option<String>,
    #[serde(default)]
    sessions: BTreeMap<String, SessionRecord>,
    #[serde(default)]
    bindings: BTreeMap<String, BindingRecord>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: registry_version(),
            workspace_id: None,
            sessions: BTreeMap::new(),
            bindings: BTreeMap::new(),
        }
    }
}

fn registry_version() -> u32 {
    1
}

struct LockedRegistry {
    _lock: File,
    registry: Registry,
}

#[derive(Default)]
struct Inventory {
    workspaces: HashSet<String>,
    tabs: HashMap<String, bool>,
    panes: HashSet<String>,
}

pub struct HerdrBackend {
    cfg: Config,
    paths: Paths,
    config_path: PathBuf,
    client: OsString,
}

impl HerdrBackend {
    pub fn new(cfg: Config, paths: Paths, config_path: PathBuf) -> Self {
        Self {
            cfg,
            paths,
            config_path,
            client: OsString::from("herdr"),
        }
    }

    fn state_root(&self) -> PathBuf {
        self.paths.socket_state_dir("herdr", Some(NAMESPACE))
    }

    fn registry_path(&self) -> PathBuf {
        self.state_root().join("registry.json")
    }

    fn lock_registry(&self) -> Result<LockedRegistry> {
        let root = self.state_root();
        create_private_dir_all(&root)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(root.join("registry.lock"))
            .context("opening Herdr registry lock")?;
        loop {
            // SAFETY: flock only reads the valid file descriptor and holds the lock until close.
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } == 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error).context("locking Herdr registry");
            }
        }
        let registry = match std::fs::read_to_string(self.registry_path()) {
            Ok(raw) => serde_json::from_str(&raw).context("parsing Herdr registry")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Registry::default(),
            Err(error) => return Err(error).context("reading Herdr registry"),
        };
        Ok(LockedRegistry {
            _lock: lock,
            registry,
        })
    }

    fn save_registry(&self, locked: &LockedRegistry) -> Result<()> {
        let path = self.registry_path();
        let temp = path.with_extension(format!("tmp.{}", std::process::id()));
        let data = serde_json::to_vec_pretty(&locked.registry)?;
        write_private(&temp, &data, 0o600)?;
        std::fs::rename(&temp, &path).with_context(|| format!("publishing {}", path.display()))
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.client);
        command.args(["--session", "default"]);
        command
    }

    fn run<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self
            .command()
            .args(args)
            .output()
            .context("running Herdr CLI")?;
        output_text(output)
    }

    fn run_json<I, S>(&self, args: I) -> Result<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let raw = self.run(args)?;
        serde_json::from_str(&raw).with_context(|| format!("parsing Herdr response: {raw}"))
    }

    fn inventory(&self) -> Result<Inventory> {
        let workspace_value = self.run_json(["workspace", "list"])?;
        let tab_value = self.run_json(["tab", "list"])?;
        let pane_value = self.run_json(["pane", "list"])?;
        let workspaces = result_array(&workspace_value, "workspaces")?
            .iter()
            .filter_map(|value| string_field(value, "workspace_id"))
            .collect();
        let tabs = result_array(&tab_value, "tabs")?
            .iter()
            .filter_map(|value| {
                Some((
                    string_field(value, "tab_id")?,
                    value
                        .get("focused")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                ))
            })
            .collect();
        let panes = result_array(&pane_value, "panes")?
            .iter()
            .filter_map(|value| string_field(value, "pane_id"))
            .collect();
        Ok(Inventory {
            workspaces,
            tabs,
            panes,
        })
    }

    fn reconcile(&self, locked: &mut LockedRegistry) -> Result<Vec<(String, SessionRecord)>> {
        let inventory = self.inventory()?;
        let workspace_exists = locked
            .registry
            .workspace_id
            .as_ref()
            .is_some_and(|id| inventory.workspaces.contains(id));
        let stale = locked
            .registry
            .sessions
            .iter()
            .filter(|(_, record)| {
                !workspace_exists
                    || !inventory.tabs.contains_key(&record.tab_id)
                    || !inventory.panes.contains(&record.pane_id)
            })
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let mut removed = Vec::new();
        for name in stale {
            if let Some(record) = locked.registry.sessions.remove(&name) {
                removed.push((name, record));
            }
        }
        locked
            .registry
            .bindings
            .retain(|_, binding| inventory.panes.contains(&binding.pane_id));
        if !workspace_exists {
            locked.registry.workspace_id = None;
        }
        self.save_registry(locked)?;
        Ok(removed)
    }

    fn fire_removed_hooks(&self, removed: Vec<(String, SessionRecord)>) {
        for (name, record) in removed {
            let status = read_exit_status(&record.status_file);
            self.fire_hook(&name, &record, status);
        }
    }

    fn workspace_exists(&self, workspace_id: &str) -> bool {
        self.run(["workspace", "get", workspace_id]).is_ok()
    }

    fn create_tab(
        &self,
        workspace: Option<&str>,
        spec: &CreateSpec,
        dir: &str,
    ) -> Result<(String, String, String)> {
        let tab_label = self.tab_label(&spec.name);
        let env = [
            format!("TPP_SESSION_NAME={}", spec.name),
            format!("TPP_SESSION={}", spec.name),
            format!("TPP_CONFIG={}", self.config_path.display()),
            format!("TPP_STATE_DIR={}", self.paths.state_dir.display()),
        ];
        let mut args = if let Some(workspace) = workspace {
            vec![
                "tab".to_string(),
                "create".to_string(),
                "--workspace".to_string(),
                workspace.to_string(),
                "--cwd".to_string(),
                dir.to_string(),
                "--label".to_string(),
                tab_label.clone(),
            ]
        } else {
            vec![
                "workspace".to_string(),
                "create".to_string(),
                "--cwd".to_string(),
                dir.to_string(),
                "--label".to_string(),
                WORKSPACE_LABEL.to_string(),
            ]
        };
        for value in env {
            args.push("--env".to_string());
            args.push(value);
        }
        args.push("--no-focus".to_string());
        let value = self.run_json(args)?;
        let result = value
            .get("result")
            .context("Herdr response is missing result")?;
        let tab = result.get("tab").context("Herdr response is missing tab")?;
        let pane = result
            .get("root_pane")
            .context("Herdr response is missing root pane")?;
        let tab_id = required_string(tab, "tab_id")?;
        let pane_id = required_string(pane, "pane_id")?;
        let workspace_id = required_string(tab, "workspace_id")?;
        if workspace.is_none() {
            if let Err(error) = self.run(["tab", "rename", &tab_id, &tab_label]) {
                let _ = self.run(["tab", "close", &tab_id]);
                return Err(error);
            }
        }
        Ok((workspace_id, tab_id, pane_id))
    }

    fn tab_label(&self, name: &str) -> String {
        let prefix = storage_prefix(&self.cfg);
        name.strip_prefix(&prefix)
            .filter(|label| !label.is_empty())
            .unwrap_or(name)
            .to_string()
    }

    fn session_state_dir(&self, name: &str, tab_id: &str) -> PathBuf {
        self.state_root().join("sessions").join(format!(
            "{}-{}",
            encode_state_component(name),
            encode_state_component(tab_id)
        ))
    }

    fn prepare_record(
        &self,
        spec: &CreateSpec,
        dir: String,
        tab_id: String,
        pane_id: String,
    ) -> Result<(SessionRecord, Option<String>)> {
        let root = self.session_state_dir(&spec.name, &tab_id);
        create_private_dir_all(&root)?;
        let status_file = root.join("status");
        let name_file = root.join("name");
        write_private(&name_file, spec.name.as_bytes(), 0o600)?;
        let shell = spec.command.is_empty();
        let launch = if shell {
            self.cfg
                .shell
                .as_ref()
                .filter(|shell| !shell.trim().is_empty())
                .map(|shell| format!("exec {}", shell_quote(shell)))
        } else {
            Some(String::new())
        };
        let command = if shell {
            self.cfg
                .shell
                .clone()
                .filter(|shell| !shell.trim().is_empty())
                .unwrap_or_else(|| "shell".to_string())
        } else {
            spec.command.join(" ")
        };
        let hook = spec
            .on_exit
            .as_ref()
            .map(|command| {
                let hook = HookRecord {
                    command_file: root.join("on-exit.cmd"),
                    marker: root.join("on-exit.once"),
                    log: self.state_root().join("on-exit.log"),
                };
                write_private(&hook.command_file, command.as_bytes(), 0o600)?;
                Ok::<_, anyhow::Error>(hook)
            })
            .transpose()?;
        let runner_file = if shell {
            write_private(&status_file, b"shell\n", 0o600)?;
            None
        } else {
            write_private(&status_file, b"running\n", 0o600)?;
            let runner = root.join("run.sh");
            let script = runner_script(&status_file, &name_file, hook.as_ref());
            write_private(&runner, script.as_bytes(), 0o700)?;
            Some(runner)
        };
        let record = SessionRecord {
            dir,
            command,
            created: now_epoch(),
            tab_id,
            pane_id,
            parent_pane: spec.parent_pane.clone(),
            watch: spec.watch,
            shell,
            status_file,
            name_file,
            runner_file: runner_file.clone(),
            hook,
        };
        let launch = if let Some(runner) = runner_file {
            let mut command = "printf '\\033[3J\\033[H\\033[2J'; ".to_string();
            command.push_str(&shell_quote(&runner.to_string_lossy()));
            for arg in &spec.command {
                command.push(' ');
                command.push_str(&shell_quote(arg));
            }
            Some(command)
        } else {
            launch
        };
        Ok((record, launch))
    }

    fn record(&self, target: &str) -> Option<(String, SessionRecord)> {
        let target = target.trim().trim_start_matches('=');
        let locked = self.lock_registry().ok()?;
        if let Some(record) = locked.registry.sessions.get(target) {
            return Some((target.to_string(), record.clone()));
        }
        locked
            .registry
            .sessions
            .iter()
            .find(|(_, record)| record.pane_id == target)
            .map(|(name, record)| (name.clone(), record.clone()))
    }

    fn pane_id(&self, target: &str) -> Option<String> {
        let target = target.trim().trim_start_matches('=');
        if let Some((_, record)) = self.record(target) {
            return Some(record.pane_id);
        }
        let value = self.run_json(["pane", "get", target]).ok()?;
        value
            .pointer("/result/pane/pane_id")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    fn pane_process_state(&self, pane_id: &str, activity: i64) -> PaneState {
        let value = self
            .run_json(["pane", "process-info", "--pane", pane_id])
            .ok();
        let info = value
            .as_ref()
            .and_then(|value| value.pointer("/result/process_info"));
        let pid = info
            .and_then(|value| value.get("foreground_process_group_id"))
            .and_then(Value::as_u64)
            .or_else(|| {
                info.and_then(|value| value.get("shell_pid"))
                    .and_then(Value::as_u64)
            })
            .and_then(|pid| u32::try_from(pid).ok());
        PaneState {
            dead: false,
            pid,
            exit_status: None,
            activity: Some(activity),
        }
    }

    fn viewport_rows(&self, pane_id: &str) -> u32 {
        self.run_json(["pane", "get", pane_id])
            .ok()
            .and_then(|value| {
                value
                    .pointer("/result/pane/scroll/viewport_rows")
                    .and_then(Value::as_u64)
            })
            .and_then(|rows| u32::try_from(rows).ok())
            .unwrap_or(100)
    }

    fn available_rows(&self, pane_id: &str) -> u64 {
        self.run_json(["pane", "get", pane_id])
            .ok()
            .and_then(|value| {
                let scroll = value.pointer("/result/pane/scroll")?;
                let history = scroll.get("max_offset_from_bottom")?.as_u64()?;
                let viewport = scroll.get("viewport_rows")?.as_u64()?;
                Some(history.saturating_add(viewport))
            })
            .unwrap_or(1000)
    }

    fn record_state(&self, record: &SessionRecord) -> Option<PaneState> {
        self.run(["pane", "get", &record.pane_id]).ok()?;
        if !record.shell {
            if let Some(status) = read_exit_status(&record.status_file) {
                return Some(PaneState {
                    dead: true,
                    pid: None,
                    exit_status: Some(status),
                    activity: Some(record.created),
                });
            }
        }
        Some(self.pane_process_state(&record.pane_id, record.created))
    }

    fn fire_hook(&self, name: &str, record: &SessionRecord, status: Option<i32>) {
        let Some(hook) = &record.hook else {
            return;
        };
        match std::fs::create_dir(&hook.marker) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return,
            Err(error) => {
                append_hook_log(&hook.log, &format!("claiming hook for {name}: {error}"));
                return;
            }
        }
        let command = match std::fs::read_to_string(&hook.command_file) {
            Ok(command) => command,
            Err(error) => {
                append_hook_log(&hook.log, &format!("reading hook for {name}: {error}"));
                return;
            }
        };
        let result = Command::new("sh")
            .args(["-c", &command])
            .env("TPP_SESSION_NAME", name)
            .env("TPP_SESSION", name)
            .env(
                "TPP_EXIT_STATUS",
                status.map(|value| value.to_string()).unwrap_or_default(),
            )
            .status();
        match result {
            Ok(status) if status.success() => {}
            Ok(status) => append_hook_log(
                &hook.log,
                &format!(
                    "hook failed for {name}: exit {}",
                    status.code().unwrap_or(1)
                ),
            ),
            Err(error) => append_hook_log(&hook.log, &format!("running hook for {name}: {error}")),
        }
    }

    fn ensure_writable(&self, pane_id: &str) -> Result<()> {
        if let Some((name, record)) = self.record(pane_id) {
            if self.record_state(&record).is_some_and(|state| state.dead) {
                bail!("session {name} has exited");
            }
        }
        Ok(())
    }

    fn binding_row(&self, name: &str, binding: &BindingRecord) -> Option<BoundPane> {
        let location = self.inspect_pane(&binding.pane_id).ok()?;
        let dead = self
            .record(&binding.pane_id)
            .and_then(|(_, record)| self.record_state(&record))
            .is_some_and(|state| state.dead);
        let status = if dead { "dead" } else { "live" }.to_string();
        Some(BoundPane {
            name: name.to_string(),
            role: binding.role.clone(),
            pane_id: location.pane_id,
            location: location.location,
            session: location.session,
            window: location.window,
            pane: location.pane,
            status,
        })
    }
}

impl Backend for HerdrBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Herdr
    }

    fn namespace(&self) -> Option<String> {
        Some(NAMESPACE.to_string())
    }

    fn selector_args(&self) -> Vec<String> {
        Vec::new()
    }

    fn target_description(&self) -> String {
        let workspace = self
            .lock_registry()
            .ok()
            .and_then(|locked| locked.registry.workspace_id.clone());
        match workspace {
            Some(workspace) => format!("Herdr default session (tpp workspace {workspace})"),
            None => "Herdr default session (tpp workspace not created)".to_string(),
        }
    }

    fn exists(&self, name: &str) -> bool {
        self.record(name)
            .is_some_and(|(_, record)| self.run(["pane", "get", &record.pane_id]).is_ok())
    }

    fn list(&self) -> Result<Vec<SessionInfo>> {
        let mut locked = self.lock_registry()?;
        let removed = self.reconcile(&mut locked)?;
        let inventory = self.inventory()?;
        let records = locked
            .registry
            .sessions
            .iter()
            .map(|(name, record)| (name.clone(), record.clone()))
            .collect::<Vec<_>>();
        drop(locked);
        self.fire_removed_hooks(removed);
        let mut sessions = records
            .into_iter()
            .filter_map(|(name, record)| {
                let state = self.record_state(&record)?;
                Some(SessionInfo {
                    name,
                    dir: record.dir,
                    command: record.command,
                    created: record.created,
                    activity: state.activity.unwrap_or(record.created),
                    attached: inventory.tabs.get(&record.tab_id).copied().unwrap_or(false),
                    windows: 1,
                    parent_pane: record.parent_pane,
                    dead: state.dead,
                    pid: state.pid,
                    exit_status: state.exit_status,
                    exited: state.dead,
                })
            })
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.created));
        Ok(sessions)
    }

    fn create(&self, spec: CreateSpec) -> Result<String> {
        let mut locked = self.lock_registry()?;
        let removed = self.reconcile(&mut locked)?;
        drop(locked);
        self.fire_removed_hooks(removed);
        let mut locked = self.lock_registry()?;
        if locked.registry.sessions.contains_key(&spec.name) {
            bail!("session already exists: {}", spec.name);
        }
        let dir = spec.dir.clone().unwrap_or_else(|| ".".to_string());
        let existing_workspace = locked.registry.workspace_id.clone();
        let (workspace_id, tab_id, pane_id) =
            self.create_tab(existing_workspace.as_deref(), &spec, &dir)?;
        let prepared = self.prepare_record(&spec, dir, tab_id.clone(), pane_id.clone());
        let (record, launch) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = self.run(["tab", "close", &tab_id]);
                return Err(error);
            }
        };
        locked.registry.workspace_id = Some(workspace_id);
        locked.registry.sessions.insert(spec.name.clone(), record);
        if let Err(error) = self.save_registry(&locked) {
            locked.registry.sessions.remove(&spec.name);
            let _ = self.run(["tab", "close", &tab_id]);
            return Err(error);
        }
        if let Some(command) = launch {
            if let Err(error) = self.run(["pane", "run", &pane_id, &command]) {
                locked.registry.sessions.remove(&spec.name);
                if locked.registry.sessions.is_empty() {
                    locked.registry.workspace_id = None;
                }
                let _ = self.run(["tab", "close", &tab_id]);
                let save_result = self.save_registry(&locked);
                drop(locked);
                save_result?;
                return Err(error);
            }
        }
        drop(locked);
        Ok(spec.name)
    }

    fn remove(&self, name: &str) -> Result<()> {
        let mut locked = self.lock_registry()?;
        let record = locked
            .registry
            .sessions
            .get(name)
            .cloned()
            .with_context(|| format!("no such Herdr session {name}"))?;
        let status = read_exit_status(&record.status_file);
        if self.run(["pane", "get", &record.pane_id]).is_ok() {
            self.run(["tab", "close", &record.tab_id])?;
        }
        locked.registry.sessions.remove(name);
        if locked.registry.sessions.is_empty() {
            let keep = locked
                .registry
                .workspace_id
                .as_deref()
                .is_some_and(|workspace| self.workspace_exists(workspace));
            if !keep {
                locked.registry.workspace_id = None;
            }
        }
        self.save_registry(&locked)?;
        drop(locked);
        self.fire_hook(name, &record, status);
        Ok(())
    }

    fn rename(&self, old: &str, new: &str) -> Result<()> {
        let mut locked = self.lock_registry()?;
        if locked.registry.sessions.contains_key(new) {
            bail!("session already exists: {new}");
        }
        let record = locked
            .registry
            .sessions
            .get(old)
            .cloned()
            .with_context(|| format!("no such Herdr session {old}"))?;
        let old_label = self.tab_label(old);
        let new_label = self.tab_label(new);
        self.run(["tab", "rename", &record.tab_id, &new_label])?;
        if let Err(error) = write_private(&record.name_file, new.as_bytes(), 0o600) {
            let _ = self.run(["tab", "rename", &record.tab_id, &old_label]);
            return Err(error);
        }
        locked.registry.sessions.remove(old);
        locked.registry.sessions.insert(new.to_string(), record);
        if let Err(error) = self.save_registry(&locked) {
            let record = locked
                .registry
                .sessions
                .remove(new)
                .expect("renamed record");
            locked.registry.sessions.insert(old.to_string(), record);
            let _ = write_private(
                &locked.registry.sessions[old].name_file,
                old.as_bytes(),
                0o600,
            );
            let _ = self.run([
                "tab",
                "rename",
                &locked.registry.sessions[old].tab_id,
                &old_label,
            ]);
            return Err(error);
        }
        Ok(())
    }

    fn focus(&self, name: &str) -> Result<()> {
        let locked = self.lock_registry()?;
        let record = locked
            .registry
            .sessions
            .get(name.trim().trim_start_matches('='))
            .cloned()
            .with_context(|| format!("no such Herdr session {name}"))?;
        let workspace = locked
            .registry
            .workspace_id
            .clone()
            .context("tpp Herdr workspace is missing")?;
        drop(locked);
        self.run(["workspace", "focus", &workspace])?;
        self.run(["tab", "focus", &record.tab_id])?;
        if std::env::var_os("HERDR_ENV").is_some() {
            return Ok(());
        }
        let error = self.command().args(["session", "attach", "default"]).exec();
        Err(error).context("attaching to Herdr default session")
    }

    fn origin_pane(&self, name: &str) -> Option<String> {
        self.record(name).map(|(_, record)| record.pane_id)
    }

    fn parent_pane(&self, name: &str) -> Option<String> {
        self.record(name).and_then(|(_, record)| record.parent_pane)
    }

    fn session_dir(&self, name: &str) -> Option<String> {
        self.record(name).map(|(_, record)| record.dir)
    }

    fn current_pane(&self) -> Option<String> {
        std::env::var_os("HERDR_ENV")?;
        let pane = std::env::var("HERDR_PANE_ID").ok()?;
        self.canonical_pane(&pane)
    }

    fn current_session(&self) -> Option<String> {
        let pane = self.current_pane()?;
        self.session_for_pane(&pane)
    }

    fn canonical_pane(&self, target: &str) -> Option<String> {
        self.pane_id(target)
    }

    fn session_for_pane(&self, pane: &str) -> Option<String> {
        self.record(pane).map(|(name, _)| name)
    }

    fn pane_exists(&self, target: &str) -> bool {
        self.pane_id(target)
            .is_some_and(|pane| self.run(["pane", "get", &pane]).is_ok())
    }

    fn pane_state(&self, target: &str) -> Option<PaneState> {
        if let Some((_, record)) = self.record(target) {
            return self.record_state(&record);
        }
        let pane = self.pane_id(target)?;
        self.run(["pane", "get", &pane]).ok()?;
        Some(self.pane_process_state(&pane, now_epoch()))
    }

    fn capture(&self, target: &str, spec: CaptureSpec) -> Result<String> {
        let pane = self
            .pane_id(target)
            .with_context(|| format!("pane not found: {target}"))?;
        let mut args = vec!["pane".to_string(), "read".to_string(), pane];
        if spec.lines == Some(0) && !spec.all_history {
            args.extend(["--source".to_string(), "visible".to_string()]);
        } else {
            args.extend(["--source".to_string(), "recent-unwrapped".to_string()]);
            if spec.all_history {
                args.extend([
                    "--lines".to_string(),
                    self.available_rows(&args[2]).to_string(),
                ]);
            } else {
                if let Some(lines) = spec.lines.filter(|lines| *lines > 0) {
                    let requested = lines.saturating_add(self.viewport_rows(&args[2]));
                    args.extend(["--lines".to_string(), requested.to_string()]);
                }
            }
        }
        if spec.escape {
            args.extend(["--format".to_string(), "ansi".to_string()]);
        }
        let raw = self.run(args)?;
        if spec.all_history || spec.lines == Some(0) {
            return Ok(raw);
        }
        Ok(spec
            .lines
            .filter(|lines| *lines > 0)
            .map(|lines| trailing_lines(&raw, lines as usize))
            .unwrap_or(raw))
    }

    fn send_text(&self, target: &str, body: &str, _bracketed: bool) -> Result<()> {
        let pane = self
            .pane_id(target)
            .with_context(|| format!("pane not found: {target}"))?;
        self.ensure_writable(&pane)?;
        self.run(["pane", "send-text", &pane, body])?;
        Ok(())
    }

    fn send_keys(&self, target: &str, keys: &[String]) -> Result<()> {
        let pane = self
            .pane_id(target)
            .with_context(|| format!("pane not found: {target}"))?;
        self.ensure_writable(&pane)?;
        let mut args = vec!["pane".to_string(), "send-keys".to_string(), pane];
        args.extend(keys.iter().cloned());
        self.run(args)?;
        Ok(())
    }

    fn submit_text(
        &self,
        target: &str,
        body: &str,
        bracketed: bool,
        enter_delay_ms: u64,
    ) -> Result<()> {
        if !bracketed && enter_delay_ms > 0 {
            self.send_text(target, body, false)?;
            std::thread::sleep(std::time::Duration::from_millis(enter_delay_ms));
            return self.send_keys(target, &["Enter".to_string()]);
        }
        let pane = self
            .pane_id(target)
            .with_context(|| format!("pane not found: {target}"))?;
        self.ensure_writable(&pane)?;
        self.run(["pane", "run", &pane, body])?;
        Ok(())
    }

    fn set_watch_armed(&self, name: &str, armed: bool) -> Result<()> {
        let mut locked = self.lock_registry()?;
        let target = name.trim().trim_start_matches('=');
        let record = if locked.registry.sessions.contains_key(target) {
            locked.registry.sessions.get_mut(target)
        } else {
            locked
                .registry
                .sessions
                .values_mut()
                .find(|record| record.pane_id == target)
        };
        if let Some(record) = record {
            record.watch = armed;
            self.save_registry(&locked)?;
        }
        Ok(())
    }

    fn watch_armed(&self, name: &str) -> bool {
        self.record(name).is_some_and(|(_, record)| record.watch)
    }

    fn inspect_pane(&self, target: &str) -> Result<PaneLocation> {
        let pane_id = self
            .pane_id(target)
            .with_context(|| format!("pane not found: {target}"))?;
        let value = self.run_json(["pane", "get", &pane_id])?;
        let pane = value
            .pointer("/result/pane")
            .context("Herdr response is missing pane")?;
        let workspace = required_string(pane, "workspace_id")?;
        let tab = required_string(pane, "tab_id")?;
        Ok(PaneLocation {
            pane_id: pane_id.clone(),
            session: workspace.clone(),
            window: tab.clone(),
            pane: pane_id.clone(),
            location: format!("{workspace}/{tab}/{pane_id}"),
        })
    }

    fn list_bindings(&self) -> Result<Vec<BoundPane>> {
        let locked = self.lock_registry()?;
        let bindings = locked
            .registry
            .bindings
            .iter()
            .map(|(name, binding)| (name.clone(), binding.clone()))
            .collect::<Vec<_>>();
        drop(locked);
        let mut rows = Vec::new();
        let mut stale = Vec::new();
        for (name, binding) in &bindings {
            match self.binding_row(name, binding) {
                Some(row) => rows.push(row),
                None => stale.push(name.clone()),
            }
        }
        if !stale.is_empty() {
            let mut locked = self.lock_registry()?;
            for name in stale {
                let expected = bindings
                    .iter()
                    .find(|(candidate, _)| candidate == &name)
                    .map(|(_, binding)| &binding.pane_id);
                if locked
                    .registry
                    .bindings
                    .get(&name)
                    .is_some_and(|binding| Some(&binding.pane_id) == expected)
                {
                    locked.registry.bindings.remove(&name);
                }
            }
            self.save_registry(&locked)?;
        }
        Ok(rows)
    }

    fn bind_pane(&self, name: &str, role: &str, pane: &str) -> Result<Vec<BoundPane>> {
        let pane = self.inspect_pane(pane)?;
        let mut locked = self.lock_registry()?;
        locked
            .registry
            .bindings
            .retain(|bound_name, binding| bound_name == name || binding.pane_id != pane.pane_id);
        let previous = locked.registry.bindings.insert(
            name.to_string(),
            BindingRecord {
                role: role.to_string(),
                pane_id: pane.pane_id,
            },
        );
        self.save_registry(&locked)?;
        drop(locked);
        Ok(previous
            .as_ref()
            .and_then(|binding| self.binding_row(name, binding))
            .into_iter()
            .collect())
    }

    fn unbind_pane(&self, name: &str) -> Result<Vec<BoundPane>> {
        let mut locked = self.lock_registry()?;
        let previous = locked.registry.bindings.remove(name);
        self.save_registry(&locked)?;
        drop(locked);
        Ok(previous
            .as_ref()
            .and_then(|binding| self.binding_row(name, binding))
            .into_iter()
            .collect())
    }

    fn live_activity_supported(&self) -> bool {
        false
    }

    fn is_alive(&self, name: &str) -> bool {
        self.record(name)
            .and_then(|(_, record)| self.record_state(&record))
            .is_some_and(|state| !state.dead)
    }
}

fn output_text(output: Output) -> Result<String> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end_matches(['\r', '\n'])
            .to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if stderr.is_empty() { stdout } else { stderr };
    bail!(
        "Herdr command failed (exit {}): {}",
        output.status.code().unwrap_or(1),
        message
    )
}

fn result_array<'a>(value: &'a Value, field: &str) -> Result<&'a Vec<Value>> {
    value
        .get("result")
        .and_then(|value| value.get(field))
        .and_then(Value::as_array)
        .with_context(|| format!("Herdr response is missing {field}"))
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_string)
}

fn required_string(value: &Value, field: &str) -> Result<String> {
    string_field(value, field).with_context(|| format!("Herdr response is missing {field}"))
}

fn write_private(path: &Path, data: &[u8], mode: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(mode)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    file.set_permissions(std::fs::Permissions::from_mode(mode))?;
    file.write_all(data)
        .with_context(|| format!("writing {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("syncing {}", path.display()))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn trailing_lines(value: &str, count: usize) -> String {
    let lines = value.lines().collect::<Vec<_>>();
    lines[lines.len().saturating_sub(count)..].join("\n")
}

fn runner_script(status_file: &Path, name_file: &Path, hook: Option<&HookRecord>) -> String {
    let hook_body = hook
        .map(|hook| {
            format!(
                r#"if mkdir {} 2>/dev/null; then
  cmd=$(cat {} 2>/dev/null) || cmd=
  session_name=$(cat "$name_file" 2>/dev/null) || session_name=
  if [ -n "$cmd" ]; then
    env TPP_SESSION_NAME="$session_name" TPP_SESSION="$session_name" TPP_EXIT_STATUS="$status" sh -c "$cmd"
    hook_status=$?
    if [ "$hook_status" -ne 0 ]; then
      printf '%s hook failed for %s: exit %s\n' "$(date -u +%FT%TZ)" "$session_name" "$hook_status" >> {}
    fi
  fi
fi
"#,
                shell_quote(&hook.marker.to_string_lossy()),
                shell_quote(&hook.command_file.to_string_lossy()),
                shell_quote(&hook.log.to_string_lossy()),
            )
        })
        .unwrap_or_default();
    format!(
        r#"#!/bin/sh
umask 077
status_file={}
name_file={}
status=0
"$@" || status=$?
temp="${{status_file}}.tmp.$$"
printf 'exited\n%s\n' "$status" > "$temp"
mv "$temp" "$status_file"
{}trap 'exit 0' HUP TERM INT
while :; do sleep 86400; done
"#,
        shell_quote(&status_file.to_string_lossy()),
        shell_quote(&name_file.to_string_lossy()),
        hook_body,
    )
}

fn read_exit_status(path: &Path) -> Option<i32> {
    let mut raw = String::new();
    File::open(path).ok()?.read_to_string(&mut raw).ok()?;
    let mut lines = raw.lines();
    (lines.next()? == "exited")
        .then(|| lines.next()?.parse().ok())
        .flatten()
}

fn append_hook_log(path: &Path, message: &str) {
    let Some(parent) = path.parent() else {
        return;
    };
    if create_private_dir_all(parent).is_err() {
        return;
    }
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
    {
        let _ = writeln!(file, "{} {message}", now_epoch());
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{read_exit_status, runner_script, shell_quote, HerdrBackend, HookRecord};
    use crate::backend::{Backend, CaptureSpec, CreateSpec};
    use crate::config::Config;
    use crate::paths::Paths;

    fn fake_backend(root: &std::path::Path) -> (HerdrBackend, PathBuf) {
        let script = root.join("herdr");
        let state = root.join("fake-herdr-state");
        let log = root.join("fake-herdr.log");
        let source = r#"#!/bin/sh
state=__STATE__
log=__LOG__
printf '%s\n' "$*" >> "$log"
if [ "$1" = "--session" ]; then shift 2; fi
entity=$1
action=$2
shift 2
case "$entity:$action" in
  workspace:list)
    if [ -s "$state" ]; then
      printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"w1"}]}}'
    else
      printf '%s\n' '{"result":{"workspaces":[]}}'
    fi
    ;;
  workspace:create)
    printf '%s\n' 'w1:t1 w1:p1' > "$state"
    printf '%s\n' '{"result":{"tab":{"tab_id":"w1:t1","workspace_id":"w1"},"root_pane":{"pane_id":"w1:p1"}}}'
    ;;
  workspace:get)
    [ -s "$state" ] || exit 1
    printf '%s\n' '{"result":{"workspace":{"workspace_id":"w1"}}}'
    ;;
  workspace:focus|tab:focus|tab:rename|pane:run|pane:send-text|pane:send-keys)
    printf '%s\n' '{"result":{"type":"ok"}}'
    ;;
  tab:create)
    count=$(wc -l < "$state" | tr -d ' ')
    next=$((count + 1))
    printf 'w1:t%s w1:p%s\n' "$next" "$next" >> "$state"
    printf '{"result":{"tab":{"tab_id":"w1:t%s","workspace_id":"w1"},"root_pane":{"pane_id":"w1:p%s"}}}\n' "$next" "$next"
    ;;
  tab:list)
    items=
    while read -r tab pane; do
      [ -n "$tab" ] || continue
      item=$(printf '{"tab_id":"%s","workspace_id":"w1","focused":false}' "$tab")
      if [ -n "$items" ]; then items="$items,$item"; else items=$item; fi
    done < "$state"
    printf '{"result":{"tabs":[%s]}}\n' "$items"
    ;;
  tab:close)
    tab=$1
    awk -v tab="$tab" '$1 != tab' "$state" > "$state.next"
    mv "$state.next" "$state"
    printf '%s\n' '{"result":{"type":"ok"}}'
    ;;
  pane:list)
    items=
    while read -r tab pane; do
      [ -n "$pane" ] || continue
      item=$(printf '{"pane_id":"%s","tab_id":"%s","workspace_id":"w1"}' "$pane" "$tab")
      if [ -n "$items" ]; then items="$items,$item"; else items=$item; fi
    done < "$state"
    printf '{"result":{"panes":[%s]}}\n' "$items"
    ;;
  pane:get)
    pane=$1
    tab=$(awk -v pane="$pane" '$2 == pane { print $1 }' "$state")
    [ -n "$tab" ] || exit 1
    printf '{"result":{"pane":{"pane_id":"%s","tab_id":"%s","workspace_id":"w1","scroll":{"max_offset_from_bottom":96,"viewport_rows":24}}}}\n' "$pane" "$tab"
    ;;
  pane:process-info)
    pane=$2
    printf '{"result":{"process_info":{"pane_id":"%s","foreground_process_group_id":42,"shell_pid":42}}}\n' "$pane"
    ;;
  pane:read)
    lines=0
    while [ "$#" -gt 0 ]; do
      if [ "$1" = "--lines" ]; then lines=$2; shift 2; else shift; fi
    done
    if [ "$lines" -gt 80 ]; then
      i=1
      while [ "$i" -le "$lines" ]; do
        printf 'line-%s\n' "$i"
        i=$((i + 1))
      done
    else
      printf '%s\n' 'fake-output'
    fi
    ;;
  *)
    printf 'unsupported fake Herdr command: %s %s\n' "$entity" "$action" >&2
    exit 2
    ;;
esac
"#
        .replace("__STATE__", &shell_quote(&state.to_string_lossy()))
        .replace("__LOG__", &shell_quote(&log.to_string_lossy()));
        std::fs::write(&script, source).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let paths = Paths {
            config_dir: root.join("config"),
            state_dir: root.join("state"),
        };
        let mut backend = HerdrBackend::new(
            Config {
                herdr_mode: true,
                ..Config::default()
            },
            paths,
            root.join("config.toml"),
        );
        backend.client = script.into_os_string();
        (backend, log)
    }

    fn create_spec(name: &str) -> CreateSpec {
        CreateSpec {
            name: name.to_string(),
            dir: Some("/tmp".to_string()),
            command: Vec::new(),
            on_exit: None,
            parent_pane: None,
            watch: false,
        }
    }

    #[test]
    fn shell_quote_preserves_single_quotes() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn tab_labels_strip_the_normalized_storage_prefix() {
        let root = tempdir().unwrap();
        let (mut backend, _) = fake_backend(root.path());
        backend.cfg.session_prefix = "team:".to_string();

        assert_eq!(backend.tab_label("team_api"), "api");
        assert_eq!(backend.tab_label("outside"), "outside");
    }

    #[test]
    fn focus_uses_the_persisted_opaque_workspace_id() {
        let root = tempdir().unwrap();
        let (backend, log) = fake_backend(root.path());
        backend.create(create_spec("tpp/one")).unwrap();
        {
            let mut locked = backend.lock_registry().unwrap();
            locked.registry.workspace_id = Some("opaque-workspace".to_string());
            locked.registry.sessions.get_mut("tpp/one").unwrap().tab_id = "opaque-tab".to_string();
            backend.save_registry(&locked).unwrap();
        }

        backend.focus("tpp/one").unwrap();

        let calls = std::fs::read_to_string(log).unwrap();
        assert!(calls.contains("workspace focus opaque-workspace"));
        assert!(calls.contains("tab focus opaque-tab"));
    }

    #[test]
    fn exited_status_requires_complete_record() {
        let root = tempdir().unwrap();
        let path = root.path().join("status");
        std::fs::write(&path, "running\n").unwrap();
        assert_eq!(read_exit_status(&path), None);
        std::fs::write(&path, "exited\n17\n").unwrap();
        assert_eq!(read_exit_status(&path), Some(17));
    }

    #[test]
    fn runner_reads_mutable_session_name() {
        let hook = HookRecord {
            command_file: "/tmp/hook.cmd".into(),
            marker: "/tmp/hook.once".into(),
            log: "/tmp/hook.log".into(),
        };
        let script = runner_script("/tmp/status".as_ref(), "/tmp/name".as_ref(), Some(&hook));
        assert!(script.contains("session_name=$(cat \"$name_file\""));
        assert!(script.contains("\"$@\" || status=$?"));
    }

    #[test]
    fn sessions_share_one_workspace_and_use_named_tabs() {
        let root = tempdir().unwrap();
        let (backend, log) = fake_backend(root.path());

        assert_eq!(backend.create(create_spec("tpp/one")).unwrap(), "tpp/one");
        assert_eq!(backend.create(create_spec("tpp/two")).unwrap(), "tpp/two");
        assert_eq!(backend.list().unwrap().len(), 2);
        assert_eq!(
            backend
                .capture(
                    "tpp/one",
                    CaptureSpec {
                        lines: None,
                        escape: false,
                        all_history: true,
                    },
                )
                .unwrap()
                .lines()
                .count(),
            120
        );
        assert_eq!(
            backend
                .capture(
                    "tpp/one",
                    CaptureSpec {
                        lines: Some(20),
                        escape: false,
                        all_history: false,
                    },
                )
                .unwrap(),
            "fake-output"
        );
        backend.send_text("tpp/two", "hello", true).unwrap();
        backend.submit_text("tpp/two", "delayed", true, 50).unwrap();
        backend
            .submit_text("tpp/two", "literal-delayed", false, 1)
            .unwrap();
        backend.rename("tpp/two", "tpp/renamed").unwrap();
        backend.bind_pane("agent", "worker", "tpp/renamed").unwrap();
        assert_eq!(backend.list_bindings().unwrap()[0].pane_id, "w1:p2");

        backend.remove("tpp/one").unwrap();
        backend.remove("tpp/renamed").unwrap();
        assert!(backend.list().unwrap().is_empty());

        let calls = std::fs::read_to_string(log).unwrap();
        assert_eq!(calls.matches("workspace create").count(), 1);
        assert_eq!(calls.matches("tab create").count(), 1);
        assert!(calls.contains("tab rename w1:t1 one"));
        assert!(calls.contains("--label two"));
        assert!(calls.contains("pane run w1:p2 delayed"));
        assert!(calls.contains("pane send-text w1:p2 literal-delayed"));
        assert!(calls.contains("pane send-keys w1:p2 Enter"));
        assert!(calls.contains("tab rename w1:t2 renamed"));
    }
}
