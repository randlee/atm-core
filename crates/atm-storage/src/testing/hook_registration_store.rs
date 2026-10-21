//! In-memory `HookRegistrationStore` double (feature `test-utils`).
//!
//! Every trait call bumps `calls`, so tests can assert how often a route
//! touched the store (ANC-014).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use crate::contract::sealed;
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::hook_registration_store::{HookRegistration, HookRegistrationStore};
use crate::hook_types::{HookName, PhaseId};
use crate::types::{IsoTimestamp, TeamName};

/// Longest a pause handshake may wait before the test fails instead of hanging.
const PAUSE_DEADLINE: Duration = Duration::from_secs(10);

/// Keeps every registration row in memory, keyed by `(team, phase, hook)`.
/// `Mutex` and atomics because the trait mutates through `&self` and requires `Send + Sync`; `SeqCst` is deliberate (test counters, not a hot path).
#[derive(Debug, Default)]
pub struct InMemoryHookRegistrationStore {
    rows: Mutex<Vec<HookRegistration>>,
    pub calls: AtomicUsize,
    /// When set, `active_all` fails every call with an injected error (refresh failure tests).
    pub fail_active_all: AtomicBool,
    pause: Mutex<Option<PauseSlot>>,
}

/// The store's half of an [`ActiveAllPause`], taken by the next `active_all`.
#[derive(Debug)]
struct PauseSlot {
    entered: SyncSender<()>,
    release: Receiver<()>,
}

/// The test's half of a pause: waits for a paused `active_all` to reach the
/// store, then lets it finish.
#[derive(Debug)]
pub struct ActiveAllPause {
    entered: Mutex<Receiver<()>>,
    release: SyncSender<()>,
    entry_deadline: Duration,
}

impl ActiveAllPause {
    /// Blocks until a store read has reached the pause. Panics if none arrives
    /// within this pause's entry deadline, so a missed pause fails instead of hanging.
    pub fn wait_entered(&self) {
        let entered = self.entered.lock().unwrap_or_else(PoisonError::into_inner);
        let result = if self.entry_deadline.is_zero() {
            // A negative test can inspect the empty channel without using a clock.
            entered.try_recv().map_err(|error| match error {
                TryRecvError::Empty => RecvTimeoutError::Timeout,
                TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
            })
        } else {
            entered.recv_timeout(self.entry_deadline)
        };
        match result {
            Err(RecvTimeoutError::Timeout) => panic!(
                "hook registration pause: timed out after {:?} waiting for a paused active_all to reach the pause (entered)",
                self.entry_deadline
            ),
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
        }
    }

    /// Lets the paused read finish with the rows it has read.
    pub fn release(&self) {
        let _ = self.release.send(());
    }
}

impl InMemoryHookRegistrationStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The next `active_all` reports entry and then blocks until the returned
    /// pause is released; later calls run unpaused.
    pub fn pause_next_active_all(&self) -> ActiveAllPause {
        self.pause_next_active_all_with_deadline(PAUSE_DEADLINE)
    }

    /// Like `pause_next_active_all`, with a deadline for observing entry.
    /// The store's release wait retains its normal 10-second safety limit.
    pub fn pause_next_active_all_with_deadline(&self, entry_deadline: Duration) -> ActiveAllPause {
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        *self.pause.lock().unwrap_or_else(PoisonError::into_inner) = Some(PauseSlot {
            entered: entered_tx,
            release: release_rx,
        });
        ActiveAllPause {
            entered: Mutex::new(entered_rx),
            release: release_tx,
            entry_deadline,
        }
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
        let pause = self
            .pause
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(pause) = pause {
            let _ = pause.entered.send(());
            match pause.release.recv_timeout(PAUSE_DEADLINE) {
                Err(RecvTimeoutError::Timeout) => panic!(
                    "hook registration pause: timed out after {PAUSE_DEADLINE:?} waiting for release of the paused active_all (release)"
                ),
                Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_entered_panics_naming_entered_when_no_read_reaches_the_pause() {
        let store = InMemoryHookRegistrationStore::new();
        let pause = store.pause_next_active_all_with_deadline(Duration::ZERO);

        let failure =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pause.wait_entered()))
                .expect_err("wait_entered must panic when no read reaches the pause");
        let message = failure
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| failure.downcast_ref::<&str>().copied())
            .unwrap_or_default();

        assert!(message.contains("(entered)"), "{message}");
        assert_eq!(store.calls.load(Ordering::SeqCst), 0);
    }
}
