//! Recorded exited sessions. When a session is exited/removed with recording on, its final
//! scrollback is written to `~/.local/state/tpp/exited/<name>.{json,log}` so `cat` can replay
//! a dead session and `clear` can purge them. The log is written before the metadata, so a
//! crash leaves at worst an orphan log (harmless) rather than metadata pointing at nothing.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths::{create_private_dir_all, Paths};
use crate::session::now_epoch;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitedRecord {
    pub name: String,
    pub scope: String,
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
}

impl Store {
    pub fn new(paths: &Paths) -> Self {
        Self {
            dir: paths.exited_dir(),
        }
    }

    fn paths_for(&self, name: &str) -> (PathBuf, PathBuf) {
        let base = self.dir.join(encode(name));
        (base.with_extension("json"), base.with_extension("log"))
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
        let (_, log) = self.paths_for(name);
        match std::fs::read_to_string(&log) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", log.display())),
        }
    }

    /// All recorded exited sessions, newest first.
    pub fn list(&self) -> Result<Vec<ExitedRecord>> {
        let mut out = Vec::new();
        let rd = match std::fs::read_dir(&self.dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.dir.display())),
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
        out.sort_by(|a, b| b.exited_at.cmp(&a.exited_at));
        Ok(out)
    }

    /// Look up one recorded session by exact name.
    pub fn get(&self, name: &str) -> Result<Option<ExitedRecord>> {
        let (json, _) = self.paths_for(name);
        match std::fs::read_to_string(&json) {
            Ok(text) => Ok(serde_json::from_str(&text).ok()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", json.display())),
        }
    }

    /// Delete one record (json + log). Returns whether anything was removed.
    pub fn remove(&self, name: &str) -> bool {
        let (json, log) = self.paths_for(name);
        let a = std::fs::remove_file(&json).is_ok();
        let b = std::fs::remove_file(&log).is_ok();
        a || b
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
        // Sweep any orphan logs whose json vanished.
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for entry in rd.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
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
}

#[allow(dead_code)]
pub fn decode_filename(file: &str) -> String {
    decode(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_roundtrips_slashes() {
        let name = "claude/feat-build-payments";
        assert_eq!(decode(&encode(name)), name);
    }

    #[test]
    fn encode_has_no_path_separators() {
        assert!(!encode("a/b/c").contains('/'));
    }
}
