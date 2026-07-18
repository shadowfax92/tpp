//! Per-session stable-screen detection and watchdog process lifecycle.

use std::time::Duration;

use anyhow::{Context, Result};
use regex::Regex;

use crate::config::{WatchAction, WatchCfg, WatchRuleCfg};

const BUILTIN_RULES: &[(&str, WatchAction)] = &[
    ("Press enter to continue", WatchAction::Enter),
    ("Enter to confirm", WatchAction::Enter),
    ("Do you trust", WatchAction::Enter),
    ("trust this folder", WatchAction::Enter),
    ("? for shortcuts", WatchAction::Ignore),
];

#[derive(Debug)]
enum RulePattern {
    Substring(String),
    Regex(Regex),
}

#[derive(Debug)]
struct CompiledRule {
    source: String,
    pattern: RulePattern,
    action: WatchAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuleMatch {
    source: String,
    action: WatchAction,
}

#[derive(Debug)]
struct RuleSet {
    rules: Vec<CompiledRule>,
}

impl RuleSet {
    fn compile(configured: &[WatchRuleCfg]) -> Result<Self> {
        let mut rules = Vec::with_capacity(configured.len() + BUILTIN_RULES.len());
        for rule in configured {
            rules.push(CompiledRule::compile(&rule.pattern, rule.action)?);
        }
        for &(pattern, action) in BUILTIN_RULES {
            rules.push(CompiledRule::compile(pattern, action)?);
        }
        Ok(Self { rules })
    }

    fn matched(&self, screen: &str) -> Option<RuleMatch> {
        self.rules.iter().find_map(|rule| {
            let matches = match &rule.pattern {
                RulePattern::Substring(pattern) => screen.contains(pattern),
                RulePattern::Regex(pattern) => pattern.is_match(screen),
            };
            matches.then(|| RuleMatch {
                source: rule.source.clone(),
                action: rule.action,
            })
        })
    }
}

impl CompiledRule {
    fn compile(pattern: &str, action: WatchAction) -> Result<Self> {
        let compiled = if pattern.len() >= 2 && pattern.starts_with('/') && pattern.ends_with('/') {
            RulePattern::Regex(
                Regex::new(&pattern[1..pattern.len() - 1])
                    .with_context(|| format!("invalid watch rule regex {pattern:?}"))?,
            )
        } else {
            RulePattern::Substring(pattern.to_string())
        };
        Ok(Self {
            source: pattern.to_string(),
            pattern: compiled,
            action,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchDecision {
    Enter { pattern: String },
    Escalate { reason: String },
}

#[derive(Debug, Default)]
struct WatchState {
    screen_hash: Option<u64>,
    stable_since: Duration,
    enters: u32,
    awaiting_change: bool,
    escalated: bool,
    last_escalation: Option<Duration>,
}

pub struct WatchEngine {
    cfg: WatchCfg,
    rules: RuleSet,
    state: WatchState,
}

impl WatchEngine {
    /// Compile configured rules and initialize one session's detection state.
    pub fn new(cfg: &WatchCfg) -> Result<Self> {
        Ok(Self {
            cfg: cfg.clone(),
            rules: RuleSet::compile(&cfg.rules)?,
            state: WatchState::default(),
        })
    }

    /// Evaluate an ANSI-stripped screen tail captured at the supplied elapsed time.
    pub fn observe(
        &mut self,
        now: Duration,
        screen_hash: u64,
        screen: &str,
    ) -> Option<WatchDecision> {
        let matched = self.rules.matched(screen);
        self.state
            .observe(now, screen_hash, matched.as_ref(), &self.cfg)
    }
}

impl WatchState {
    /// Turn one captured-screen observation into at most one watchdog action.
    fn observe(
        &mut self,
        now: Duration,
        screen_hash: u64,
        matched: Option<&RuleMatch>,
        cfg: &WatchCfg,
    ) -> Option<WatchDecision> {
        if self.screen_hash != Some(screen_hash) {
            self.screen_hash = Some(screen_hash);
            self.stable_since = now;
            self.enters = 0;
            self.awaiting_change = false;
            self.escalated = false;
            return None;
        }

        if matched.is_some_and(|rule| rule.action == WatchAction::Ignore) {
            return None;
        }

        if self.awaiting_change {
            return self.escalate(
                now,
                cfg,
                "screen unchanged after automatic Enter".to_string(),
            );
        }

        let stable_for = now.saturating_sub(self.stable_since);
        match matched {
            Some(rule) if stable_for >= Duration::from_secs(cfg.prompt_stable.as_secs()) => {
                match rule.action {
                    WatchAction::Enter if cfg.auto_enter && self.enters < cfg.max_enters => {
                        self.enters += 1;
                        self.awaiting_change = true;
                        Some(WatchDecision::Enter {
                            pattern: rule.source.clone(),
                        })
                    }
                    WatchAction::Enter if !cfg.auto_enter => self.escalate(
                        now,
                        cfg,
                        format!("matched {:?}, but auto-Enter is disabled", rule.source),
                    ),
                    WatchAction::Enter => self.escalate(
                        now,
                        cfg,
                        format!("automatic Enter limit reached for {:?}", rule.source),
                    ),
                    WatchAction::Notify => {
                        self.escalate(now, cfg, format!("matched watch rule {:?}", rule.source))
                    }
                    WatchAction::Ignore => None,
                }
            }
            None if stable_for >= Duration::from_secs(cfg.stuck_after.as_secs()) => self.escalate(
                now,
                cfg,
                format!("screen unchanged for {}", cfg.stuck_after.display()),
            ),
            _ => None,
        }
    }

    fn escalate(&mut self, now: Duration, cfg: &WatchCfg, reason: String) -> Option<WatchDecision> {
        if self.escalated {
            return None;
        }
        if self.last_escalation.is_some_and(|last| {
            now.saturating_sub(last) < Duration::from_secs(cfg.cooldown.as_secs())
        }) {
            return None;
        }
        self.escalated = true;
        self.last_escalation = Some(now);
        Some(WatchDecision::Escalate { reason })
    }
}

/// Validate user rule syntax before a session is labeled as watched.
pub fn validate_config(cfg: &WatchCfg) -> Result<()> {
    WatchEngine::new(cfg).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::{RuleSet, WatchDecision, WatchState};
    use crate::config::{WatchAction, WatchCfg, WatchRuleCfg};
    use std::time::Duration;

    fn seconds(value: u64) -> Duration {
        Duration::from_secs(value)
    }

    #[test]
    fn user_rules_precede_builtins_and_first_match_wins() {
        let rules = RuleSet::compile(&[
            WatchRuleCfg {
                pattern: "Press enter".to_string(),
                action: WatchAction::Ignore,
            },
            WatchRuleCfg {
                pattern: "continue".to_string(),
                action: WatchAction::Notify,
            },
        ])
        .unwrap();

        let matched = rules.matched("Press enter to continue").unwrap();
        assert_eq!(matched.source, "Press enter");
        assert_eq!(matched.action, WatchAction::Ignore);
    }

    #[test]
    fn slash_delimited_rules_are_regexes() {
        let rules = RuleSet::compile(&[WatchRuleCfg {
            pattern: "/OAuth [0-9]+/".to_string(),
            action: WatchAction::Notify,
        }])
        .unwrap();

        assert_eq!(
            rules.matched("OAuth 123").unwrap().action,
            WatchAction::Notify
        );
        assert!(RuleSet::compile(&[WatchRuleCfg {
            pattern: "/[unterminated/".to_string(),
            action: WatchAction::Notify,
        }])
        .is_err());
    }

    #[test]
    fn builtins_cover_frozen_prompts_and_claude_idle() {
        let rules = RuleSet::compile(&[]).unwrap();
        for screen in [
            "Press enter to continue",
            "Enter to confirm · Esc to cancel",
            "Do you trust the contents of this directory?",
            "Yes, I trust this folder",
        ] {
            assert_eq!(rules.matched(screen).unwrap().action, WatchAction::Enter);
        }
        assert_eq!(
            rules.matched("composer  ? for shortcuts").unwrap().action,
            WatchAction::Ignore
        );
    }

    #[test]
    fn matched_prompts_use_the_fast_threshold() {
        let cfg = WatchCfg::default();
        let rules = RuleSet::compile(&[]).unwrap();
        let matched = rules.matched("Press enter to continue").unwrap();
        let mut state = WatchState::default();

        assert_eq!(state.observe(seconds(0), 1, Some(&matched), &cfg), None);
        assert_eq!(state.observe(seconds(4), 1, Some(&matched), &cfg), None);
        assert_eq!(
            state.observe(seconds(5), 1, Some(&matched), &cfg),
            Some(WatchDecision::Enter {
                pattern: "Press enter to continue".to_string()
            })
        );
    }

    #[test]
    fn unknown_screens_use_the_stuck_threshold() {
        let cfg = WatchCfg::default();
        let mut state = WatchState::default();

        assert_eq!(state.observe(seconds(0), 1, None, &cfg), None);
        assert_eq!(state.observe(seconds(29), 1, None, &cfg), None);
        assert_eq!(
            state.observe(seconds(30), 1, None, &cfg),
            Some(WatchDecision::Escalate {
                reason: "screen unchanged for 30s".to_string()
            })
        );
        assert_eq!(state.observe(seconds(60), 1, None, &cfg), None);
    }

    #[test]
    fn screen_changes_reset_the_episode_and_enter_budget() {
        let cfg = WatchCfg::default();
        let rules = RuleSet::compile(&[]).unwrap();
        let matched = rules.matched("Press enter to continue").unwrap();
        let mut state = WatchState::default();

        state.observe(seconds(0), 1, Some(&matched), &cfg);
        assert!(matches!(
            state.observe(seconds(5), 1, Some(&matched), &cfg),
            Some(WatchDecision::Enter { .. })
        ));
        assert_eq!(state.observe(seconds(6), 2, None, &cfg), None);
        assert_eq!(state.observe(seconds(35), 2, None, &cfg), None);
        assert!(matches!(
            state.observe(seconds(36), 2, None, &cfg),
            Some(WatchDecision::Escalate { .. })
        ));
    }

    #[test]
    fn unchanged_screen_after_enter_escalates() {
        let cfg = WatchCfg::default();
        let rules = RuleSet::compile(&[]).unwrap();
        let matched = rules.matched("Enter to confirm").unwrap();
        let mut state = WatchState::default();

        state.observe(seconds(0), 9, Some(&matched), &cfg);
        assert!(matches!(
            state.observe(seconds(5), 9, Some(&matched), &cfg),
            Some(WatchDecision::Enter { .. })
        ));
        assert_eq!(
            state.observe(seconds(8), 9, Some(&matched), &cfg),
            Some(WatchDecision::Escalate {
                reason: "screen unchanged after automatic Enter".to_string()
            })
        );
    }

    #[test]
    fn disabled_or_exhausted_auto_enter_escalates() {
        let rules = RuleSet::compile(&[]).unwrap();
        let matched = rules.matched("Enter to confirm").unwrap();
        let mut disabled = WatchCfg::default();
        disabled.auto_enter = false;
        let mut state = WatchState::default();
        state.observe(seconds(0), 1, Some(&matched), &disabled);
        assert!(matches!(
            state.observe(seconds(5), 1, Some(&matched), &disabled),
            Some(WatchDecision::Escalate { .. })
        ));

        let mut exhausted = WatchCfg::default();
        exhausted.max_enters = 0;
        let mut state = WatchState::default();
        state.observe(seconds(0), 1, Some(&matched), &exhausted);
        assert!(matches!(
            state.observe(seconds(5), 1, Some(&matched), &exhausted),
            Some(WatchDecision::Escalate { .. })
        ));
    }

    #[test]
    fn ignore_rules_suppress_unknown_escalation() {
        let cfg = WatchCfg::default();
        let rules = RuleSet::compile(&[]).unwrap();
        let matched = rules.matched("? for shortcuts").unwrap();
        let mut state = WatchState::default();

        state.observe(seconds(0), 1, Some(&matched), &cfg);
        assert_eq!(state.observe(seconds(600), 1, Some(&matched), &cfg), None);
    }

    #[test]
    fn cooldown_spans_content_change_episodes() {
        let cfg = WatchCfg::default();
        let mut state = WatchState::default();

        state.observe(seconds(0), 1, None, &cfg);
        assert!(matches!(
            state.observe(seconds(30), 1, None, &cfg),
            Some(WatchDecision::Escalate { .. })
        ));
        state.observe(seconds(31), 2, None, &cfg);
        assert_eq!(state.observe(seconds(61), 2, None, &cfg), None);
        assert!(matches!(
            state.observe(seconds(630), 2, None, &cfg),
            Some(WatchDecision::Escalate { .. })
        ));
    }
}
