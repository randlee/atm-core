//! Test observation of the replacement-daemon shutdown sequence.
//!
//! The shutdown path calls these hooks unconditionally; production builds
//! compile them to nothing (`step` only awaits its future, `record` ignores
//! its arguments, the stall is a no-op and the gate is ready). Test builds
//! consult a [`Probe`] installed for the current task by [`observe`]: it
//! records each step's start, deadline and return, and can hold the logger
//! flush or the shared export-shutdown step open so a test decides when they
//! end instead of a timer.

use std::future::Future;

use tokio::time::Instant;

/// Awaits one shutdown step that was given `deadline`, recording it as
/// `name` for an observing test.
pub(crate) async fn step<F: Future>(name: &'static str, deadline: Instant, step: F) -> F::Output {
    #[cfg(test)]
    let started = Instant::now();
    let output = step.await;
    #[cfg(test)]
    record(name, started, deadline);
    #[cfg(not(test))]
    let _ = (name, deadline);
    output
}

/// Records a step that started at `started` with `deadline` and returned now.
pub(crate) fn record(name: &'static str, started: Instant, deadline: Instant) {
    #[cfg(test)]
    if let Some(probe) = test_probe::current() {
        probe.push(name, started, deadline);
    }
    #[cfg(not(test))]
    let _ = (name, started, deadline);
}

/// Work to run on the blocking pool before a logger flush.
pub(crate) fn logger_flush_stall() -> impl FnOnce() + Send + 'static {
    #[cfg(test)]
    let stall = test_probe::current().and_then(|probe| probe.flush_stall());
    move || {
        #[cfg(test)]
        if let Some(stall) = stall {
            stall();
        }
    }
}

/// Completes when the shared export-shutdown step may publish its outcome.
/// The probe is read here, on the caller's task, because the step itself runs
/// in a spawned task outside the probe's scope.
pub(crate) fn export_gate() -> impl Future<Output = ()> + Send + 'static {
    #[cfg(test)]
    {
        let gate = test_probe::current().and_then(|probe| probe.take_export_gate());
        async move {
            if let Some(gate) = gate {
                // Released by a send or by the sender's drop.
                drop(gate.await);
            }
        }
    }
    #[cfg(not(test))]
    std::future::ready(())
}

#[cfg(test)]
pub(crate) use test_probe::{Probe, ShutdownStep, observe};

#[cfg(test)]
mod test_probe {
    use std::future::Future;
    use std::sync::{Arc, Mutex, OnceLock, PoisonError};

    use tokio::sync::oneshot;
    use tokio::time::Instant;

    tokio::task_local! {
        static PROBE: Arc<Probe>;
    }

    /// One bounded shutdown step as a test observed it: when it started, the
    /// absolute deadline it was given or computed, and when it returned.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct ShutdownStep {
        pub(crate) step: &'static str,
        pub(crate) started: Instant,
        pub(crate) deadline: Instant,
        pub(crate) returned: Instant,
    }

    type Stall = Arc<dyn Fn() + Send + Sync>;

    /// What one test observes and controls of the shutdown steps it awaits.
    #[derive(Default)]
    pub(crate) struct Probe {
        steps: Mutex<Vec<ShutdownStep>>,
        flush_stall: OnceLock<Stall>,
        export_gate: Mutex<Option<oneshot::Receiver<()>>>,
    }

    impl Probe {
        pub(crate) fn new() -> Arc<Self> {
            Arc::default()
        }

        /// Every recorded step, in call order.
        pub(crate) fn steps(&self) -> Vec<ShutdownStep> {
            self.steps
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        /// Runs `stall` on the blocking pool before every observed logger
        /// flush. First install wins.
        pub(crate) fn stall_logger_flush(&self, stall: impl Fn() + Send + Sync + 'static) {
            let _ = self.flush_stall.set(Arc::new(stall));
        }

        /// Holds the next observed export-shutdown step open after its
        /// provider shutdowns until the returned sender is used or dropped.
        pub(crate) fn hold_export_shutdown(&self) -> oneshot::Sender<()> {
            let (release, gate) = oneshot::channel();
            *self
                .export_gate
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(gate);
            release
        }

        pub(super) fn push(&self, step: &'static str, started: Instant, deadline: Instant) {
            self.steps
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(ShutdownStep {
                    step,
                    started,
                    deadline,
                    returned: Instant::now(),
                });
        }

        pub(super) fn flush_stall(&self) -> Option<Stall> {
            self.flush_stall.get().cloned()
        }

        pub(super) fn take_export_gate(&self) -> Option<oneshot::Receiver<()>> {
            self.export_gate
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
        }
    }

    pub(super) fn current() -> Option<Arc<Probe>> {
        PROBE.try_with(Arc::clone).ok()
    }

    /// Runs `future` with `probe` observing every shutdown step it awaits on
    /// this task.
    pub(crate) async fn observe<F: Future>(probe: &Arc<Probe>, future: F) -> F::Output {
        PROBE.scope(Arc::clone(probe), future).await
    }
}
