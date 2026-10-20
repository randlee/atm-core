//! Hook registration storage contract (ADR-068 D1, ADR-069 D1, design §4).
//!
//! The trait is storage-neutral: the SQLite backend lands in bk-6 and the
//! in-memory double lives in `crate::testing::hook_registration_store`.
//! Stores read no clock; every timestamp is an argument.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::contract::sealed;
use crate::error::AtmError;
use crate::types::{AgentName, IsoTimestamp, TeamName};

/// A hook name the hook surface accepts: the six task hooks and the two cron
/// hooks (ADR-068 D3, design §2.1).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HookName(String);

impl HookName {
    /// All supported hooks, in the order the design lists them.
    pub const ALL_HOOKS: [&'static str; 8] = [
        "task.assign",
        "task.start",
        "task.close",
        "task.move",
        "task.list",
        "task.events",
        "cron.tick",
        "cron.stop",
    ];
    /// The six task hooks, in the order the design lists them.
    pub const TASK_HOOKS: [&'static str; 6] = [
        Self::ALL_HOOKS[0],
        Self::ALL_HOOKS[1],
        Self::ALL_HOOKS[2],
        Self::ALL_HOOKS[3],
        Self::ALL_HOOKS[4],
        Self::ALL_HOOKS[5],
    ];
    pub const CRON_TICK: &'static str = Self::ALL_HOOKS[6];
    pub const CRON_STOP: &'static str = Self::ALL_HOOKS[7];

    /// Constructs the cron tick hook name without validating a static name.
    pub fn cron_tick() -> Self {
        Self(Self::CRON_TICK.to_owned())
    }

    /// Constructs the cron stop hook name without validating a static name.
    pub fn cron_stop() -> Self {
        Self(Self::CRON_STOP.to_owned())
    }

    /// Accepts any of the eight hook names.
    pub fn parse(name: &str) -> Result<Self, AtmError> {
        if Self::ALL_HOOKS.contains(&name) {
            Ok(Self(name.to_owned()))
        } else {
            Err(AtmError::hook_module_invalid(&format!(
                "unknown hook name {name}; expected one of {}",
                Self::ALL_HOOKS.join(", ")
            )))
        }
    }

    /// Accepts exactly the six task hooks; the register route's only validator.
    pub fn parse_task_hook(name: &str) -> Result<Self, AtmError> {
        let hook = Self::parse(name)?;
        if Self::TASK_HOOKS.contains(&hook.as_str()) {
            Ok(hook)
        } else {
            Err(AtmError::hook_module_invalid(&format!(
                "{name} is not a task hook; expected one of {}",
                Self::TASK_HOOKS.join(", ")
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for HookName {
    type Error = AtmError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<HookName> for String {
    fn from(value: HookName) -> Self {
        value.0
    }
}

impl std::fmt::Display for HookName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A phase id: `^[a-z][a-z0-9-]{0,31}$`, checked with plain character tests.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PhaseId(String);

impl PhaseId {
    pub fn parse(value: &str) -> Result<Self, AtmError> {
        if is_phase_id(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(AtmError::validation_with_recovery(
                format!("phase id {value} must match ^[a-z][a-z0-9-]{{0,31}}$"),
                "use a lowercase phase id such as bk or bk-2",
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_phase_id(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 32
        && first.is_ascii_lowercase()
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

impl TryFrom<String> for PhaseId {
    type Error = AtmError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<PhaseId> for String {
    fn from(value: PhaseId) -> Self {
        value.0
    }
}

impl std::fmt::Display for PhaseId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One hook registration row, keyed by `(team, phase, hook)` (D-5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookRegistration {
    pub team: TeamName,
    pub phase: PhaseId,
    pub hook: HookName,
    pub module_path: PathBuf,
    pub cwd: PathBuf,
    pub timeout_ms: u32,
    /// Registration-time custom JSON object (design §2.1).
    pub custom: serde_json::Map<String, serde_json::Value>,
    pub active: bool,
    pub registered_by: AgentName,
    pub registered_at: IsoTimestamp,
    pub disabled_at: Option<IsoTimestamp>,
}

/// Sealed, storage-neutral hook registration capability.
pub trait HookRegistrationStore: sealed::Sealed + Send + Sync {
    /// Upsert on `(team, phase, hook)`; other phases' rows are untouched, and
    /// any number may be active (D-5). The caller stamps
    /// `registration.registered_at`; the store reads no clock.
    ///
    /// An active key replaces `module_path`, `cwd`, `timeout_ms`, `custom` and
    /// `registered_by`, and keeps the stored `registered_at` (chain position).
    /// An inactive key takes the argument's `registered_at`, clears
    /// `disabled_at` and becomes active.
    fn register(&self, registration: HookRegistration) -> Result<HookRegistration, AtmError>;

    /// Sets `active = false` and `disabled_at = at` on the active rows of
    /// `(team, phase)`, or of one hook. An inactive row keeps its
    /// `disabled_at`. Returns only the rows it changed.
    fn disable(
        &self,
        team: &TeamName,
        phase: &PhaseId,
        hook: Option<&HookName>,
        at: IsoTimestamp,
    ) -> Result<Vec<HookRegistration>, AtmError>;

    /// Every active row for `(team, hook)`, one per phase, ordered by
    /// `registered_at` ascending then phase (D-5).
    fn active(&self, team: &TeamName, hook: &HookName) -> Result<Vec<HookRegistration>, AtmError>;

    /// Every row of the team, with an optional phase filter. Active rows come
    /// first, then `registered_at`, then phase.
    fn list(
        &self,
        team: &TeamName,
        phase: Option<&PhaseId>,
    ) -> Result<Vec<HookRegistration>, AtmError>;

    /// Every active row of every team, ordered by team, hook, `registered_at`
    /// ascending, then phase: the snapshot source (D-8).
    fn active_all(&self) -> Result<Vec<HookRegistration>, AtmError>;
}
