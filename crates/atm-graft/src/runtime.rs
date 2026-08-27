use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use std::sync::mpsc::TrySendError;

use atm_core::GraftConfig;
use atm_core::boundary::{
    BuiltInPostSendDispatch, GraftNudgeTarget, MessageReceivedHookEmitter, NudgeKind,
    PostSendBuiltInTarget,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::graft::{
    GRAFT_RECEIVER_ACCEPT_POLL_INTERVAL, GraftPostSendResponse, GraftReceiverListener,
};
use atm_core::protocol::LocalCapability;
use atm_core::protocol::OwnerGeneration;
use atm_core::types::{AgentName, ChatId, TeamName};

use crate::nudge_sink::GraftReceiveHook;
use crate::{
    GraftClient, GraftObservability, GraftSessionState, HostNudge, HostNudgeInjector,
    RECEIVE_LOOP_JOIN_DEADLINE, SessionSnapshot,
};

pub(crate) const RECEIVE_LOOP_READY_DEADLINE: Duration = Duration::from_secs(3);
const HOST_NUDGE_INJECTION_DEADLINE: Duration = Duration::from_millis(250);
const GRAFT_RECEIVER_IO_DEADLINE: Duration = Duration::from_secs(3);
/// A live listener checks its published record at this cadence so an external
/// record deletion is repaired without adding filesystem work to every poll.
const GRAFT_RECEIVER_RECORD_RECHECK_INTERVAL: Duration = Duration::from_secs(1);
pub(crate) const GRAFT_LEASE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
#[allow(dead_code)]
pub(crate) const ACTIVE_LEASE_WINDOW: Duration = Duration::from_secs(15);
/// Each recovery cycle makes a small, bounded number of bind attempts before
/// yielding to the slower re-arm cadence below.
const GRAFT_RECEIVER_REBIND_MAX_ATTEMPTS: usize = 3;
/// The first rebind delay is short enough for interactive recovery; later
/// attempts increase linearly so a persistently bad socket does not spin.
const GRAFT_RECEIVER_REBIND_INITIAL_DELAY: Duration = Duration::from_millis(100);
/// Cool down after a successful rebind before polling again, preventing a
/// repeated hard accept failure from becoming a tight rebind loop.
const GRAFT_RECEIVER_REBIND_CYCLE_DELAY: Duration = Duration::from_millis(100);
/// Wait between exhausted rebind cycles. The receiver remains armed and never
/// starts a daemon; it only retries its own loopback listener publication.
const GRAFT_RECEIVER_REARM_DELAY: Duration = Duration::from_secs(1);
/// Stop a failed recovery burst after this interval, emit a distinct degraded
/// signal, then enter a slower half-open retry cycle instead of retrying
/// indefinitely without escalation.
const GRAFT_RECEIVER_RECOVERY_MAX_DURATION: Duration = Duration::from_secs(30);
/// Give a persistently unavailable endpoint time to recover before a new
/// bounded recovery burst begins after the circuit opens.
const GRAFT_RECEIVER_RECOVERY_CIRCUIT_DELAY: Duration = Duration::from_secs(5);
/// Repeated receiver failures are initially reported after this delay, then
/// exponentially backed off to retain useful diagnostics without log storms.
const GRAFT_RECEIVER_RECOVERY_WARN_INITIAL_DELAY: Duration = Duration::from_secs(1);
/// Cap the warning backoff so a long-lived outage still leaves a periodic,
/// actionable diagnostic trail.
const GRAFT_RECEIVER_RECOVERY_WARN_MAX_DELAY: Duration = Duration::from_secs(30);
const MAX_HOST_NUDGE_HELPERS: usize = 8;

type ReceiveLoopJoinHelper = (
    Receiver<Result<(), AtmError>>,
    JoinHandle<()>,
    std::thread::ThreadId,
);

#[derive(Debug)]
struct ReceiverRecoveryCircuit {
    began_at: Instant,
    next_warning_at: Instant,
    warning_delay: Duration,
}

impl ReceiverRecoveryCircuit {
    fn new(now: Instant) -> Self {
        Self {
            began_at: now,
            next_warning_at: now + GRAFT_RECEIVER_RECOVERY_WARN_INITIAL_DELAY,
            warning_delay: GRAFT_RECEIVER_RECOVERY_WARN_INITIAL_DELAY,
        }
    }

    fn warning_due(&mut self, now: Instant) -> bool {
        if now < self.next_warning_at {
            return false;
        }
        self.next_warning_at = now + self.warning_delay;
        self.warning_delay = self
            .warning_delay
            .saturating_mul(2)
            .min(GRAFT_RECEIVER_RECOVERY_WARN_MAX_DELAY);
        true
    }

    fn is_exhausted(&self, now: Instant) -> bool {
        now.duration_since(self.began_at) >= GRAFT_RECEIVER_RECOVERY_MAX_DURATION
    }

    fn reset(&mut self, now: Instant) {
        *self = Self::new(now);
    }
}

#[derive(Debug)]
struct HelperThreadBudget {
    max_inflight: usize,
    inflight: Arc<AtomicUsize>,
}

impl HelperThreadBudget {
    fn new(max_inflight: usize) -> Self {
        Self {
            max_inflight,
            inflight: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn max_inflight(&self) -> usize {
        self.max_inflight
    }

    fn inflight(&self) -> usize {
        self.inflight.load(Ordering::SeqCst)
    }

    fn try_acquire(self: &Arc<Self>) -> Option<HelperThreadPermit> {
        let mut current = self.inflight();
        loop {
            if current >= self.max_inflight {
                return None;
            }
            match self.inflight.compare_exchange(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    return Some(HelperThreadPermit {
                        inflight: Arc::clone(&self.inflight),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }
}

struct HelperThreadPermit {
    inflight: Arc<AtomicUsize>,
}

impl Drop for HelperThreadPermit {
    fn drop(&mut self) {
        self.inflight.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) struct ReceiverReadyLatch {
    ready_tx: SyncSender<()>,
    ready_rx: Receiver<()>,
}

impl ReceiverReadyLatch {
    pub(crate) fn new() -> Self {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        Self { ready_tx, ready_rx }
    }

    pub(crate) fn notifier(&self) -> SyncSender<()> {
        self.ready_tx.clone()
    }

    #[cfg(test)]
    pub(crate) fn signal_listening(&self) -> Result<(), AtmError> {
        signal_ready_sender(&self.ready_tx)
    }

    pub(crate) fn wait_until_listening(
        &self,
        timeout: std::time::Duration,
    ) -> Result<(), AtmError> {
        match self.ready_rx.recv_timeout(timeout) {
            Ok(()) => Ok(()),
            Err(RecvTimeoutError::Timeout) => Err(AtmError::new(
                AtmErrorCode::WaitTimeout,
                format!(
                    "graft receiver readiness was not signaled within {:?}",
                    timeout
                ),
            )),
            Err(RecvTimeoutError::Disconnected) => Err(AtmError::new(
                AtmErrorCode::InternalError,
                "graft receiver readiness latch disconnected before signaling startup",
            )),
        }
    }
}

fn signal_ready_sender(ready_tx: &SyncSender<()>) -> Result<(), AtmError> {
    ready_tx.try_send(()).map_err(|error| match error {
        TrySendError::Full(()) => AtmError::new(
            AtmErrorCode::InternalError,
            "graft receiver readiness was signaled more than once",
        ),
        TrySendError::Disconnected(()) => AtmError::new(
            AtmErrorCode::InternalError,
            "graft receiver readiness latch is unavailable",
        ),
    })
}

struct BoundedHostNudgeInjector {
    injector: Arc<dyn HostNudgeInjector>,
    helper_budget: Arc<HelperThreadBudget>,
}

impl crate::HostNudgeInjector for BoundedHostNudgeInjector {
    fn inject_nudge(&self, nudge: &HostNudge) -> Result<(), AtmError> {
        let helper_permit = acquire_host_nudge_helper_permit(&self.helper_budget)?;
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        spawn_host_nudge_helper(
            Arc::clone(&self.injector),
            nudge.clone(),
            helper_permit,
            result_tx,
        )?;
        receive_host_nudge_result(result_rx, &self.helper_budget)
    }
}

impl BoundedHostNudgeInjector {
    fn spawn(injector: Arc<dyn HostNudgeInjector>) -> Self {
        Self {
            injector,
            helper_budget: Arc::new(HelperThreadBudget::new(MAX_HOST_NUDGE_HELPERS)),
        }
    }
}

fn acquire_host_nudge_helper_permit(
    helper_budget: &Arc<HelperThreadBudget>,
) -> Result<HelperThreadPermit, AtmError> {
    helper_budget.try_acquire().ok_or_else(|| {
        let error = AtmError::new(
            AtmErrorCode::WaitTimeout,
            format!(
                "graft host nudge helper budget is exhausted at {} in-flight helpers",
                helper_budget.max_inflight()
            ),
        );
        warn_host_nudge_result("helper_budget_exhausted", &error, helper_budget, None);
        error
    })
}

fn spawn_host_nudge_helper(
    injector: Arc<dyn HostNudgeInjector>,
    nudge: HostNudge,
    helper_permit: HelperThreadPermit,
    result_tx: SyncSender<Result<(), AtmError>>,
) -> Result<(), AtmError> {
    thread::Builder::new()
        .name("atm-graft-host-nudge".to_string())
        .spawn(move || {
            let _helper_permit = helper_permit;
            let result = injector.inject_nudge(&nudge);
            if result_tx.send(result).is_err() {
                tracing::debug!(
                    timeout_ms = HOST_NUDGE_INJECTION_DEADLINE.as_millis(),
                    "graft host nudge helper dropped its result because the bounded caller already timed out"
                );
            }
        })
        .map(|_| ())
        .map_err(|_source| {
            AtmError::new(
                AtmErrorCode::InternalError,
                "failed to spawn graft host nudge helper",
            )
        })
}

fn receive_host_nudge_result(
    result_rx: Receiver<Result<(), AtmError>>,
    helper_budget: &Arc<HelperThreadBudget>,
) -> Result<(), AtmError> {
    match result_rx.recv_timeout(HOST_NUDGE_INJECTION_DEADLINE) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => {
            let error = AtmError::new(
                AtmErrorCode::WaitTimeout,
                format!(
                    "graft host nudge injection exceeded the {:?} delivery deadline",
                    HOST_NUDGE_INJECTION_DEADLINE
                ),
            );
            warn_host_nudge_result(
                "timeout",
                &error,
                helper_budget,
                Some(HOST_NUDGE_INJECTION_DEADLINE.as_millis()),
            );
            Err(error)
        }
        Err(RecvTimeoutError::Disconnected) => {
            let error = AtmError::new(
                AtmErrorCode::InternalError,
                "graft host nudge helper disconnected before returning a delivery result",
            );
            warn_host_nudge_result("disconnected", &error, helper_budget, None);
            Err(error)
        }
    }
}

fn warn_host_nudge_result(
    outcome: &'static str,
    error: &AtmError,
    helper_budget: &Arc<HelperThreadBudget>,
    timeout_ms: Option<u128>,
) {
    tracing::warn!(
        subsystem = "atm_graft.host_nudge",
        action = "inject_nudge",
        outcome,
        timeout_ms,
        helper_budget_max = helper_budget.max_inflight(),
        helper_budget_inflight = helper_budget.inflight(),
        error_code = %error.code(),
        error_message = %error.message(),
        "graft host nudge helper error"
    );
}

pub(crate) fn load_graft_config(workspace_root: &Path) -> Result<Option<GraftConfig>, AtmError> {
    let config = atm_core::load_atm_config(workspace_root)?;
    Ok(config.map(|config| config.graft))
}

// Shared snapshot access is split across the session owner, receive loop, and
// observability callbacks. Reads dominate writes and each reader clones the
// snapshot immediately, so `Arc<RwLock<_>>` keeps mutation simple without
// holding a lock across cross-boundary calls.
type SharedSessionSnapshot = Arc<RwLock<SessionSnapshot>>;

pub(crate) fn read_snapshot(snapshot: &SharedSessionSnapshot) -> Result<SessionSnapshot, AtmError> {
    snapshot
        .read()
        .map(|snapshot| snapshot.clone())
        .map_err(|_| AtmError::daemon_unavailable("graft session snapshot lock poisoned"))
}

fn write_snapshot(
    snapshot: &SharedSessionSnapshot,
    state: GraftSessionState,
) -> Result<(), AtmError> {
    let mut snapshot = snapshot
        .write()
        .map_err(|_| AtmError::daemon_unavailable("graft session snapshot lock poisoned"))?;
    snapshot.state = state;
    Ok(())
}

pub(crate) fn set_session_state(
    snapshot: &SharedSessionSnapshot,
    state: GraftSessionState,
    observability: &dyn GraftObservability,
) -> Result<(), AtmError> {
    write_snapshot(snapshot, state)?;
    observability.session_state_changed(&read_snapshot(snapshot)?);
    Ok(())
}

pub(crate) fn join_receive_loop_with_deadline(
    join_handle: JoinHandle<Result<(), AtmError>>,
) -> Result<(), AtmError> {
    let (result_rx, join_helper, join_helper_thread_id) =
        spawn_receive_loop_join_helper(join_handle)?;
    match result_rx.recv_timeout(RECEIVE_LOOP_JOIN_DEADLINE) {
        Ok(result) => finish_join_receive_loop(join_helper, result),
        Err(RecvTimeoutError::Timeout) => {
            Err(join_receive_loop_timeout_error(join_helper_thread_id))
        }
        Err(RecvTimeoutError::Disconnected) => handle_join_helper_disconnect(join_helper),
    }
}

fn spawn_receive_loop_join_helper(
    join_handle: JoinHandle<Result<(), AtmError>>,
) -> Result<ReceiveLoopJoinHelper, AtmError> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let join_helper = thread::Builder::new()
        .name("atm-graft-receive-loop-join".to_string())
        .spawn(move || {
            let result = join_handle
                .join()
                .unwrap_or_else(|_| Err(receive_loop_panic_error()));
            let _ = result_tx.send(result);
        })
        .map_err(join_helper_spawn_error)?;
    let join_helper_thread_id = join_helper.thread().id();
    Ok((result_rx, join_helper, join_helper_thread_id))
}

fn finish_join_receive_loop(
    join_helper: JoinHandle<()>,
    result: Result<(), AtmError>,
) -> Result<(), AtmError> {
    if join_helper.join().is_err() {
        let error = join_helper_panic_error();
        warn_runtime_error("finish_join_receive_loop", None, &error);
        return Err(error);
    }
    result
}

fn handle_join_helper_disconnect(join_helper: JoinHandle<()>) -> Result<(), AtmError> {
    join_helper.join().map_or_else(
        |_| {
            let error = join_helper_panic_error();
            warn_runtime_error("handle_join_helper_disconnect", None, &error);
            Err(error)
        },
        |_| {
            let error = join_helper_disconnect_error();
            warn_runtime_error("handle_join_helper_disconnect", None, &error);
            Err(error)
        },
    )
}

fn receive_loop_panic_error() -> AtmError {
    AtmError::daemon_unavailable("graft receiver loop panicked")
}

fn join_helper_spawn_error(_source: std::io::Error) -> AtmError {
    AtmError::daemon_unavailable("failed to spawn graft receive-loop join helper")
}

fn join_helper_panic_error() -> AtmError {
    AtmError::daemon_unavailable("graft receive-loop join helper panicked")
}

fn join_helper_disconnect_error() -> AtmError {
    AtmError::daemon_unavailable("graft receive-loop join helper disconnected unexpectedly")
}

fn join_receive_loop_timeout_error(join_helper_thread_id: std::thread::ThreadId) -> AtmError {
    tracing::debug!(
        timeout_ms = RECEIVE_LOOP_JOIN_DEADLINE.as_millis(),
        thread_id = ?join_helper_thread_id,
        "graft receive-loop join timed out; helper left detached after deadline"
    );
    AtmError::daemon_unavailable(format!(
        "graft receive loop shutdown exceeded the {:?} join deadline",
        RECEIVE_LOOP_JOIN_DEADLINE
    ))
}

fn warn_runtime_error(action: &'static str, endpoint_path: Option<&Path>, error: &AtmError) {
    match endpoint_path {
        Some(endpoint_path) => tracing::warn!(
            subsystem = "atm_graft.receiver_loop",
            action,
            outcome = "error",
            endpoint = %endpoint_path.display(),
            error_code = %error.code(),
            error_message = %error.message(),
            "graft receiver runtime error"
        ),
        None => tracing::warn!(
            subsystem = "atm_graft.receiver_loop",
            action,
            outcome = "error",
            error_code = %error.code(),
            error_message = %error.message(),
            "graft receiver runtime error"
        ),
    }
}

pub(crate) struct GraftReceiverLoopContext {
    pub(crate) graft_root: PathBuf,
    pub(crate) team: TeamName,
    pub(crate) agent: AgentName,
    pub(crate) owner_chat_id: Option<ChatId>,
    pub(crate) client: Option<GraftClient>,
    pub(crate) snapshot: SharedSessionSnapshot,
    pub(crate) injector: Arc<dyn HostNudgeInjector>,
    pub(crate) observability: Arc<dyn GraftObservability>,
    pub(crate) stop_rx: Receiver<()>,
    pub(crate) ready_tx: Option<SyncSender<()>>,
    pub(crate) receiver_target_tx: Option<SyncSender<(SocketAddr, LocalCapability)>>,
}

/// The listener's owner generation is a freshly minted ULID (see
/// `GraftReceiverListener::bind`), so this conversion into the storage-layer
/// validated newtype can never fail in practice.
fn owner_generation(listener: &GraftReceiverListener) -> OwnerGeneration {
    OwnerGeneration::new(listener.owner_generation())
        .expect("graft receiver listener owner generation is always a valid ULID")
}

/// Owns a listener together with its best-effort daemon lease lifecycle.
struct RegisteredGraftReceiver {
    listener: GraftReceiverListener,
    team: TeamName,
    agent: AgentName,
    client: Option<GraftClient>,
}

impl RegisteredGraftReceiver {
    fn new(listener: GraftReceiverListener, ctx: &GraftReceiverLoopContext) -> Self {
        Self {
            listener,
            team: ctx.team.clone(),
            agent: ctx.agent.clone(),
            client: ctx.client.clone(),
        }
    }

    fn refresh(&mut self) -> Result<(), AtmError> {
        let client = match self.client.clone() {
            Some(client) => client,
            None => GraftClient::connect_existing()?,
        };
        let result = client.register_receiver_sync(
            self.team.clone(),
            self.agent.clone(),
            self.listener.local_addr()?,
            self.listener.capability().clone(),
            owner_generation(&self.listener),
        );
        self.client = result.as_ref().ok().map(|_| client);
        result
    }
}

impl Drop for RegisteredGraftReceiver {
    fn drop(&mut self) {
        let Some(client) = self.client.take() else {
            return;
        };
        if let Err(error) = client.unregister_receiver_sync(
            self.team.clone(),
            self.agent.clone(),
            owner_generation(&self.listener),
        ) {
            tracing::debug!(
                subsystem = "atm_graft.receiver_loop",
                action = "unregister_graft_receiver",
                outcome = "best_effort_failure",
                error_code = %error.code(),
                error_message = %error.message(),
                "graft receiver lease unregister did not complete"
            );
        }
    }
}

pub(crate) fn run_graft_receiver_loop(ctx: GraftReceiverLoopContext) -> Result<(), AtmError> {
    let injector = BoundedHostNudgeInjector::spawn(Arc::clone(&ctx.injector));
    let result = listen_for_graft_nudges(&ctx, &injector);
    let terminal_state = if result.is_ok() {
        GraftSessionState::Closed
    } else {
        GraftSessionState::Degraded
    };
    if let Err(state_error) =
        set_session_state(&ctx.snapshot, terminal_state, ctx.observability.as_ref())
    {
        if result.is_ok() {
            return Err(state_error);
        }
        warn_runtime_error("set_session_state", Some(&ctx.graft_root), &state_error);
    }
    result
}

fn listen_for_graft_nudges(
    ctx: &GraftReceiverLoopContext,
    injector: &BoundedHostNudgeInjector,
) -> Result<(), AtmError> {
    let mut listener = activate_graft_receiver(ctx)?;
    publish_receiver_target(ctx, &listener.listener);
    if let Some(ready_tx) = ctx.ready_tx.as_ref() {
        signal_ready_sender(ready_tx)?;
    }
    // Non-blocking accept + poll: the loop re-checks its stop signal every
    // ACCEPT_POLL_INTERVAL instead of parking in a blocking accept, so no
    // wake-by-connect machinery is needed to unblock shutdown.
    let mut last_record_recheck = Instant::now();
    let mut last_lease_refresh = Instant::now();
    loop {
        if stop_requested(&ctx.stop_rx) {
            return Ok(());
        }
        if last_lease_refresh.elapsed() >= GRAFT_LEASE_REFRESH_INTERVAL {
            last_lease_refresh = Instant::now();
            refresh_receiver_lease(ctx, &mut listener);
        }
        if last_record_recheck.elapsed() >= GRAFT_RECEIVER_RECORD_RECHECK_INTERVAL {
            last_record_recheck = Instant::now();
            republish_receiver_record(ctx, &listener.listener);
        }
        match listener.listener.poll_accept() {
            Ok(Some(mut stream)) => {
                if stop_requested(&ctx.stop_rx) {
                    return Ok(());
                }
                if let Err(error) =
                    handle_graft_receiver_connection(ctx, injector, &listener.listener, &mut stream)
                {
                    warn_runtime_error(
                        "handle_graft_receiver_connection",
                        Some(&ctx.graft_root),
                        &error,
                    );
                }
            }
            Ok(None) => thread::sleep(GRAFT_RECEIVER_ACCEPT_POLL_INTERVAL),
            Err(error) => {
                match recover_after_poll_accept_error(ctx, listener, &error)? {
                    Some(rebound_listener) => listener = rebound_listener,
                    None => return Ok(()),
                }
                last_record_recheck = Instant::now();
            }
        }
    }
}

fn refresh_receiver_lease(ctx: &GraftReceiverLoopContext, listener: &mut RegisteredGraftReceiver) {
    if let Err(error) = listener.refresh() {
        warn_runtime_error("refresh_graft_receiver", Some(&ctx.graft_root), &error);
        if let Ok(snapshot) = read_snapshot(&ctx.snapshot) {
            ctx.observability
                .receiver_ownership(&snapshot, "refresh_receiver_lease", "error");
        }
    } else if let Ok(snapshot) = read_snapshot(&ctx.snapshot) {
        ctx.observability
            .receiver_ownership(&snapshot, "refresh_receiver_lease", "ok");
    }
}

fn republish_receiver_record(ctx: &GraftReceiverLoopContext, listener: &GraftReceiverListener) {
    match listener.republish_if_missing() {
        Ok(true) => {
            if let Ok(snapshot) = read_snapshot(&ctx.snapshot) {
                ctx.observability
                    .receiver_ownership(&snapshot, "restore_receiver_record", "ok");
            }
        }
        Ok(false) => {}
        Err(error) => {
            warn_runtime_error("restore_receiver_record", Some(&ctx.graft_root), &error);
            if let Ok(snapshot) = read_snapshot(&ctx.snapshot) {
                ctx.observability
                    .receiver_ownership(&snapshot, "restore_receiver_record", "error");
            }
        }
    }
}

fn publish_receiver_target(ctx: &GraftReceiverLoopContext, listener: &GraftReceiverListener) {
    if let Some(target_tx) = &ctx.receiver_target_tx
        && let Ok(endpoint) = listener.local_addr()
    {
        let _ = target_tx.send((endpoint, listener.capability().clone()));
    }
}

fn activate_graft_receiver(
    ctx: &GraftReceiverLoopContext,
) -> Result<RegisteredGraftReceiver, AtmError> {
    match GraftReceiverListener::bind(
        &ctx.graft_root,
        &ctx.team,
        &ctx.agent,
        ctx.owner_chat_id.clone(),
    ) {
        Ok(listener) => {
            let mut receiver = RegisteredGraftReceiver::new(listener, ctx);
            if let Err(error) = receiver.refresh() {
                warn_runtime_error("register_graft_receiver", Some(&ctx.graft_root), &error);
            }
            let snapshot = read_snapshot(&ctx.snapshot)?;
            ctx.observability
                .receiver_ownership(&snapshot, "activate_receiver_owner", "ok");
            Ok(receiver)
        }
        Err(error) => {
            let snapshot = read_snapshot(&ctx.snapshot)?;
            let outcome = if error.code() == AtmErrorCode::GraftReceiverAlreadyActive {
                "conflict"
            } else {
                "error"
            };
            ctx.observability
                .receiver_ownership(&snapshot, "activate_receiver_owner", outcome);
            Err(error)
        }
    }
}

fn recover_after_poll_accept_error(
    ctx: &GraftReceiverLoopContext,
    listener: RegisteredGraftReceiver,
    error: &AtmError,
) -> Result<Option<RegisteredGraftReceiver>, AtmError> {
    warn_runtime_error("poll_graft_receiver", Some(&ctx.graft_root), error);
    drop(listener);
    recover_graft_receiver(ctx)
}

fn recover_graft_receiver(
    ctx: &GraftReceiverLoopContext,
) -> Result<Option<RegisteredGraftReceiver>, AtmError> {
    set_session_state(
        &ctx.snapshot,
        GraftSessionState::Degraded,
        ctx.observability.as_ref(),
    )?;
    let mut recovery_circuit = ReceiverRecoveryCircuit::new(Instant::now());
    loop {
        if stop_requested(&ctx.stop_rx) {
            return Ok(None);
        }
        match rebind_graft_receiver(ctx) {
            Ok(Some(listener)) => {
                set_session_state(
                    &ctx.snapshot,
                    GraftSessionState::Listening,
                    ctx.observability.as_ref(),
                )?;
                if wait_for_stop_or_delay(&ctx.stop_rx, GRAFT_RECEIVER_REBIND_CYCLE_DELAY) {
                    return Ok(None);
                }
                return Ok(Some(listener));
            }
            Ok(None) => return Ok(None),
            Err(error) => {
                let now = Instant::now();
                let snapshot = read_snapshot(&ctx.snapshot)?;
                if recovery_circuit.warning_due(now) {
                    warn_runtime_error("rearm_graft_receiver", Some(&ctx.graft_root), &error);
                }
                if recovery_circuit.is_exhausted(now) {
                    ctx.observability.receiver_ownership(
                        &snapshot,
                        "rebind_receiver_owner",
                        "circuit_open",
                    );
                    ctx.observability.session_error(
                        &snapshot,
                        "graft_receiver_recovery_circuit_open",
                        &error,
                    );
                    warn_runtime_error(
                        "graft_receiver_recovery_circuit_open",
                        Some(&ctx.graft_root),
                        &error,
                    );
                    if wait_for_stop_or_delay(&ctx.stop_rx, GRAFT_RECEIVER_RECOVERY_CIRCUIT_DELAY) {
                        return Ok(None);
                    }
                    recovery_circuit.reset(Instant::now());
                } else {
                    ctx.observability.receiver_ownership(
                        &snapshot,
                        "rebind_receiver_owner",
                        "retry",
                    );
                    if wait_for_stop_or_delay(&ctx.stop_rx, GRAFT_RECEIVER_REARM_DELAY) {
                        return Ok(None);
                    }
                }
            }
        }
    }
}

fn rebind_graft_receiver(
    ctx: &GraftReceiverLoopContext,
) -> Result<Option<RegisteredGraftReceiver>, AtmError> {
    let mut last_error = None;
    for attempt in 1..=GRAFT_RECEIVER_REBIND_MAX_ATTEMPTS {
        match GraftReceiverListener::bind(
            &ctx.graft_root,
            &ctx.team,
            &ctx.agent,
            ctx.owner_chat_id.clone(),
        ) {
            Ok(listener) => {
                let mut receiver = RegisteredGraftReceiver::new(listener, ctx);
                publish_receiver_target(ctx, &receiver.listener);
                if let Err(error) = receiver.refresh() {
                    warn_runtime_error("register_graft_receiver", Some(&ctx.graft_root), &error);
                }
                let snapshot = read_snapshot(&ctx.snapshot)?;
                ctx.observability
                    .receiver_ownership(&snapshot, "rebind_receiver_owner", "ok");
                return Ok(Some(receiver));
            }
            Err(error) => {
                last_error = Some(error);
                if attempt < GRAFT_RECEIVER_REBIND_MAX_ATTEMPTS {
                    let delay = GRAFT_RECEIVER_REBIND_INITIAL_DELAY.saturating_mul(attempt as u32);
                    if wait_for_stop_or_delay(&ctx.stop_rx, delay) {
                        return Ok(None);
                    }
                }
            }
        }
    }
    let snapshot = read_snapshot(&ctx.snapshot)?;
    ctx.observability
        .receiver_ownership(&snapshot, "rebind_receiver_owner", "error");
    Err(last_error.expect("at least one graft receiver rebind attempt"))
}

fn wait_for_stop_or_delay(stop_rx: &Receiver<()>, delay: Duration) -> bool {
    matches!(
        stop_rx.recv_timeout(delay),
        Ok(()) | Err(RecvTimeoutError::Disconnected)
    )
}

fn stop_requested(stop_rx: &Receiver<()>) -> bool {
    matches!(
        stop_rx.try_recv(),
        Ok(()) | Err(std::sync::mpsc::TryRecvError::Disconnected)
    )
}

fn handle_graft_receiver_connection(
    ctx: &GraftReceiverLoopContext,
    injector: &BoundedHostNudgeInjector,
    listener: &GraftReceiverListener,
    stream: &mut TcpStream,
) -> Result<(), AtmError> {
    let request = listener.read_request(stream, GRAFT_RECEIVER_IO_DEADLINE)?;
    let event = request.event;
    let rendered_nudge = request.rendered_nudge;
    let message_body = request.message_body;
    let dispatch = BuiltInPostSendDispatch {
        target: PostSendBuiltInTarget::Graft(GraftNudgeTarget {
            recipient: event.recipient.clone(),
            recipient_team: event.recipient_team.clone(),
            rendered_nudge,
            message_body,
        }),
        event,
        kind: NudgeKind::Steer,
    };
    let response = match (GraftReceiveHook {
        injector,
        snapshot: &ctx.snapshot,
        observability: ctx.observability.as_ref(),
    })
    .emit_received_message(
        &dispatch,
        atm_core::RequestDeadline::after(GRAFT_RECEIVER_IO_DEADLINE),
    ) {
        Ok(_) => GraftPostSendResponse::Delivered,
        Err(error) => GraftPostSendResponse::Error(error),
    };
    listener.write_response(stream, &response)
}

#[cfg(test)]
mod tests {
    use atm_core::boundary::PostSendHookEvent;
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::graft::{
        GRAFT_RECEIVER_ACCEPT_POLL_INTERVAL, GraftPostSendRequest, GraftPostSendResponse,
        GraftReceiverListener, deliver_graft_post_send,
    };
    use atm_core::protocol::LocalCapability;
    use atm_core::schema::AtmMessageId;
    use atm_core::test_support::{TEST_LEAD, TEST_QA, TEST_TEAM};
    use atm_core::types::{AgentName, ChatId, TeamName};
    use std::fs;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Barrier, Mutex, RwLock};
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    use crate::{GraftObservability, HostNudge, HostNudgeInjector};

    use super::{
        BoundedHostNudgeInjector, GRAFT_RECEIVER_RECOVERY_MAX_DURATION,
        GRAFT_RECEIVER_RECOVERY_WARN_INITIAL_DELAY, GraftReceiverLoopContext, HelperThreadBudget,
        MAX_HOST_NUDGE_HELPERS, RECEIVE_LOOP_READY_DEADLINE, ReceiverReadyLatch,
        ReceiverRecoveryCircuit, RegisteredGraftReceiver, handle_graft_receiver_connection,
        join_receive_loop_with_deadline, load_graft_config, read_snapshot,
        recover_after_poll_accept_error, recover_graft_receiver, run_graft_receiver_loop,
        wait_for_stop_or_delay,
    };
    use crate::{GraftSessionState, SessionSnapshot};

    const DELIVER_CONNECT_DEADLINE: Duration = Duration::from_secs(2);
    const DELIVER_IO_DEADLINE: Duration = Duration::from_secs(3);

    #[derive(Debug, Default)]
    struct RecordingInjector {
        nudges: Mutex<Vec<HostNudge>>,
    }

    impl HostNudgeInjector for RecordingInjector {
        fn inject_nudge(&self, nudge: &HostNudge) -> Result<(), AtmError> {
            self.nudges.lock().expect("nudges lock").push(nudge.clone());
            Ok(())
        }
    }

    #[derive(Debug)]
    struct FailingInjector;

    impl HostNudgeInjector for FailingInjector {
        fn inject_nudge(&self, _nudge: &HostNudge) -> Result<(), AtmError> {
            Err(AtmError::for_code(AtmErrorCode::PostSendGraftUnavailable))
        }
    }

    #[derive(Debug, Default)]
    struct NoopObservability;

    impl GraftObservability for NoopObservability {}

    struct StateObservability {
        states: mpsc::SyncSender<GraftSessionState>,
    }

    impl GraftObservability for StateObservability {
        fn session_state_changed(&self, snapshot: &SessionSnapshot) {
            let _ = self.states.try_send(snapshot.state);
        }
    }

    #[derive(Debug)]
    struct FirstCallBlocksInjector {
        first_call_gate: Mutex<Option<mpsc::Receiver<()>>>,
        call_count: AtomicUsize,
    }

    impl HostNudgeInjector for FirstCallBlocksInjector {
        fn inject_nudge(&self, _nudge: &HostNudge) -> Result<(), AtmError> {
            let call_index = self.call_count.fetch_add(1, Ordering::SeqCst);
            if call_index == 0 {
                let gate = self
                    .first_call_gate
                    .lock()
                    .expect("first_call_gate lock")
                    .take()
                    .expect("first call gate");
                gate.recv().expect("release first call");
            }
            Ok(())
        }
    }

    #[derive(Debug)]
    struct AlwaysBlocksInjector {
        released: Arc<std::sync::atomic::AtomicBool>,
        call_count: AtomicUsize,
    }

    impl HostNudgeInjector for AlwaysBlocksInjector {
        fn inject_nudge(&self, _nudge: &HostNudge) -> Result<(), AtmError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            while !self.released.load(Ordering::SeqCst) {
                std::thread::yield_now();
            }
            Ok(())
        }
    }

    struct TestPaths {
        _tempdir: TempDir,
        workspace_root: PathBuf,
    }

    type SpawnedReceiver = (
        std::sync::mpsc::Sender<()>,
        std::thread::JoinHandle<Result<(), AtmError>>,
        Arc<RwLock<SessionSnapshot>>,
        (SocketAddr, LocalCapability),
    );

    fn test_paths() -> TestPaths {
        let tempdir = TempDir::new().expect("tempdir");
        let workspace_root = tempdir.path().join("workspace");
        fs::create_dir_all(&workspace_root).expect("create workspace dir");
        TestPaths {
            _tempdir: tempdir,
            workspace_root,
        }
    }

    fn receiver_record_path(paths: &TestPaths) -> PathBuf {
        paths
            .workspace_root
            .join(".atm")
            .join("graft")
            .join(TEST_TEAM)
            .join(format!("{TEST_QA}.json"))
    }

    fn bind_receiver(
        paths: &TestPaths,
        owner_chat_id: Option<ChatId>,
    ) -> Result<GraftReceiverListener, AtmError> {
        GraftReceiverListener::bind(
            &paths.workspace_root,
            &TeamName::from_validated(TEST_TEAM),
            &AgentName::from_validated(TEST_QA),
            owner_chat_id,
        )
    }

    fn deliver_request(
        endpoint: SocketAddr,
        capability: &LocalCapability,
        event: PostSendHookEvent,
    ) -> GraftPostSendResponse {
        deliver_graft_post_send(
            endpoint,
            capability,
            &GraftPostSendRequest {
                event,
                rendered_nudge: "<atm>test nudge</atm>".to_string(),
                message_body: "full immutable body".to_string(),
            },
            DELIVER_CONNECT_DEADLINE,
            DELIVER_IO_DEADLINE,
        )
        .expect("deliver graft post-send")
    }

    fn request_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: AgentName::from_validated(TEST_LEAD),
            sender_chat_id: None,
            sender_team: TeamName::from_validated(TEST_TEAM),
            sender_host: None,
            recipient: AgentName::from_validated(TEST_QA),
            recipient_team: TeamName::from_validated(TEST_TEAM),
            message_id: AtmMessageId::new(),
            description: "review failing smoke lane".to_string(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            recipient_pane_id: None,
        }
    }

    fn request_nudge() -> HostNudge {
        let event = request_event();
        HostNudge {
            body: event.description.clone(),
            notice_text: format!("📬 from {}\n{}", event.source_address(), event.description),
            event,
        }
    }

    fn spawn_receiver(
        graft_root: PathBuf,
        injector: Arc<dyn HostNudgeInjector>,
    ) -> SpawnedReceiver {
        let (stop_tx, stop_rx) = mpsc::channel();
        let ready_latch = ReceiverReadyLatch::new();
        let (target_tx, target_rx) = mpsc::sync_channel(1);
        let snapshot = Arc::new(RwLock::new(SessionSnapshot {
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_QA),
            state: GraftSessionState::Listening,
        }));
        let ctx = GraftReceiverLoopContext {
            graft_root,
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_QA),
            owner_chat_id: None,
            client: None,
            snapshot: Arc::clone(&snapshot),
            injector,
            observability: Arc::new(NoopObservability),
            stop_rx,
            ready_tx: Some(ready_latch.notifier()),
            receiver_target_tx: Some(target_tx),
        };
        let join = std::thread::spawn(move || run_graft_receiver_loop(ctx));
        ready_latch
            .wait_until_listening(RECEIVE_LOOP_READY_DEADLINE)
            .expect("receiver ready");
        let target = target_rx
            .recv_timeout(RECEIVE_LOOP_READY_DEADLINE)
            .expect("receiver target");
        (stop_tx, join, snapshot, target)
    }

    fn stop_receiver(
        stop_tx: std::sync::mpsc::Sender<()>,
        join: std::thread::JoinHandle<Result<(), AtmError>>,
    ) {
        // The non-blocking accept loop observes the stop signal within one poll
        // interval, so no wake-by-connect is required to unblock shutdown.
        stop_tx.send(()).expect("stop");
        join_receive_loop_with_deadline(join).expect("join receiver");
    }

    #[test]
    fn load_config_reads_graft_enabled_and_defaults() {
        let tempdir = TempDir::new().expect("tempdir");
        fs::write(
            tempdir.path().join(".atm.toml"),
            "[atm.graft]\nenabled = true\n",
        )
        .expect("write config");
        assert!(
            load_graft_config(tempdir.path())
                .expect("graft config")
                .expect("config")
                .enabled
        );
    }

    #[test]
    fn receiver_ready_latch_signals_and_waits() {
        let latch = ReceiverReadyLatch::new();
        latch.signal_listening().expect("signal");
        latch
            .wait_until_listening(RECEIVE_LOOP_READY_DEADLINE)
            .expect("wait");
    }

    #[test]
    fn receiver_recovery_delays_observe_stop_without_waiting_for_backoff() {
        let (stop_tx, stop_rx) = mpsc::channel();
        stop_tx.send(()).expect("request stop");
        assert!(wait_for_stop_or_delay(&stop_rx, Duration::from_secs(1)));
    }

    #[test]
    fn receiver_listener_binds_at_expected_endpoint() {
        let paths = test_paths();
        let endpoint_path = receiver_record_path(&paths);
        let listener = bind_receiver(&paths, None).expect("bind listener");
        assert!(
            endpoint_path.exists(),
            "endpoint record should be published"
        );
        drop(listener);
        assert!(
            !endpoint_path.exists(),
            "endpoint record should be removed on drop"
        );
    }

    #[test]
    fn bounded_host_nudge_injector_timeout_does_not_wedge_future_delivery() {
        let (gate_tx, gate_rx) = mpsc::channel();
        let injector = BoundedHostNudgeInjector::spawn(Arc::new(FirstCallBlocksInjector {
            first_call_gate: Mutex::new(Some(gate_rx)),
            call_count: AtomicUsize::new(0),
        }) as Arc<dyn HostNudgeInjector>);

        let first_error = injector
            .inject_nudge(&request_nudge())
            .expect_err("first delivery should time out");
        assert_eq!(first_error.code(), AtmErrorCode::WaitTimeout);

        injector
            .inject_nudge(&request_nudge())
            .expect("second delivery should use a fresh helper thread");

        gate_tx.send(()).expect("release blocked first helper");
    }

    #[test]
    fn helper_budget_failed_acquire_leaves_inflight_unchanged() {
        let budget = Arc::new(HelperThreadBudget::new(0));
        assert!(budget.try_acquire().is_none());
        assert_eq!(budget.inflight(), 0);
    }

    #[test]
    fn helper_budget_concurrent_acquires_never_exceed_limit() {
        let budget = Arc::new(HelperThreadBudget::new(MAX_HOST_NUDGE_HELPERS));
        let workers = MAX_HOST_NUDGE_HELPERS * 4;
        let start = Arc::new(Barrier::new(workers + 1));
        let release = Arc::new(Barrier::new(workers + 1));
        let (result_tx, result_rx) = mpsc::channel();
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let budget = Arc::clone(&budget);
            let start = Arc::clone(&start);
            let release = Arc::clone(&release);
            let result_tx = result_tx.clone();
            handles.push(std::thread::spawn(move || {
                start.wait();
                let permit = budget.try_acquire();
                result_tx
                    .send(permit.is_some())
                    .expect("send acquire result");
                release.wait();
                drop(permit);
            }));
        }
        start.wait();
        drop(result_tx);
        let acquired = (0..workers)
            .map(|_| result_rx.recv().expect("receive acquire result"))
            .filter(|acquired| *acquired)
            .count();
        assert!(acquired <= MAX_HOST_NUDGE_HELPERS);
        assert_eq!(budget.inflight(), acquired);
        release.wait();
        for handle in handles {
            handle.join().expect("join acquire worker");
        }
        assert_eq!(budget.inflight(), 0);
    }

    #[test]
    fn helper_permit_drop_releases_exactly_one_slot() {
        let budget = Arc::new(HelperThreadBudget::new(2));
        let permit = budget.try_acquire().expect("first permit");
        assert_eq!(budget.inflight(), 1);
        drop(permit);
        assert_eq!(budget.inflight(), 0);
    }

    #[test]
    fn helper_permit_survives_budget_drop() {
        let budget = Arc::new(HelperThreadBudget::new(1));
        let inflight = Arc::clone(&budget.inflight);
        let permit = budget.try_acquire().expect("permit");
        drop(budget);
        assert_eq!(inflight.load(Ordering::SeqCst), 1);
        drop(permit);
        assert_eq!(inflight.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn bounded_host_nudge_injector_caps_helper_growth_under_repeated_hangs() {
        let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let blocking_injector = Arc::new(AlwaysBlocksInjector {
            released: Arc::clone(&released),
            call_count: AtomicUsize::new(0),
        });
        let injector = BoundedHostNudgeInjector::spawn(
            Arc::clone(&blocking_injector) as Arc<dyn HostNudgeInjector>
        );

        for _ in 0..MAX_HOST_NUDGE_HELPERS {
            let error = injector
                .inject_nudge(&request_nudge())
                .expect_err("blocked helper should time out");
            assert_eq!(error.code(), AtmErrorCode::WaitTimeout);
        }

        let error = injector
            .inject_nudge(&request_nudge())
            .expect_err("helper budget should eventually cap repeated hangs");
        assert_eq!(error.code(), AtmErrorCode::WaitTimeout);
        assert!(
            error
                .message()
                .contains("graft host nudge helper budget is exhausted"),
            "{error:?}"
        );
        assert_eq!(
            blocking_injector.call_count.load(Ordering::SeqCst),
            MAX_HOST_NUDGE_HELPERS
        );

        released.store(true, Ordering::SeqCst);
    }

    #[test]
    fn receiver_loop_delivers_direct_nudge_and_returns_ack_under_repeated_load() {
        let paths = test_paths();
        let injector = Arc::new(RecordingInjector::default());
        let (stop_tx, join, snapshot, (endpoint, capability)) = spawn_receiver(
            paths.workspace_root.clone(),
            injector.clone() as Arc<dyn HostNudgeInjector>,
        );

        for _ in 0..100 {
            let response = deliver_request(endpoint, &capability, request_event());
            assert_eq!(response, GraftPostSendResponse::Delivered);
        }
        let nudges = injector.nudges.lock().expect("nudges lock");
        assert_eq!(nudges.len(), 100);
        assert_eq!(
            nudges[0].body,
            "<atm>test nudge</atm>\n\nfull immutable body"
        );
        assert_eq!(
            read_snapshot(&snapshot).expect("snapshot").state,
            GraftSessionState::Listening
        );

        stop_receiver(stop_tx, join);
    }

    #[test]
    fn receiver_loop_restores_a_missing_endpoint_record_before_the_next_delivery() {
        let paths = test_paths();
        let endpoint_path = receiver_record_path(&paths);
        let injector = Arc::new(RecordingInjector::default());
        let (stop_tx, join, _snapshot, (endpoint, capability)) = spawn_receiver(
            paths.workspace_root.clone(),
            injector.clone() as Arc<dyn HostNudgeInjector>,
        );

        fs::remove_file(&endpoint_path).expect("remove published endpoint record");
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let (_poll_wait_tx, poll_wait_rx) = mpsc::channel();
        while !endpoint_path.exists() && std::time::Instant::now() < deadline {
            assert!(
                !wait_for_stop_or_delay(&poll_wait_rx, GRAFT_RECEIVER_ACCEPT_POLL_INTERVAL),
                "test-only wait channel must remain open until the endpoint record is restored"
            );
        }
        assert!(
            endpoint_path.exists(),
            "receiver must restore a record deleted by an external lifecycle event"
        );

        assert_eq!(
            deliver_request(endpoint, &capability, request_event()),
            GraftPostSendResponse::Delivered
        );
        assert_eq!(injector.nudges.lock().expect("nudges lock").len(), 1);
        stop_receiver(stop_tx, join);
    }

    #[test]
    fn hard_accept_failure_rebinds_and_resumes_authenticated_delivery() {
        let paths = test_paths();
        let endpoint_path = receiver_record_path(&paths);
        let injector = Arc::new(RecordingInjector::default());
        let (_stop_tx, stop_rx) = mpsc::channel();
        let snapshot = Arc::new(RwLock::new(SessionSnapshot {
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_QA),
            state: GraftSessionState::Degraded,
        }));
        let ctx = GraftReceiverLoopContext {
            graft_root: paths.workspace_root.clone(),
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_QA),
            owner_chat_id: None,
            client: None,
            snapshot: Arc::clone(&snapshot),
            injector: injector.clone() as Arc<dyn HostNudgeInjector>,
            observability: Arc::new(NoopObservability),
            stop_rx,
            ready_tx: None,
            receiver_target_tx: None,
        };
        let listener = bind_receiver(&paths, None).expect("bind initial listener");
        let listener = RegisteredGraftReceiver::new(listener, &ctx);
        let initial_record = fs::read_to_string(&endpoint_path).expect("read initial record");

        let listener = recover_after_poll_accept_error(
            &ctx,
            listener,
            &AtmError::daemon_unavailable("simulated hard accept failure"),
        )
        .expect("recover after hard accept failure")
        .expect("rebind after hard accept failure");
        assert_eq!(
            read_snapshot(&snapshot)
                .expect("read post-rebind snapshot")
                .state,
            GraftSessionState::Listening,
            "only a successful rebind returns the session to Listening"
        );
        assert_ne!(
            fs::read_to_string(&endpoint_path).expect("read rebound record"),
            initial_record,
            "rebind must republish a fresh capability record"
        );

        let endpoint = listener.listener.local_addr().expect("rebound endpoint");
        let capability = listener.listener.capability().clone();
        let sender =
            std::thread::spawn(move || deliver_request(endpoint, &capability, request_event()));
        let mut stream = loop {
            if let Some(stream) = listener
                .listener
                .poll_accept()
                .expect("poll rebound listener")
            {
                break stream;
            }
            std::thread::yield_now();
        };
        let bounded_injector = BoundedHostNudgeInjector::spawn(injector.clone());
        handle_graft_receiver_connection(&ctx, &bounded_injector, &listener.listener, &mut stream)
            .expect("deliver through rebound listener");
        assert_eq!(
            sender.join().expect("join sender"),
            GraftPostSendResponse::Delivered
        );
        assert_eq!(injector.nudges.lock().expect("nudges lock").len(), 1);
    }

    #[test]
    fn receiver_recovery_marks_session_degraded_while_rebind_is_blocked() {
        let paths = test_paths();
        let initial_listener = bind_receiver(&paths, None).expect("bind initial listener");
        drop(initial_listener);
        let blocker = bind_receiver(&paths, None).expect("bind competing listener");
        let (stop_tx, stop_rx) = mpsc::channel();
        let (state_tx, state_rx) = mpsc::sync_channel(2);
        let snapshot = Arc::new(RwLock::new(SessionSnapshot {
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_QA),
            state: GraftSessionState::Listening,
        }));
        let ctx = GraftReceiverLoopContext {
            graft_root: paths.workspace_root,
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_QA),
            owner_chat_id: None,
            client: None,
            snapshot: Arc::clone(&snapshot),
            injector: Arc::new(RecordingInjector::default()),
            observability: Arc::new(StateObservability { states: state_tx }),
            stop_rx,
            ready_tx: None,
            receiver_target_tx: None,
        };
        let recovery = std::thread::spawn(move || recover_graft_receiver(&ctx));

        assert_eq!(
            state_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("recovery must publish its degraded state"),
            GraftSessionState::Degraded
        );
        assert_eq!(
            read_snapshot(&snapshot).expect("read retry snapshot").state,
            GraftSessionState::Degraded
        );

        stop_tx.send(()).expect("stop recovery");
        assert!(
            recovery
                .join()
                .expect("join recovery")
                .expect("recover result")
                .is_none(),
            "stop must end the rebind loop without a false Listening state"
        );
        drop(blocker);
    }

    #[test]
    fn receiver_recovery_circuit_escalates_after_a_bounded_recovery_window() {
        let started = Instant::now();
        let mut circuit = ReceiverRecoveryCircuit::new(started);

        assert!(!circuit.is_exhausted(started));
        assert!(!circuit.warning_due(started));
        assert!(circuit.warning_due(started + GRAFT_RECEIVER_RECOVERY_WARN_INITIAL_DELAY));
        assert!(!circuit.warning_due(
            started
                + GRAFT_RECEIVER_RECOVERY_WARN_INITIAL_DELAY
                + GRAFT_RECEIVER_RECOVERY_WARN_INITIAL_DELAY
                - Duration::from_millis(1)
        ));
        assert!(circuit.warning_due(
            started
                + GRAFT_RECEIVER_RECOVERY_WARN_INITIAL_DELAY
                + GRAFT_RECEIVER_RECOVERY_WARN_INITIAL_DELAY
        ));
        assert!(circuit.is_exhausted(started + GRAFT_RECEIVER_RECOVERY_MAX_DURATION));

        circuit.reset(started + GRAFT_RECEIVER_RECOVERY_MAX_DURATION);
        assert!(!circuit.is_exhausted(started + GRAFT_RECEIVER_RECOVERY_MAX_DURATION));
        assert!(!circuit.warning_due(started + GRAFT_RECEIVER_RECOVERY_MAX_DURATION));
    }

    #[test]
    fn receiver_loop_returns_typed_error_when_injector_fails() {
        let paths = test_paths();
        let (stop_tx, join, snapshot, (endpoint, capability)) =
            spawn_receiver(paths.workspace_root.clone(), Arc::new(FailingInjector));

        let response = deliver_request(endpoint, &capability, request_event());

        match response {
            GraftPostSendResponse::Delivered => panic!("expected typed failure response"),
            GraftPostSendResponse::Error(error) => {
                assert_eq!(error.code(), AtmErrorCode::PostSendGraftUnavailable);
                assert_eq!(
                    error.message(),
                    "Repair the configured post-send target and retry if delivery is required."
                );
            }
        }
        assert_eq!(
            read_snapshot(&snapshot).expect("snapshot").state,
            GraftSessionState::Listening
        );

        stop_receiver(stop_tx, join);
    }
}
