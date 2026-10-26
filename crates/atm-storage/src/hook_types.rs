//! Semantic hook names and phase ids shared by storage contracts and their callers.

use serde::{Deserialize, Serialize};

use crate::error::AtmError;

pub const CRON_NAME_MAX_BYTES: usize = 64;

/// A cron schedule name validated at the boundary before it reaches a scheduler.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CronName(String);

impl CronName {
    pub fn parse(value: &str) -> Result<Self, AtmError> {
        if value.is_empty() || value.len() > CRON_NAME_MAX_BYTES {
            return Err(AtmError::hook_module_invalid(
                "name must contain 1..=64 bytes",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CronName {
    type Error = AtmError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<CronName> for String {
    fn from(value: CronName) -> Self {
        value.0
    }
}

impl std::fmt::Display for CronName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod cron_name_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cron_name_parse_bounds() {
        assert_eq!(
            CronName::parse("").unwrap_err().code(),
            crate::error::AtmErrorCode::HookModuleInvalid
        );
        assert_eq!(
            CronName::parse(&"n".repeat(CRON_NAME_MAX_BYTES))
                .unwrap()
                .as_str()
                .len(),
            CRON_NAME_MAX_BYTES
        );
        assert_eq!(
            CronName::parse(&"n".repeat(CRON_NAME_MAX_BYTES + 1))
                .unwrap_err()
                .code(),
            crate::error::AtmErrorCode::HookModuleInvalid
        );
        assert!(serde_json::from_value::<CronName>(json!("")).is_err());
    }
}

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

/// The six task hooks as a closed set; converts to `HookName` without validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskHook {
    Assign = 0,
    Start = 1,
    Close = 2,
    Move = 3,
    List = 4,
    Events = 5,
}

impl TaskHook {
    pub const ALL: [TaskHook; 6] = [
        Self::Assign,
        Self::Start,
        Self::Close,
        Self::Move,
        Self::List,
        Self::Events,
    ];

    pub const fn as_str(self) -> &'static str {
        HookName::TASK_HOOKS[self as usize]
    }

    pub fn hook_name(self) -> HookName {
        HookName(self.as_str().to_owned())
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
