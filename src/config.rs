use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionPolicy {
    #[default]
    Queue,
    Restart,
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchOptions {
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub default_ignores: bool,
    pub workdir: Option<PathBuf>,
    pub poll_ms: Option<u64>,
    pub debounce_ms: u64,
    pub max_wait_ms: u64,
    pub timeout_ms: Option<u64>,
    pub policy: ExecutionPolicy,
    pub initial: bool,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            includes: vec![],
            excludes: vec![],
            default_ignores: true,
            workdir: None,
            poll_ms: None,
            debounce_ms: 500,
            max_wait_ms: 5000,
            timeout_ms: None,
            policy: ExecutionPolicy::Queue,
            initial: false,
        }
    }
}

impl WatchOptions {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("poll", self.poll_ms),
            ("timeout", self.timeout_ms),
            ("max-wait", Some(self.max_wait_ms)),
        ] {
            if value == Some(0) {
                return Err(format!("--{name} must be greater than zero"));
            }
        }
        // Bound durations so Instant arithmetic remains representable on all platforms.
        if [
            Some(self.debounce_ms),
            Some(self.max_wait_ms),
            self.poll_ms,
            self.timeout_ms,
        ]
        .into_iter()
        .flatten()
        .any(|ms| ms > 31_536_000_000)
        {
            return Err("durations must not exceed one year".into());
        }
        for pattern in self.includes.iter().chain(&self.excludes) {
            glob::Pattern::new(pattern).map_err(|e| format!("invalid pattern {pattern:?}: {e}"))?;
        }
        Ok(())
    }
}
