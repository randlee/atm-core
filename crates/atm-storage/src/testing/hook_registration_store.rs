//! In-memory `HookRegistrationStore` double (feature `test-utils`).
//!
//! Every trait call bumps `calls`, so tests can assert how often a route
//! touched the store (ANC-014).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::contract::sealed;
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::hook_registration_store::{HookName, HookRegistration, HookRegistrationStore, PhaseId};
use crate::types::{IsoTimestamp, TeamName};

/// Keeps every registration row in memory, keyed by `(team, phase, hook)`.
#[derive(Debug, Default)]
pub struct InMemoryHookRegistrationStore {
    rows: Mutex<Vec<HookRegistration>>,
    pub calls: AtomicUsize,
    /// When set, `active_all` fails once with an injected error (refresh failure tests).
    pub fail_active_all: AtomicBool,
}

impl InMemoryHookRegistrationStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn record_call(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Vec<HookRegistration>>, AtmError> {
        self.rows.lock().map_err(|_| {
            AtmError::new(
                AtmErrorCode::InternalError,
                "in-memory hook registration rows lock poisoned",
            )
        })
    }
}

impl sealed::Sealed for InMemoryHookRegistrationStore {}

fn ordered_by_registration(rows: &mut [HookRegistration]) {
    rows.sort_by(|a, b| {
        a.registered_at
            .cmp(&b.registered_at)
            .then_with(|| a.phase.cmp(&b.phase))
    });
}

impl HookRegistrationStore for InMemoryHookRegistrationStore {
    fn register(&self, registration: HookRegistration) -> Result<HookRegistration, AtmError> {
        self.record_call();
        let mut rows = self.lock()?;
        let index = rows.iter().position(|row| {
            row.team == registration.team
                && row.phase == registration.phase
                && row.hook == registration.hook
        });
        let stored = match index {
            Some(index) if rows[index].active => {
                let row = &mut rows[index];
                row.module_path = registration.module_path;
                row.cwd = registration.cwd;
                row.timeout_ms = registration.timeout_ms;
                row.custom = registration.custom;
                row.registered_by = registration.registered_by;
                row.clone()
            }
            Some(index) => {
                rows[index] = HookRegistration {
                    active: true,
                    disabled_at: None,
                    ..registration
                };
                rows[index].clone()
            }
            None => {
                let row = HookRegistration {
                    active: true,
                    disabled_at: None,
                    ..registration
                };
                rows.push(row.clone());
                row
            }
        };
        Ok(stored)
    }

    fn disable(
        &self,
        team: &TeamName,
        phase: &PhaseId,
        hook: Option<&HookName>,
        at: IsoTimestamp,
    ) -> Result<Vec<HookRegistration>, AtmError> {
        self.record_call();
        let mut rows = self.lock()?;
        let mut changed = Vec::new();
        for row in rows.iter_mut().filter(|row| {
            row.active
                && row.team == *team
                && row.phase == *phase
                && hook.is_none_or(|hook| row.hook == *hook)
        }) {
            row.active = false;
            row.disabled_at = Some(at);
            changed.push(row.clone());
        }
        Ok(changed)
    }

    fn active(&self, team: &TeamName, hook: &HookName) -> Result<Vec<HookRegistration>, AtmError> {
        self.record_call();
        let rows = self.lock()?;
        let mut selected: Vec<HookRegistration> = rows
            .iter()
            .filter(|row| row.active && row.team == *team && row.hook == *hook)
            .cloned()
            .collect();
        ordered_by_registration(&mut selected);
        Ok(selected)
    }

    fn list(
        &self,
        team: &TeamName,
        phase: Option<&PhaseId>,
    ) -> Result<Vec<HookRegistration>, AtmError> {
        self.record_call();
        let rows = self.lock()?;
        let mut selected: Vec<HookRegistration> = rows
            .iter()
            .filter(|row| row.team == *team && phase.is_none_or(|phase| row.phase == *phase))
            .cloned()
            .collect();
        selected.sort_by(|a, b| {
            (!a.active)
                .cmp(&!b.active)
                .then_with(|| a.registered_at.cmp(&b.registered_at))
                .then_with(|| a.phase.cmp(&b.phase))
        });
        Ok(selected)
    }

    fn active_all(&self) -> Result<Vec<HookRegistration>, AtmError> {
        self.record_call();
        if self.fail_active_all.load(Ordering::SeqCst) {
            return Err(AtmError::new(
                AtmErrorCode::InternalError,
                "injected hook registration active_all failure",
            ));
        }
        let rows = self.lock()?;
        let mut selected: Vec<HookRegistration> =
            rows.iter().filter(|row| row.active).cloned().collect();
        selected.sort_by(|a, b| {
            a.team
                .cmp(&b.team)
                .then_with(|| a.hook.cmp(&b.hook))
                .then_with(|| a.registered_at.cmp(&b.registered_at))
                .then_with(|| a.phase.cmp(&b.phase))
        });
        Ok(selected)
    }
}
