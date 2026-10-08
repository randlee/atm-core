//! BD6 cumulative shutdown deadline over a composed daemon with stalled
//! steps.
//!
//! Two steps stall: a local write held open by its received hook (listener
//! drain) and a peer write whose detached hook never finishes
//! (peer-connection drain). Task telemetry is queued behind a collector that
//! never answers, but the telemetry drains, logger and exporter still finish
//! promptly here, so this test proves only the listener-to-peer-drain
//! handoff. The clock stays real: the exporter step shuts the SDK providers
//! down in `spawn_blocking`, which stops a paused clock from advancing while
//! their exports wait on its timers. Exact virtual-time deadline tests of the
//! recovery sweep and the peer pool live beside those types.
#![cfg(test)]

use std::sync::Arc;
use std::time::Duration;

use atm_core::AuthenticatedIngress;
use atm_core::api::{ApiRequest, RequestDeadline};
use atm_core::boundary::{
    AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, MessageReceivedHookSelector,
    PostSendEmissionPath,
};
use atm_core::error::AtmError;
use atm_core::protocol::RequestEnvelope;
use atm_http_runtime::CanonicalWriteHandler;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::Instant;

use super::{Daemon, EXPORT_WAIT, Receiver, endpoint_env, task_record};
use crate::{LISTENER_ABORT_GRACE, REPLACEMENT_DRAIN_DEADLINE};

/// Scheduling allowance on a real clock; a renewed step budget adds at least
/// the 1s exporter or logger bound.
const SCHEDULING_SLACK: Duration = Duration::from_millis(250);

/// Reports each hook start, then never finishes.
struct StallingHook(UnboundedSender<()>);

impl atm_core::boundary::sealed::Sealed for StallingHook {}

impl AsyncMessageReceivedHookEmitter for StallingHook {
    fn emit_received_message(
        &self,
        _dispatch: BuiltInPostSendDispatch,
        _deadline: RequestDeadline,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>>
    {
        let _ = self.0.send(());
        Box::pin(std::future::pending())
    }
}

struct StallingSelector(StallingHook);

impl atm_core::boundary::sealed::Sealed for StallingSelector {}

impl MessageReceivedHookSelector for StallingSelector {
    fn select_emitter(
        &self,
        _dispatch: &BuiltInPostSendDispatch,
    ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
        Some(&self.0)
    }
}

async fn hook_started(started: &mut UnboundedReceiver<()>, what: &str) {
    tokio::time::timeout(Duration::from_secs(10), started.recv())
        .await
        .unwrap_or_else(|_| panic!("{what} hook never started"))
        .expect("stalling hook is alive");
}

/// Positive: with the listener and a detached peer hook stalled, the
/// production shutdown sequence returns within the one deadline plus the
/// listener abort grace.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_stalled_shutdown_step_shares_one_cumulative_deadline() {
    let stalled = Receiver::start(true).await;
    let (root, observability) = Daemon::bootstrap(endpoint_env(&stalled.endpoint)).await;
    let (started_tx, mut started) = unbounded_channel();
    let selector: Arc<dyn MessageReceivedHookSelector> =
        Arc::new(StallingSelector(StallingHook(started_tx)));
    let daemon = Daemon::compose_with_selector(root, observability, None, selector).await;

    for seq in 1..=64 {
        daemon.workers.task_telemetry.try_emit(task_record(seq));
    }
    stalled
        .capture
        .wait(EXPORT_WAIT, "a stalled export", || {
            stalled
                .capture
                .started
                .load(std::sync::atomic::Ordering::SeqCst)
                >= 1
        })
        .await;
    daemon
        .handler
        .write(
            daemon.request("sender", "recipient", "BD6-F1-PEER", None),
            AuthenticatedIngress::Peer,
            RequestDeadline::after(Duration::from_secs(10)),
        )
        .await
        .expect("a peer write is acknowledged before its detached hook");
    hook_started(&mut started, "detached peer").await;
    let client = Arc::clone(&daemon.client);
    let request = daemon.request("sender", "recipient", "BD6-F1-LOCAL", None);
    let in_flight = tokio::spawn(async move {
        client
            .execute(ApiRequest::new(RequestEnvelope::Write(Box::new(request))))
            .await
    });
    hook_started(&mut started, "in-flight local").await;

    let began = Instant::now();
    let stopped = daemon.shutdown().await;
    let elapsed = began.elapsed();

    let ceiling = REPLACEMENT_DRAIN_DEADLINE + LISTENER_ABORT_GRACE + SCHEDULING_SLACK;
    assert!(
        elapsed >= REPLACEMENT_DRAIN_DEADLINE && elapsed <= ceiling,
        "{elapsed:?} outside [{REPLACEMENT_DRAIN_DEADLINE:?}, {ceiling:?}]"
    );
    // The held request ends at its own server budget, inside the drain; the
    // detached hook then gets only what the listener left.
    stopped.expect("the listener drained the held request within its budget");
    assert!(
        in_flight.is_finished(),
        "the listener drain ended the held request"
    );
    stalled.stop().await;
}
