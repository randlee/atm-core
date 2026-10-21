//! Cron schedule storage contract (ADR-069 D4, design §5 and §9.7).
//!
//! Stores read no clock: `created_at`, `ended_at`, `last_run_at` and the
//! stop/record timestamps are arguments. A run that outlives its slot is
//! fenced by `(id, generation)`, so a restarted schedule never receives a
//! stale run's write.

use std::fmt;
use std::num::NonZeroU32;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::contract::sealed;
use crate::error::AtmError;
use crate::hook_types::PhaseId;
use crate::types::{AgentName, IsoTimestamp, TeamName};

/// Stable row id of one cron schedule. Display prints the integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CronScheduleId(u64);

impl CronScheduleId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for CronScheduleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get())
    }
}

/// Monotonically increasing fencing generation for one cron schedule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct CronGeneration(NonZeroU32);

impl CronGeneration {
    pub const FIRST: Self = Self(NonZeroU32::MIN);

    pub fn new(value: u32) -> Result<Self, AtmError> {
        NonZeroU32::new(value).map(Self).ok_or_else(|| {
            AtmError::validation_with_recovery(
                "cron generation must be at least 1",
                "generations start at 1",
            )
        })
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }

    pub fn next(self) -> Option<Self> {
        self.get()
            .checked_add(1)
            .and_then(NonZeroU32::new)
            .map(Self)
    }
}

impl TryFrom<u32> for CronGeneration {
    type Error = AtmError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<CronGeneration> for u32 {
    fn from(value: CronGeneration) -> Self {
        value.get()
    }
}

impl fmt::Display for CronGeneration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get())
    }
}

/// One cron schedule row. Exactly one stop condition per schedule (D-9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronSchedule {
    pub id: CronScheduleId,
    pub generation: CronGeneration,
    pub team: TeamName,
    pub phase: PhaseId,
    pub name: String,
    pub cron_expr: String,
    pub module_path: PathBuf,
    pub cwd: PathBuf,
    pub args: Vec<String>,
    pub timeout_ms: u32,
    pub jitter_s: u32,
    pub custom: serde_json::Map<String, serde_json::Value>,
    pub stop: StopCondition,
    pub active: bool,
    pub created_by: AgentName,
    pub created_at: IsoTimestamp,
    pub last_run_at: Option<IsoTimestamp>,
    pub last_outcome: Option<String>,
    pub ended_at: Option<IsoTimestamp>,
}

/// When a cron schedule stops by itself (ADR-069 D4): an instant, or the
/// phase's `stop` hook reporting the work finished.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopCondition {
    Until { at: IsoTimestamp },
    Hook,
}

/// The caller's specification for a new or restarted cron schedule.
/// `jitter_s` and `created_at` come from `AtmCron`'s injected clock (bk-4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewCronSchedule {
    pub team: TeamName,
    pub phase: PhaseId,
    pub name: String,
    pub cron_expr: String,
    pub module_path: PathBuf,
    pub cwd: PathBuf,
    pub args: Vec<String>,
    pub timeout_ms: u32,
    pub jitter_s: u32,
    pub custom: serde_json::Map<String, serde_json::Value>,
    pub stop: StopCondition,
    pub created_by: AgentName,
    pub created_at: IsoTimestamp,
}

/// Sealed, storage-neutral cron schedule capability.
pub trait CronScheduleStore: sealed::Sealed + Send + Sync {
    /// No row for `(team, phase, name)`: insert with [`CronGeneration::FIRST`]. An ended
    /// row is updated in place in one transaction: id kept, generation + 1,
    /// every field from `schedule`, `active = 1`, and `ended_at`,
    /// `last_run_at` and `last_outcome` cleared (D-i). An active row is a
    /// validation error naming `atm cron end`.
    fn start(&self, schedule: NewCronSchedule) -> Result<CronSchedule, AtmError>;

    /// Ends the active rows of `(team, phase)`, or the one named schedule.
    /// Returns the rows it changed.
    fn end(
        &self,
        team: &TeamName,
        phase: &PhaseId,
        name: Option<&str>,
        at: IsoTimestamp,
    ) -> Result<Vec<CronSchedule>, AtmError>;

    /// Active rows first, then `created_at`, then phase, then name.
    fn list(
        &self,
        team: Option<&TeamName>,
        phase: Option<&PhaseId>,
    ) -> Result<Vec<CronSchedule>, AtmError>;

    /// Every active row, for the daemon's start (ANC-020).
    fn active_all(&self) -> Result<Vec<CronSchedule>, AtmError>;

    /// `UPDATE .. WHERE id = ? AND generation = ? AND active = 1`: returns
    /// `Ok(false)` when the row was ended or restarted since the run began,
    /// so a run that outlives its slot never writes the restarted row.
    fn record_run(
        &self,
        id: CronScheduleId,
        generation: CronGeneration,
        at: IsoTimestamp,
        outcome: &str,
    ) -> Result<bool, AtmError>;

    /// The stop condition fired (ADR-069 D4): `active = 0`, `ended_at = at`,
    /// `last_outcome = "stopped: <reason>"`, under the same `WHERE` clause as
    /// [`CronScheduleStore::record_run`].
    fn record_stop(
        &self,
        id: CronScheduleId,
        generation: CronGeneration,
        at: IsoTimestamp,
        reason: &str,
    ) -> Result<bool, AtmError>;
}

#[cfg(test)]
mod tests {
    use super::{CronGeneration, CronScheduleId, StopCondition};
    use crate::types::IsoTimestamp;

    #[test]
    fn cron_schedule_id_construction_and_reading_are_explicit() {
        let id = CronScheduleId::new(42);
        assert_eq!(id.get(), 42);
        assert_eq!(id.to_string(), "42");
    }

    #[test]
    fn cron_generation_rejects_zero_and_counts_up() {
        assert!(CronGeneration::new(0).is_err());
        assert_eq!(CronGeneration::FIRST.get(), 1);
        assert_eq!(
            CronGeneration::FIRST.next().expect("next generation").get(),
            2
        );
        assert_eq!(
            CronGeneration::new(u32::MAX)
                .expect("maximum generation")
                .next(),
            None
        );
        assert!(serde_json::from_value::<CronGeneration>(serde_json::json!(0)).is_err());
        assert_eq!(
            serde_json::to_value(CronGeneration::FIRST).expect("serialize generation"),
            serde_json::json!(1)
        );
    }

    #[test]
    fn until_stop_serializes_the_iso_timestamp() {
        let at: IsoTimestamp = "2026-10-21T00:00:00Z".parse().expect("timestamp");
        assert_eq!(
            serde_json::to_value(StopCondition::Until { at }).expect("serialize"),
            serde_json::json!({"kind": "until", "at": "2026-10-21T00:00:00Z"})
        );
    }
}
