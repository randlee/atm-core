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

use crate::hook_types::{HookName, PhaseId};

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
