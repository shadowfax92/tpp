//! Recorded exited sessions. When a session is exited/removed with recording on, its final
//! scrollback is written under `~/.local/state/tpp/exited/<socket>/` so `cat` can replay a dead
//! session without leaking transcripts across tmux sockets. The log is written before the
//! metadata, so a crash leaves at worst an orphan log (harmless) rather than metadata pointing
//! at nothing.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths::{create_private_dir_all, Paths};
use crate::session::now_epoch;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitedRecord {
    pub name: String,
    pub dir: String,
    pub command: String,
    pub exited_at: i64,
}

/// Encode a session name into a single safe filename component. Percent-encodes anything
/// outside `[A-Za-z0-9._-]` (notably `/`), so `claude/feat-x` round-trips to one file.
fn encode(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    for b in name.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => s.push(b as char),
            _ => s.push_str(&format!("%{b:02X}")),
        }
    }
    s
}

fn decode(file: &str) -> String {
    let bytes = file.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&file[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct Store {
    dir: PathBuf,
    legacy_dir: Option<PathBuf>,
}

impl Store {
    pub fn new(paths: &Paths, socket: Option<&str>) -> Self {
        let root = paths.exited_dir();
        Self {
            dir: root.join(socket_dir(socket)),
            legacy_dir: socket.is_none().then_some(root),
        }
    }

    fn paths_for_dir(dir: &std::path::Path, name: &str) -> (PathBuf, PathBuf) {
        let base = dir.join(encode(name));
        (base.with_extension("json"), base.with_extension("log"))
    }

    fn paths_for(&self, name: &str) -> (PathBuf, PathBuf) {
        Self::paths_for_dir(&self.dir, name)
    }

    pub fn record(&self, rec: &ExitedRecord, output: &str) -> Result<()> {
        create_private_dir_all(&self.dir)?;
        let (json, log) = self.paths_for(&rec.name);
        std::fs::write(&log, output).with_context(|| format!("writing {}", log.display()))?;
        let data = serde_json::to_vec_pretty(rec)?;
        std::fs::write(&json, data).with_context(|| format!("writing {}", json.display()))?;
        Ok(())
    }

    pub fn read_log(&self, name: &str) -> Result<Option<String>> {
        for dir in self.lookup_dirs() {
            let (_, log) = Self::paths_for_dir(dir, name);
            match std::fs::read_to_string(&log) {
                Ok(s) => return Ok(Some(s)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("reading {}", log.display())),
            }
        }
        Ok(None)
    }

    /// All recorded exited sessions, newest first.
    pub fn list(&self) -> Result<Vec<ExitedRecord>> {
        let mut out = self.list_dir(&self.dir)?;
        if let Some(dir) = &self.legacy_dir {
            let seen: std::collections::HashSet<String> =
                out.iter().map(|rec| rec.name.clone()).collect();
            out.extend(
                self.list_dir(dir)?
                    .into_iter()
                    .filter(|rec| !seen.contains(&rec.name)),
            );
        }
        out.sort_by(|a, b| b.exited_at.cmp(&a.exited_at));
        Ok(out)
    }

    fn list_dir(&self, dir: &std::path::Path) -> Result<Vec<ExitedRecord>> {
        let mut out = Vec::new();
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(rec) = serde_json::from_str::<ExitedRecord>(&text) {
                    out.push(rec);
                }
            }
        }
        Ok(out)
    }

    /// Look up one recorded session by exact name.
    pub fn get(&self, name: &str) -> Result<Option<ExitedRecord>> {
        for dir in self.lookup_dirs() {
            let (json, _) = Self::paths_for_dir(dir, name);
            match std::fs::read_to_string(&json) {
                Ok(text) => return Ok(serde_json::from_str(&text).ok()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("reading {}", json.display())),
            }
        }
        Ok(None)
    }

    /// Delete one record (json + log). Returns whether anything was removed.
    pub fn remove(&self, name: &str) -> bool {
        let mut removed = false;
        for dir in self.lookup_dirs() {
            let (json, log) = Self::paths_for_dir(dir, name);
            removed |= std::fs::remove_file(&json).is_ok();
            removed |= std::fs::remove_file(&log).is_ok();
        }
        removed
    }

    /// Delete every record; returns the count removed.
    pub fn clear(&self) -> Result<usize> {
        let recs = self.list()?;
        let mut n = 0;
        for rec in &recs {
            if self.remove(&rec.name) {
                n += 1;
            }
        }
        // Sweep any orphan files whose json vanished.
        sweep_files(&self.dir);
        if let Some(dir) = &self.legacy_dir {
            sweep_files(dir);
        }
        Ok(n)
    }

    /// Drop records older than `hours` (0 = keep forever). Returns count pruned.
    pub fn prune(&self, hours: u64) -> Result<usize> {
        if hours == 0 {
            return Ok(0);
        }
        let cutoff = now_epoch() - (hours as i64) * 3600;
        let mut n = 0;
        for rec in self.list()? {
            if rec.exited_at < cutoff && self.remove(&rec.name) {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Filter to records that exited within the last `hours`.
    pub fn recent(&self, hours: u64) -> Result<Vec<ExitedRecord>> {
        if hours == 0 {
            return Ok(Vec::new());
        }
        let cutoff = now_epoch() - (hours as i64) * 3600;
        Ok(self
            .list()?
            .into_iter()
            .filter(|r| r.exited_at >= cutoff)
            .collect())
    }

    fn lookup_dirs(&self) -> Vec<&std::path::Path> {
        let mut dirs = vec![self.dir.as_path()];
        if let Some(dir) = &self.legacy_dir {
            dirs.push(dir.as_path());
        }
        dirs
    }
}

fn socket_dir(socket: Option<&str>) -> String {
    match socket {
        Some(socket) => format!("socket-{}", encode(socket)),
        None => "default".to_string(),
    }
}

fn sweep_files(dir: &std::path::Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_file() {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

#[allow(dead_code)]
pub fn decode_filename(file: &str) -> String {
    decode(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn paths(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            state_dir: root.join("state"),
        }
    }

    fn record(name: &str, command: &str, exited_at: i64) -> ExitedRecord {
        ExitedRecord {
            name: name.to_string(),
            dir: String::new(),
            command: command.to_string(),
            exited_at,
        }
    }

    #[test]
    fn encode_roundtrips_slashes() {
        let name = "claude/feat-build-payments";
        assert_eq!(decode(&encode(name)), name);
    }

    #[test]
    fn encode_has_no_path_separators() {
        assert!(!encode("a/b/c").contains('/'));
    }

    #[test]
    fn records_are_namespaced_by_socket() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let paths = paths(tmp.path());
        let default = Store::new(&paths, None);
        let agents = Store::new(&paths, Some("agents"));

        default.record(&record("same", "default", 1), "default log")?;
        agents.record(&record("same", "agents", 2), "agents log")?;

        assert_eq!(default.read_log("same")?.as_deref(), Some("default log"));
        assert_eq!(agents.read_log("same")?.as_deref(), Some("agents log"));
        assert_eq!(default.list()?[0].command, "default");
        assert_eq!(agents.list()?[0].command, "agents");
        Ok(())
    }

    #[test]
    fn legacy_root_records_are_default_socket_only() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let paths = paths(tmp.path());
        let root = paths.exited_dir();
        create_private_dir_all(&root)?;
        let (json, log) = Store::paths_for_dir(&root, "legacy");
        std::fs::write(&log, "legacy log")?;
        std::fs::write(
            &json,
            serde_json::to_vec_pretty(&record("legacy", "old", 1))?,
        )?;

        let default = Store::new(&paths, None);
        let agents = Store::new(&paths, Some("agents"));

        assert_eq!(default.read_log("legacy")?.as_deref(), Some("legacy log"));
        assert_eq!(agents.read_log("legacy")?, None);
        assert_eq!(default.list()?.len(), 1);
        assert!(agents.list()?.is_empty());
        Ok(())
    }

    #[test]
    fn implicit_tmux_socket_path_does_not_read_legacy_default_records() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let paths = paths(tmp.path());
        let root = paths.exited_dir();
        create_private_dir_all(&root)?;
        let (json, log) = Store::paths_for_dir(&root, "legacy");
        std::fs::write(&log, "legacy log")?;
        std::fs::write(
            &json,
            serde_json::to_vec_pretty(&record("legacy", "old", 1))?,
        )?;

        let inherited = Store::new(&paths, Some("path:/tmp/tmux-501/custom"));

        assert_eq!(inherited.read_log("legacy")?, None);
        assert!(inherited.list()?.is_empty());
        Ok(())
    }
}
