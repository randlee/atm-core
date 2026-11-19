//! Tokio deadline propagation across the core task-command boundary.

use std::future::Future;

use atm_core::api::RequestDeadline;
use tokio::time::Instant;

tokio::task_local! {
    static DEADLINE: Instant;
    static CORE_DEADLINE: RequestDeadline;
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct InjectedDeadline(pub Instant);

pub(crate) async fn ingress_at<F: Future>(
    timeout: std::time::Duration,
    deadline: Instant,
    future: F,
) -> F::Output {
    let core = RequestDeadline::after(timeout);
    CORE_DEADLINE.scope(core, scope(deadline, future)).await
}

pub(crate) fn core_at_ingress(timeout: std::time::Duration) -> RequestDeadline {
    CORE_DEADLINE
        .try_with(|value| *value)
        .unwrap_or_else(|_| RequestDeadline::after(timeout))
}

/// Capture the existing deadline, converting a core boundary only when this is
/// the entry point of an otherwise unscoped operation.
pub(crate) fn from_core(deadline: RequestDeadline) -> Instant {
    let now = Instant::now();
    deadline
        .remaining()
        .map_or(now, |remaining| now + remaining)
}

pub(crate) fn capture(deadline: RequestDeadline) -> Instant {
    DEADLINE
        .try_with(|value| *value)
        .unwrap_or_else(|_| from_core(deadline))
}

pub(crate) fn remaining(deadline: RequestDeadline) -> Option<std::time::Duration> {
    capture(deadline)
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
}

pub(crate) fn expired(deadline: RequestDeadline) -> bool {
    remaining(deadline).is_none()
}

pub(crate) fn current() -> Option<Instant> {
    DEADLINE.try_with(|value| *value).ok()
}

pub(crate) async fn scope_optional<F: Future>(deadline: Option<Instant>, future: F) -> F::Output {
    match deadline {
        Some(deadline) => scope(deadline, future).await,
        None => future.await,
    }
}

pub(crate) async fn scope<F: Future>(deadline: Instant, future: F) -> F::Output {
    DEADLINE.scope(deadline, future).await
}

pub(crate) async fn scope_if_absent<F: Future>(deadline: RequestDeadline, future: F) -> F::Output {
    if DEADLINE.try_with(|_| ()).is_ok() {
        future.await
    } else {
        scope(capture(deadline), future).await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use tokio::time::Instant;

    #[tokio::test(start_paused = true)]
    async fn already_expired_injected_deadline_is_rejected() {
        let now = Instant::now();
        let core = atm_core::api::RequestDeadline::after(Duration::ZERO);
        super::scope(now, async {
            assert!(super::expired(core));
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn nested_entry_preserves_deadline_through_exact_boundary() {
        let deadline = Instant::now() + Duration::from_secs(2);
        let core = atm_core::api::RequestDeadline::after(Duration::ZERO);
        super::scope(deadline, async {
            tokio::time::advance(Duration::from_secs(1)).await;
            super::scope_if_absent(core, async {
                assert_eq!(super::capture(core), deadline);
                tokio::time::advance(Duration::from_secs(1) - Duration::from_nanos(1)).await;
                assert!(!super::expired(core));
                tokio::time::advance(Duration::from_nanos(1)).await;
                assert!(super::expired(core));
            })
            .await;
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn captured_deadline_survives_a_spawn() {
        let deadline = Instant::now() + Duration::from_secs(2);
        super::scope(deadline, async {
            let captured = super::DEADLINE.with(|value| *value);
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(super::scope(captured, async {
                started_tx.send(()).unwrap();
                release_rx.await.unwrap();
                assert!(Instant::now() >= super::DEADLINE.with(|value| *value));
            }));
            started_rx.await.unwrap();
            tokio::time::advance(Duration::from_secs(2)).await;
            release_tx.send(()).unwrap();
            task.await.unwrap();
        })
        .await;
    }
}
