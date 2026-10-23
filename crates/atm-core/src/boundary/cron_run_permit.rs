//! Scheduler-owned generation fence for cron action admission.
//!
//! A permit is deliberately non-serializable and has no public constructor.
//! The scheduler mints it after reading the active row; action admission uses
//! the contained `(schedule_id, generation)` fence to refuse work after an
//! end or restart without attempting to undo an already committed action.

use atm_storage::{CronGeneration, CronScheduleId};

#[derive(Debug)]
pub struct CronRunPermit {
    schedule_id: CronScheduleId,
    generation: CronGeneration,
    _private: (),
}

impl CronRunPermit {
    /// Scheduler composition only. Hook modules and child environments never
    /// receive a permit or its raw fence fields.
    #[allow(dead_code)]
    pub(crate) fn minted_by_scheduler(
        schedule_id: CronScheduleId,
        generation: CronGeneration,
    ) -> Self {
        Self {
            schedule_id,
            generation,
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
