//! `config.toml` model and loading. Every field has a default, so a missing or partial
//! file is fine — `Config::load` returns defaults when the file is absent.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

pub const DEFAULT_SESSION_PREFIX: &str = "tpp/";
pub const DEFAULT_REAP_TTL_SECS: u64 = 6 * 60 * 60;

fn default_session_prefix() -> String {
    DEFAULT_SESSION_PREFIX.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// tmux socket name (`tmux -L <name>`). Empty/unset = the default tmux server, so tpp
    /// sessions live alongside your normal ones. Set a name to isolate them.
    pub socket: Option<String>,
    /// Command run by `tpp new`/`tpp run` when none is given. Defaults to `$SHELL`.
    pub shell: Option<String>,
    /// Prefix applied to all tpp-created tmux session names. Empty = no prefix.
    #[serde(default = "default_session_prefix")]
    pub session_prefix: String,
    pub ls: LsCfg,
    pub send: SendCfg,
    pub new: NewCfg,
    pub capture: CaptureCfg,
    pub tail: TailCfg,
    pub exit: ExitCfg,
    pub wait: WaitCfg,
    pub reap: ReapCfg,
    pub watch: WatchCfg,
    /// Legacy no-op accepted so older config files keep loading after scopes were removed.
    #[serde(rename = "scope", default, skip_serializing)]
    pub legacy_scope: Option<toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LsCfg {
    /// Also show sessions that exited within this many hours (0 = never).
    pub show_exited_hours: u64,
    /// Legacy no-op accepted so older config files keep loading after scopes were removed.
    #[serde(rename = "default", default, skip_serializing)]
    pub legacy_default: Option<toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SendCfg {
    /// Use bracketed paste for multi-line text so it lands verbatim in TUIs.
    pub bracketed_paste: bool,
    /// Pause after a paste before pressing Enter, letting a TUI settle (ms).
    pub enter_delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NewCfg {
    /// Keep a finished command's output on screen (so `cat`/`tail` still work) instead of
    /// letting tmux close the session the instant the command exits.
    pub remain_on_exit: bool,
    /// Per-session scrollback limit applied to panes created after the session starts.
    pub history_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureCfg {
    /// Default number of trailing lines for `cat` (0 = visible screen only).
    pub lines: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TailCfg {
    /// Poll interval for `tail` (ms).
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExitCfg {
    /// Scrollback lines recorded when a session is exited/removed with `--record`.
    pub record_lines: u32,
    /// Auto-prune recorded exited sessions older than this many hours (0 = keep forever).
    pub prune_hours: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WaitCfg {
    /// How long output must be unchanged to count as "idle" (ms).
    pub stable_for_ms: u64,
    /// Default upper bound for `wait` (ms).
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationCfg {
    secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReapCfg {
    /// Idle threshold for detached live sessions. "0" disables live idle reaping.
    pub ttl: DurationCfg,
    /// Record output before killing a reaped session.
    pub record: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WatchCfg {
    pub enabled: bool,
    pub poll: DurationCfg,
    pub prompt_stable: DurationCfg,
    pub stuck_after: DurationCfg,
    /// Enables automated sends for both `enter` and `keys` rules.
    pub auto_enter: bool,
    /// Maximum automated sends per unchanged-screen episode.
    pub max_enters: u32,
    pub builtin_rules: bool,
    pub nudge_parent: bool,
    pub notify: String,
    pub cooldown: DurationCfg,
    pub rules: Vec<WatchRuleCfg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchRuleCfg {
    pub pattern: String,
    pub action: WatchAction,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WatchAction {
    Enter,
    Keys,
    Notify,
    Ignore,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            socket: None,
            shell: None,
            session_prefix: default_session_prefix(),
            ls: LsCfg::default(),
            send: SendCfg::default(),
            new: NewCfg::default(),
            capture: CaptureCfg::default(),
            tail: TailCfg::default(),
            exit: ExitCfg::default(),
            wait: WaitCfg::default(),
            reap: ReapCfg::default(),
            watch: WatchCfg::default(),
            legacy_scope: None,
        }
    }
}

impl Default for LsCfg {
    fn default() -> Self {
        Self {
            show_exited_hours: 24,
            legacy_default: None,
        }
    }
}
impl Default for SendCfg {
    fn default() -> Self {
        Self {
            bracketed_paste: true,
            enter_delay_ms: 0,
        }
    }
}
impl Default for NewCfg {
    fn default() -> Self {
        Self {
            remain_on_exit: true,
            history_limit: 100_000,
        }
    }
}
impl Default for CaptureCfg {
    fn default() -> Self {
        Self { lines: 200 }
    }
}
impl Default for TailCfg {
    fn default() -> Self {
        Self { interval_ms: 1000 }
    }
}
impl Default for ExitCfg {
    fn default() -> Self {
        Self {
            record_lines: 1000,
            prune_hours: 24,
        }
    }
}
impl Default for WaitCfg {
    fn default() -> Self {
        Self {
            stable_for_ms: 750,
            timeout_ms: 30_000,
        }
    }
}
impl Default for ReapCfg {
    fn default() -> Self {
        Self {
            ttl: DurationCfg::from_secs(DEFAULT_REAP_TTL_SECS),
            record: true,
        }
    }
}

impl Default for WatchCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            poll: DurationCfg::from_secs(3),
            prompt_stable: DurationCfg::from_secs(5),
            stuck_after: DurationCfg::from_secs(30),
            auto_enter: true,
            max_enters: 2,
            builtin_rules: true,
            nudge_parent: true,
            notify: String::new(),
            cooldown: DurationCfg::from_secs(10 * 60),
            rules: Vec::new(),
        }
    }
}

impl Default for DurationCfg {
    fn default() -> Self {
        Self::from_secs(0)
    }
}

impl DurationCfg {
    pub const fn from_secs(secs: u64) -> Self {
        Self { secs }
    }

    pub fn as_secs(self) -> u64 {
        self.secs
    }

    pub fn display(self) -> String {
        format_duration(self.secs)
    }
}

impl Serialize for DurationCfg {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.display())
    }
}

impl<'de> Deserialize<'de> for DurationCfg {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DurationVisitor;

        impl de::Visitor<'_> for DurationVisitor {
            type Value = DurationCfg;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a duration string like 1h, 90m, 1d, or 0")
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                parse_duration(value).map_err(E::custom)
            }

            fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_str(&value)
            }
        }

        deserializer.deserialize_str(DurationVisitor)
    }
}

/// Parse tpp duration config and CLI values.
pub fn parse_duration(raw: &str) -> std::result::Result<DurationCfg, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "0" {
        return Ok(DurationCfg::from_secs(0));
    }

    let digit_len = raw
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_digit())
        .last()
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);
    let (number, unit) = raw.split_at(digit_len);
    let value: u64 = number.parse().map_err(|_| invalid_duration_message(raw))?;
    if value == 0 {
        return Err(invalid_duration_message(raw));
    }

    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => return Err(invalid_duration_message(raw)),
    };
    value
        .checked_mul(multiplier)
        .map(DurationCfg::from_secs)
        .ok_or_else(|| invalid_duration_message(raw))
}

fn invalid_duration_message(raw: &str) -> String {
    format!("invalid duration {raw:?} (examples: 1h, 90m, 1d, 0)")
}

fn format_duration(secs: u64) -> String {
    match secs {
        0 => "0".to_string(),
        s if s % 86_400 == 0 => format!("{}d", s / 86_400),
        s if s % 3_600 == 0 => format!("{}h", s / 3_600),
        s if s % 60 == 0 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}

impl Config {
    /// Load config from `path`, returning defaults if the file does not exist.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading config {}", path.display())),
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("parsing config.toml")
    }
}

pub const STARTER_CONFIG: &str = r#"# tpp configuration  (~/.config/tpp/config.toml)
# Every setting below is shown at its default — delete what you don't need to change.

# tmux socket. Empty = the default tmux server, so tpp sessions show up in your normal
# tmux (and in `tmx`). Set a name (e.g. "tpp") to give tpp its own isolated server.
socket = ""

# Command for `tpp new`/`tpp run` when you don't pass one. Empty = $SHELL.
shell = ""

# Prefix applied to all tpp-created tmux session names. Empty = no prefix.
session_prefix = "tpp/"

[ls]
show_exited_hours = 24   # also surface sessions that exited in the last N hours

[send]
bracketed_paste = true   # multi-line text pastes verbatim (good for agent TUIs)
enter_delay_ms = 0       # pause after a paste before pressing Enter

[new]
remain_on_exit = true    # keep a finished command's output on screen for cat/tail
history_limit = 100000

[capture]
lines = 200              # default trailing lines for `cat` (0 = visible screen only)

[tail]
interval_ms = 1000       # poll cadence for `tail`

[exit]
record_lines = 1000      # scrollback recorded when a session exits
prune_hours = 24         # forget recorded exited sessions after N hours

[wait]
stable_for_ms = 750      # output must be unchanged this long to count as "idle"
timeout_ms = 30000       # default upper bound for `wait`

[reap]
ttl = "6h"               # reap detached live sessions idle longer than this; "0" disables that
record = true            # save scrollback before killing a reaped session

[watch]
enabled = true
poll = "3s"
prompt_stable = "5s"
stuck_after = "30s"
auto_enter = true
max_enters = 2
builtin_rules = true
nudge_parent = true
notify = ""
# notify = "mac-notify send --blocker \"tpp {session}: {reason}\""
cooldown = "10m"

# User rules run before built-ins; plain patterns are substrings and /.../ patterns are regexes.
# Set builtin_rules = false above to use only the rules you define here.
# [[watch.rules]]
# pattern = "Retry with a faster model"
# action = "keys"
# keys = ["Down", "Enter"]

# [[watch.rules]]
# pattern = "? for shortcuts"
# action = "ignore"

# [[watch.rules]]
# pattern = "Press enter to continue"
# action = "enter"

# [[watch.rules]]
# pattern = "Enter to confirm"
# action = "enter"

# [[watch.rules]]
# pattern = "Do you trust"
# action = "enter"

# [[watch.rules]]
# pattern = "trust this folder"
# action = "enter"

# [[watch.rules]]
# pattern = "Sign in to continue"
# action = "notify"
"#;

#[cfg(test)]
mod tests {
    use super::{parse_duration, Config, DurationCfg, WatchAction, STARTER_CONFIG};

    #[test]
    fn default_session_prefix_is_tpp_path() {
        assert_eq!(Config::default().session_prefix, "tpp/");
    }

    #[test]
    fn parse_older_config_uses_default_session_prefix() {
        let cfg = Config::parse("socket = \"\"\n").unwrap();

        assert_eq!(cfg.session_prefix, "tpp/");
    }

    #[test]
    fn parse_legacy_scope_config_without_using_it() {
        let cfg = Config::parse("[scope]\nmode = \"git\"\n[ls]\ndefault = \"scope\"\n").unwrap();

        assert_eq!(cfg.session_prefix, "tpp/");
    }

    #[test]
    fn parse_allows_empty_session_prefix() {
        let cfg = Config::parse("session_prefix = \"\"\n").unwrap();

        assert_eq!(cfg.session_prefix, "");
    }

    #[test]
    fn starter_config_documents_session_prefix() {
        assert!(STARTER_CONFIG.contains("session_prefix = \"tpp/\""));
    }

    #[test]
    fn starter_config_does_not_document_scope() {
        assert!(!STARTER_CONFIG.contains("[scope]"));
        assert!(!STARTER_CONFIG.contains("mode = \"git\""));
    }

    #[test]
    fn exit_record_lines_default_to_1000() {
        assert_eq!(Config::default().exit.record_lines, 1000);
    }

    #[test]
    fn starter_config_documents_exit_record_lines() {
        assert!(STARTER_CONFIG.contains("record_lines = 1000"));
    }

    #[test]
    fn reap_ttl_defaults_to_six_hours() {
        assert_eq!(Config::default().reap.ttl.as_secs(), 6 * 60 * 60);
        assert!(Config::default().reap.record);
    }

    #[test]
    fn parse_duration_accepts_tpp_units() {
        assert_eq!(parse_duration("30s").unwrap().as_secs(), 30);
        assert_eq!(parse_duration("90m").unwrap().as_secs(), 5_400);
        assert_eq!(parse_duration("6h").unwrap().as_secs(), 21_600);
        assert_eq!(parse_duration("1d").unwrap().as_secs(), 86_400);
        assert_eq!(parse_duration("0").unwrap().as_secs(), 0);
    }

    #[test]
    fn parse_duration_rejects_invalid_values() {
        assert!(parse_duration("0h").is_err());
        assert!(parse_duration("1w").is_err());
        assert!(parse_duration("soon").is_err());
    }

    #[test]
    fn parse_config_reap_section() {
        let cfg = Config::parse("[reap]\nttl = \"90m\"\nrecord = false\n").unwrap();

        assert_eq!(cfg.reap.ttl, DurationCfg::from_secs(5_400));
        assert!(!cfg.reap.record);
    }

    #[test]
    fn starter_config_documents_reap() {
        assert!(STARTER_CONFIG.contains("[reap]"));
        assert!(STARTER_CONFIG.contains("ttl = \"6h\""));
        assert!(STARTER_CONFIG.contains("record = true"));
    }

    #[test]
    fn watch_defaults_match_the_session_contract() {
        let watch = Config::default().watch;

        assert!(watch.enabled);
        assert_eq!(watch.poll.as_secs(), 3);
        assert_eq!(watch.prompt_stable.as_secs(), 5);
        assert_eq!(watch.stuck_after.as_secs(), 30);
        assert!(watch.auto_enter);
        assert_eq!(watch.max_enters, 2);
        assert!(watch.builtin_rules);
        assert!(watch.nudge_parent);
        assert!(watch.notify.is_empty());
        assert_eq!(watch.cooldown.as_secs(), 10 * 60);
        assert!(watch.rules.is_empty());
    }

    #[test]
    fn parse_config_watch_section_and_rules() {
        let cfg = Config::parse(
            r#"
[watch]
enabled = false
poll = "1s"
prompt_stable = "2s"
stuck_after = "45s"
auto_enter = false
max_enters = 4
nudge_parent = false
notify = "notify {session} {reason}"
cooldown = "1m"

[[watch.rules]]
pattern = "/OAuth.*/"
action = "notify"

[[watch.rules]]
pattern = "safe idle"
action = "ignore"
"#,
        )
        .unwrap();

        assert!(!cfg.watch.enabled);
        assert_eq!(cfg.watch.poll.as_secs(), 1);
        assert_eq!(cfg.watch.prompt_stable.as_secs(), 2);
        assert_eq!(cfg.watch.stuck_after.as_secs(), 45);
        assert!(!cfg.watch.auto_enter);
        assert_eq!(cfg.watch.max_enters, 4);
        assert!(!cfg.watch.nudge_parent);
        assert_eq!(cfg.watch.notify, "notify {session} {reason}");
        assert_eq!(cfg.watch.cooldown.as_secs(), 60);
        assert_eq!(cfg.watch.rules.len(), 2);
        assert_eq!(cfg.watch.rules[0].action, WatchAction::Notify);
        assert_eq!(cfg.watch.rules[1].action, WatchAction::Ignore);
        assert!(cfg.watch.rules.iter().all(|rule| rule.keys.is_empty()));
    }

    #[test]
    fn parse_keys_rule_and_serialize_keyless_rules_without_empty_keys() {
        let cfg = Config::parse(
            r#"
[watch]
builtin_rules = false

[[watch.rules]]
pattern = "Retry with a faster model"
action = "keys"
keys = ["Down", "Enter"]

[[watch.rules]]
pattern = "Sign in to continue"
action = "notify"
"#,
        )
        .unwrap();

        assert!(!cfg.watch.builtin_rules);
        assert_eq!(cfg.watch.rules[0].action, WatchAction::Keys);
        assert_eq!(cfg.watch.rules[0].keys, ["Down", "Enter"]);
        assert!(cfg.watch.rules[1].keys.is_empty());

        let serialized = toml::to_string(&cfg).unwrap();
        assert_eq!(serialized.matches("keys =").count(), 1);
    }

    #[test]
    fn partial_watch_config_enables_builtin_rules_by_default() {
        let cfg = Config::parse("[watch]\nenabled = false\n").unwrap();

        assert!(cfg.watch.builtin_rules);
    }

    #[test]
    fn starter_config_documents_watch_defaults() {
        for expected in [
            "[watch]",
            "enabled = true",
            "poll = \"3s\"",
            "prompt_stable = \"5s\"",
            "stuck_after = \"30s\"",
            "auto_enter = true",
            "max_enters = 2",
            "builtin_rules = true",
            "nudge_parent = true",
            "notify = \"\"",
            "cooldown = \"10m\"",
            "[[watch.rules]]",
            "mac-notify send --blocker",
        ] {
            assert!(STARTER_CONFIG.contains(expected), "missing {expected}");
        }
    }
}
