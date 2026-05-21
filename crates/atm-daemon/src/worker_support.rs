use atm_core::error::AtmError;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;
#[cfg(test)]
use std::{
    sync::{Condvar, LazyLock},
    time::Instant,
};

const MAX_RETAINED_JOIN_HELPERS: usize = 16;

#[derive(Debug)]
struct RetainedJoinHelper {
    label: &'static str,
    deadline: Duration,
    join_helper: JoinHandle<()>,
}

// Narrow RBP-006 exception: this process-global registry owns only timed-out
// join helpers that must outlive one bounded shutdown attempt. It must not
// expand into shared runtime coordination, queue ownership, or request state.
static RETAINED_JOIN_HELPERS: Mutex<Vec<RetainedJoinHelper>> = Mutex::new(Vec::new());

#[cfg(test)]
static RETAINED_JOIN_HELPER_EXIT_SIGNAL: LazyLock<(Mutex<u64>, Condvar)> =
    LazyLock::new(|| (Mutex::new(0), Condvar::new()));

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
    // Timeout retention is intentional: once bounded shutdown expires, keeping
    // the join helper is safer than dropping lifecycle ownership silently.
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
            current_len = retained.len(),
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
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .len()
}

#[cfg(test)]
pub(crate) fn reap_retained_join_helpers_until_empty_for_test() {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut observed_epoch = retained_join_helper_exit_epoch_for_test();
    loop {
        reap_retained_join_helpers();
        if retained_join_helper_count_for_test() == 0 {
            return;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        observed_epoch = wait_for_retained_join_helper_exit_for_test(
            observed_epoch,
            deadline.saturating_duration_since(now),
        );
    }
    panic!("retained join helpers were not reaped after the worker completion signal");
}

#[cfg(test)]
pub(crate) fn signal_retained_join_helper_exit_for_test() {
    let (epoch_lock, wake) = &*RETAINED_JOIN_HELPER_EXIT_SIGNAL;
    let mut epoch = epoch_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *epoch = epoch.saturating_add(1);
    wake.notify_all();
}

#[cfg(test)]
fn retained_join_helper_exit_epoch_for_test() -> u64 {
    let (epoch_lock, _) = &*RETAINED_JOIN_HELPER_EXIT_SIGNAL;
    *epoch_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
fn wait_for_retained_join_helper_exit_for_test(previous_epoch: u64, timeout: Duration) -> u64 {
    let (epoch_lock, wake) = &*RETAINED_JOIN_HELPER_EXIT_SIGNAL;
    let epoch = epoch_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (epoch, _) = wake
        .wait_timeout_while(epoch, timeout, |epoch| *epoch == previous_epoch)
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *epoch
}
