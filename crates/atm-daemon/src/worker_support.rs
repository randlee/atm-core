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

// Narrow RBP-006 exception: this process-global registry owns only timed-out
// join helpers that must outlive one bounded shutdown attempt. It must not
// expand into shared runtime coordination, queue ownership, or request state.
static RETAINED_JOIN_HELPERS: Mutex<Vec<RetainedJoinHelper>> = Mutex::new(Vec::new());

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
