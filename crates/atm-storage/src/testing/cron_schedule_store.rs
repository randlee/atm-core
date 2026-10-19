//! In-memory `CronScheduleStore` double (feature `test-utils`).
//!
//! `calls` counts every trait call and `writes` counts `start`, `end`,
//! `record_run` and `record_stop` calls that update at least one row. `fail_active_all`
//! and `fail_writes` make those calls return an internal error, for the bk-4
//! store-error and bk-5 initial-load tests.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::contract::sealed;
use crate::cron_schedule_store::{
    CronSchedule, CronScheduleId, CronScheduleStore, NewCronSchedule,
};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::hook_registration_store::PhaseId;
use crate::types::{IsoTimestamp, TeamName};

/// Keeps every cron schedule row in memory.
#[derive(Debug, Default)]
pub struct InMemoryCronScheduleStore {
    rows: Mutex<Vec<CronSchedule>>,
    pub calls: AtomicUsize,
    pub writes: AtomicUsize,
    pub fail_active_all: AtomicBool,
    pub fail_writes: AtomicBool,
}

impl InMemoryCronScheduleStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn record_call(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }

    fn record_write(&self) {
        self.writes.fetch_add(1, Ordering::SeqCst);
    }

    fn check_writes(&self) -> Result<(), AtmError> {
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err(injected_failure());
        }
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, Vec<CronSchedule>>, AtmError> {
        self.rows.lock().map_err(|_| {
            AtmError::new(
                AtmErrorCode::InternalError,
                "in-memory cron schedule rows lock poisoned",
            )
        })
    }
}

fn injected_failure() -> AtmError {
    AtmError::new(
        AtmErrorCode::InternalError,
        "injected cron schedule store failure",
    )
}

fn ordered_for_listing(rows: &mut [CronSchedule]) {
    rows.sort_by(|a, b| {
        (!a.active)
            .cmp(&!b.active)
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.phase.cmp(&b.phase))
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn schedule_row(id: CronScheduleId, generation: u32, spec: NewCronSchedule) -> CronSchedule {
    CronSchedule {
        id,
        generation,
        team: spec.team,
        phase: spec.phase,
        name: spec.name,
        cron_expr: spec.cron_expr,
        module_path: spec.module_path,
        cwd: spec.cwd,
        args: spec.args,
        timeout_ms: spec.timeout_ms,
        jitter_s: spec.jitter_s,
        custom: spec.custom,
        stop: spec.stop,
        active: true,
        created_by: spec.created_by,
        created_at: spec.created_at,
        last_run_at: None,
        last_outcome: None,
        ended_at: None,
    }
}

impl sealed::Sealed for InMemoryCronScheduleStore {}

impl CronScheduleStore for InMemoryCronScheduleStore {
    fn start(&self, schedule: NewCronSchedule) -> Result<CronSchedule, AtmError> {
        self.record_call();
        self.check_writes()?;
        let mut rows = self.lock()?;
        let index = rows.iter().position(|row| {
            row.team == schedule.team && row.phase == schedule.phase && row.name == schedule.name
        });
        let row = match index {
            Some(index) if rows[index].active => {
                return Err(AtmError::validation_with_recovery(
                    format!(
                        "cron schedule {} already active for phase {}",
                        schedule.name, schedule.phase
                    ),
                    format!(
                        "run atm cron end --phase {} {} first",
                        schedule.phase, schedule.name
                    ),
                ));
            }
            Some(index) => {
                let id = rows[index].id;
                let generation = rows[index].generation + 1;
                rows[index] = schedule_row(id, generation, schedule);
                rows[index].clone()
            }
            None => {
                let next = rows.iter().map(|row| row.id.0).max().unwrap_or(0) + 1;
                let row = schedule_row(CronScheduleId(next), 1, schedule);
                rows.push(row.clone());
                row
            }
        };
        self.record_write();
        Ok(row)
    }

    fn end(
        &self,
        team: &TeamName,
        phase: &PhaseId,
        name: Option<&str>,
        at: IsoTimestamp,
    ) -> Result<Vec<CronSchedule>, AtmError> {
        self.record_call();
        self.check_writes()?;
        let mut rows = self.lock()?;
        let mut changed = Vec::new();
        for row in rows.iter_mut().filter(|row| {
            row.active
                && row.team == *team
                && row.phase == *phase
                && name.is_none_or(|name| row.name == name)
        }) {
            row.active = false;
            row.ended_at = Some(at);
            changed.push(row.clone());
        }
        if !changed.is_empty() {
            self.record_write();
        }
        Ok(changed)
    }

    fn list(
        &self,
        team: Option<&TeamName>,
        phase: Option<&PhaseId>,
    ) -> Result<Vec<CronSchedule>, AtmError> {
        self.record_call();
        let rows = self.lock()?;
        let mut selected: Vec<CronSchedule> = rows
            .iter()
            .filter(|row| {
                team.is_none_or(|team| row.team == *team)
                    && phase.is_none_or(|phase| row.phase == *phase)
            })
            .cloned()
            .collect();
        ordered_for_listing(&mut selected);
        Ok(selected)
    }

    fn active_all(&self) -> Result<Vec<CronSchedule>, AtmError> {
        self.record_call();
        if self.fail_active_all.load(Ordering::SeqCst) {
            return Err(injected_failure());
        }
        let rows = self.lock()?;
        let mut selected: Vec<CronSchedule> =
            rows.iter().filter(|row| row.active).cloned().collect();
        ordered_for_listing(&mut selected);
        Ok(selected)
    }

    fn record_run(
        &self,
        id: CronScheduleId,
        generation: u32,
        at: IsoTimestamp,
        outcome: &str,
    ) -> Result<bool, AtmError> {
        self.record_call();
        self.check_writes()?;
        let mut rows = self.lock()?;
        let changed = match rows
            .iter_mut()
            .find(|row| row.id == id && row.generation == generation && row.active)
        {
            Some(row) => {
                row.last_run_at = Some(at);
                row.last_outcome = Some(outcome.to_owned());
                true
            }
            None => false,
        };
        if changed {
            self.record_write();
        }
        Ok(changed)
    }

    fn record_stop(
        &self,
        id: CronScheduleId,
        generation: u32,
        at: IsoTimestamp,
        reason: &str,
    ) -> Result<bool, AtmError> {
        self.record_call();
        self.check_writes()?;
        let mut rows = self.lock()?;
        let changed = match rows
            .iter_mut()
            .find(|row| row.id == id && row.generation == generation && row.active)
        {
            Some(row) => {
                row.active = false;
                row.ended_at = Some(at);
                row.last_outcome = Some(format!("stopped: {reason}"));
                true
            }
            None => false,
        };
        if changed {
            self.record_write();
        }
        Ok(changed)
    }
}
