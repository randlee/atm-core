//! Generation-fenced cron schedule admission for dispatched actions.

use super::*;

impl StorageAndNudgeRouter {
    pub(crate) async fn admit_cron_run_permit(
        &self,
        permit: &atm_core::boundary::CronRunPermit,
        team: &atm_core::types::TeamName,
        phase: &atm_core::boundary::PhaseId,
        deadline: RequestDeadline,
    ) -> Result<(), AtmError> {
        if deadline.expired() {
            return Err(AtmError::daemon_unavailable(
                "cron action deadline expired before permit admission",
            ));
        }
        let store = self.service_runtime.cron_schedule_store()?;
        let schedule_id = permit.schedule_id();
        let generation = permit.generation();
        let team = team.clone();
        let phase = phase.clone();
        let remaining = deadline.remaining().ok_or_else(|| {
            AtmError::daemon_unavailable("cron action deadline expired before permit admission")
        })?;
        tokio::time::timeout(remaining, tokio::task::spawn_blocking(move || {
                let schedules = store.active_all()?;
                let admitted = schedules.iter().any(|schedule| {
                    schedule.active
                        && schedule.id == schedule_id
                        && schedule.generation == generation
                        && schedule.team == team
                        && schedule.phase == phase
                });
                if admitted {
                    Ok(())
                } else {
                    Err(AtmError::validation_with_recovery(
                        "cron action permit is stale or its schedule is no longer active",
                        "allow the current cron run to stop and let the scheduler start a fresh generation",
                    ))
                }
            }))
            .await
            .map_err(|_| AtmError::daemon_unavailable("cron action permit admission timed out"))?
            .map_err(|_| AtmError::daemon_unavailable("cron action permit admission task failed"))?
    }
}
