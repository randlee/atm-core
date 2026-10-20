//! Cron schedule storage contract (ADR-069 D4, design §5 and §9.7).
//!
//! Stores read no clock: `created_at`, `ended_at`, `last_run_at` and the
//! stop/record timestamps are arguments. A run that outlives its slot is
//! fenced by `(id, generation)`, so a restarted schedule never receives a
//! stale run's write.

use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::contract::sealed;
use crate::error::AtmError;
use crate::hook_registration_store::PhaseId;
use crate::types::{AgentName, IsoTimestamp, TeamName};

/// Stable row id of one cron schedule. Display prints the integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CronScheduleId(pub u64);

impl fmt::Display for CronScheduleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One cron schedule row. Exactly one stop condition per schedule (D-9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronSchedule {
    pub id: CronScheduleId,
    pub generation: u32,
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
    Until { at: DateTime<Utc> },
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
    /// No row for `(team, phase, name)`: insert with generation 1. An ended
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
        generation: u32,
        at: IsoTimestamp,
        outcome: &str,
    ) -> Result<bool, AtmError>;

    /// The stop condition fired (ADR-069 D4): `active = 0`, `ended_at = at`,
    /// `last_outcome = "stopped: <reason>"`, under the same `WHERE` clause as
    /// [`CronScheduleStore::record_run`].
    fn record_stop(
        &self,
        id: CronScheduleId,
        generation: u32,
        at: IsoTimestamp,
        reason: &str,
    ) -> Result<bool, AtmError>;
}
