//! Scheduler-owned generation fence for cron action admission.
//!
//! A permit is deliberately non-serializable and has no public constructor.
//! The scheduler mints it after reading the active row; action admission uses
//! the contained `(schedule_id, generation)` fence to refuse work after an
//! end or restart without attempting to undo an already committed action.

use atm_storage::{CronGeneration, CronScheduleId};

use super::sealed;

#[derive(Debug)]
pub struct CronRunPermit {
    schedule_id: CronScheduleId,
    generation: CronGeneration,
    _private: (),
}

impl CronRunPermit {
    /// Projects the opaque permit from a scheduler-owned record.
    ///
    /// `CronRunPermitIssuer` is an ADR-001 workspace-convention seal whose
    /// only authorized production implementation is the private scheduler
    /// record in `atm-cron`. Hook modules and child environments never
    /// receive a permit or its raw fence fields.
    pub fn from_issuer(issuer: &impl CronRunPermitIssuer) -> Self {
        Self {
            schedule_id: issuer.schedule_id(),
            generation: issuer.generation(),
            _private: (),
        }
    }

    #[must_use]
    pub fn schedule_id(&self) -> CronScheduleId {
        self.schedule_id
    }

    #[must_use]
    pub fn generation(&self) -> CronGeneration {
        self.generation
    }
}

/// Read-only scheduler-owned state used to project a [`CronRunPermit`].
///
/// This ADR-001 workspace-convention seal is authorized only for the private
/// scheduler issuer in `atm-cron` and its deterministic test coverage. A
/// permit is never serializable or available to hook modules or child
/// environments.
pub trait CronRunPermitIssuer: sealed::Sealed {
    /// Persisted identity of the active schedule.
    fn schedule_id(&self) -> CronScheduleId;

    /// Active schedule generation checked at canonical action admission.
    fn generation(&self) -> CronGeneration;
}
