use atm_core::error::AtmError;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

const MAX_RETAINED_JOIN_HELPERS: usize = 16;

#[derive(Debug)]
struct RetainedJoinHelper {
    label: &'static str,
    deadline: Duration,
    join_helper: JoinHandle<()>,
}

static RETAINED_JOIN_HELPERS: Mutex<Vec<RetainedJoinHelper>> = Mutex::new(Vec::new());

#[derive(Debug, Default)]
pub(crate) struct JoinHandleOwner {
    // Narrow RBP-006 exception: this mutex owns only the install-once /
    // take-once handoff for one worker JoinHandle and must not expand into
    // general runtime coordination or request-state ownership.
    join_handle: Mutex<Option<JoinHandle<()>>>,
}

impl JoinHandleOwner {
    pub(crate) fn install(&self, handle: JoinHandle<()>) -> Result<(), AtmError> {
        let mut slot = self.join_handle.lock().map_err(|_| {
            AtmError::daemon_unavailable("worker join-handle ownership lock poisoned")
                .with_recovery(
                    "Restart atm-daemon; background worker join ownership can no longer be trusted.",
                )
        })?;
        if slot.is_some() {
            return Err(AtmError::validation(
                "worker join-handle ownership already contains a live handle",
            )
            .with_recovery(
                "Restart atm-daemon; a duplicate worker install violated the daemon worker-ownership contract.",
            ));
        }
        *slot = Some(handle);
        Ok(())
    }

    pub(crate) fn take(&self) -> Result<Option<JoinHandle<()>>, AtmError> {
        let mut slot = self.join_handle.lock().map_err(|_| {
            AtmError::daemon_unavailable("worker join-handle ownership lock poisoned")
                .with_recovery(
                    "Restart atm-daemon; background worker join ownership can no longer be trusted.",
                )
        })?;
        Ok(slot.take())
    }
}

pub(crate) fn reap_retained_join_helpers() {
    let mut completed = Vec::new();
    let mut retained = match RETAINED_JOIN_HELPERS.lock() {
        Ok(retained) => retained,
        Err(_) => {
            tracing::warn!(
                subsystem = "worker_support",
                action = "retained_join_helper_reap",
                outcome = "lock_poisoned",
                "retained join-helper registry lock poisoned; restart atm-daemon to recover worker ownership bookkeeping"
            );
            return;
        }
    };

    let mut pending = Vec::with_capacity(retained.len());
    for retained_helper in retained.drain(..) {
        if retained_helper.join_helper.is_finished() {
            completed.push(retained_helper);
        } else {
            pending.push(retained_helper);
        }
    }
    *retained = pending;
    drop(retained);

    for retained_helper in completed {
        let _ = retained_helper.join_helper.join();
        tracing::info!(
            subsystem = "worker_support",
            action = "retained_join_helper_reap",
            outcome = "joined",
            label = retained_helper.label,
            timeout_ms = retained_helper.deadline.as_millis(),
            "retained worker join helper completed and was reaped"
        );
    }
}

pub(crate) fn retain_join_helper(
    label: &'static str,
    join_helper: JoinHandle<()>,
    deadline: Duration,
) {
    reap_retained_join_helpers();

    let mut retained = match RETAINED_JOIN_HELPERS.lock() {
        Ok(retained) => retained,
        Err(_) => {
            tracing::warn!(
                subsystem = "worker_support",
                action = "retained_join_helper_store",
                outcome = "lock_poisoned",
                label,
                timeout_ms = deadline.as_millis(),
                "retained join-helper registry lock poisoned; dropping timed-out worker helper"
            );
            drop(join_helper);
            return;
        }
    };

    if retained.len() >= MAX_RETAINED_JOIN_HELPERS {
        tracing::warn!(
            subsystem = "worker_support",
            action = "retained_join_helper_store",
            outcome = "capacity_exceeded",
            label,
            cap = MAX_RETAINED_JOIN_HELPERS,
            timeout_ms = deadline.as_millis(),
            "retained join-helper registry is full; dropping timed-out worker helper"
        );
        drop(join_helper);
        return;
    }

    retained.push(RetainedJoinHelper {
        label,
        deadline,
        join_helper,
    });
    tracing::warn!(
        subsystem = "worker_support",
        action = "retained_join_helper_store",
        outcome = "retained",
        label,
        timeout_ms = deadline.as_millis(),
        "worker join helper retained after bounded shutdown timeout"
    );
}

#[cfg(test)]
pub(crate) fn retained_join_helper_count_for_test() -> usize {
    RETAINED_JOIN_HELPERS
        .lock()
        .expect("retained join helper registry lock")
        .len()
}
