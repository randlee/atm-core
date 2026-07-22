use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use std::sync::mpsc::TrySendError;
#[cfg(windows)]
use std::time::Instant;

use atm_core::GraftConfig;
use atm_core::boundary::PostSendHookEvent;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::graft::{
    GraftPostSendRequest, GraftPostSendResponse, read_graft_post_send_message,
    write_graft_post_send_message,
};
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{
    Listener as LocalSocketListener, ListenerOptions, Stream as LocalSocketStream,
};

use crate::nudge_sink::GraftNudgeSink;
use crate::{
    GraftObservability, GraftSessionState, HostNudgeInjector, RECEIVE_LOOP_JOIN_DEADLINE,
    SessionSnapshot,
};

pub(crate) const RECEIVE_LOOP_READY_DEADLINE: Duration = Duration::from_secs(3);
const HOST_NUDGE_INJECTION_DEADLINE: Duration = Duration::from_millis(250);
const LISTENER_WAKE_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
const GRAFT_RECEIVER_IO_DEADLINE: Duration = Duration::from_secs(3);
const MAX_HOST_NUDGE_HELPERS: usize = 8;
const MAX_LISTENER_WAKE_HELPERS: usize = 2;
#[cfg(windows)]
const GRAFT_RECEIVER_HELPER_POLL_INTERVAL: Duration = Duration::from_millis(25);

type ReceiveLoopJoinHelper = (
    Receiver<Result<(), AtmError>>,
    JoinHandle<()>,
    std::thread::ThreadId,
);

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalIpcDeadlineSupport {
    Applied,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReceiverDeadlineSupport {
    recv: LocalIpcDeadlineSupport,
    send: LocalIpcDeadlineSupport,
}

#[derive(Debug)]
struct HelperThreadBudget {
    max_inflight: usize,
    inflight: AtomicUsize,
}

impl HelperThreadBudget {
    const fn new(max_inflight: usize) -> Self {
        Self {
            max_inflight,
            inflight: AtomicUsize::new(0),
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
                        budget: Arc::clone(self),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }
}

struct HelperThreadPermit {
    budget: Arc<HelperThreadBudget>,
}

impl Drop for HelperThreadPermit {
    fn drop(&mut self) {
        self.budget.inflight.fetch_sub(1, Ordering::SeqCst);
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
    fn inject_nudge(&self, nudge: &PostSendHookEvent) -> Result<(), AtmError> {
        Self::inject_nudge(self, nudge)
    }
}

impl BoundedHostNudgeInjector {
    fn spawn(injector: Arc<dyn HostNudgeInjector>) -> Self {
        Self {
            injector,
            helper_budget: Arc::new(HelperThreadBudget::new(MAX_HOST_NUDGE_HELPERS)),
        }
    }

    fn inject_nudge(&self, event: &PostSendHookEvent) -> Result<(), AtmError> {
        let helper_permit = acquire_host_nudge_helper_permit(&self.helper_budget)?;
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        spawn_host_nudge_helper(
            Arc::clone(&self.injector),
            event.clone(),
            helper_permit,
            result_tx,
        )?;
        receive_host_nudge_result(result_rx, &self.helper_budget)
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
    event: PostSendHookEvent,
    helper_permit: HelperThreadPermit,
    result_tx: SyncSender<Result<(), AtmError>>,
) -> Result<(), AtmError> {
    thread::Builder::new()
        .name("atm-graft-host-nudge".to_string())
        .spawn(move || {
            let _helper_permit = helper_permit;
            let result = injector.inject_nudge(&event);
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

fn listener_wake_budget_exhausted_error(endpoint_path: &Path) -> AtmError {
    let helper_budget = listener_wake_helper_budget();
    AtmError::daemon_unavailable(format!(
        "graft listener wake helper budget is exhausted at {} in-flight helpers for {}",
        helper_budget.max_inflight(),
        endpoint_path.display()
    ))
}

fn listener_wake_helper_budget() -> &'static Arc<HelperThreadBudget> {
    static LISTENER_WAKE_HELPER_BUDGET: std::sync::OnceLock<Arc<HelperThreadBudget>> =
        std::sync::OnceLock::new();
    LISTENER_WAKE_HELPER_BUDGET
        .get_or_init(|| Arc::new(HelperThreadBudget::new(MAX_LISTENER_WAKE_HELPERS)))
}

pub(crate) struct GraftReceiverLoopContext {
    pub(crate) endpoint_path: PathBuf,
    pub(crate) snapshot: SharedSessionSnapshot,
    pub(crate) injector: Arc<dyn HostNudgeInjector>,
    pub(crate) observability: Arc<dyn GraftObservability>,
    pub(crate) stop_rx: Receiver<()>,
    pub(crate) ready_tx: Option<SyncSender<()>>,
}

pub(crate) fn run_graft_receiver_loop(ctx: GraftReceiverLoopContext) -> Result<(), AtmError> {
    let injector = BoundedHostNudgeInjector::spawn(Arc::clone(&ctx.injector));
    let result = (|| {
        let listener = bind_graft_receiver_listener(&ctx.endpoint_path)?;
        if let Some(ready_tx) = ctx.ready_tx.as_ref() {
            signal_ready_sender(ready_tx)?;
        }
        loop {
            if stop_requested(&ctx.stop_rx) {
                return Ok(());
            }
            let stream = accept_graft_receiver_connection(&listener, &ctx.endpoint_path)?;
            if stop_requested(&ctx.stop_rx) {
                return Ok(());
            }
            if let Err(error) = handle_graft_receiver_connection(&ctx, &injector, stream) {
                warn_runtime_error(
                    "handle_graft_receiver_connection",
                    Some(&ctx.endpoint_path),
                    &error,
                );
            }
        }
    })();
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
        warn_runtime_error("set_session_state", Some(&ctx.endpoint_path), &state_error);
    }
    result
}

pub(crate) fn wake_graft_receiver_listener(endpoint_path: &Path) -> Result<(), AtmError> {
    wake_graft_receiver_listener_with_budget_and_connector(
        endpoint_path,
        listener_wake_helper_budget(),
        LocalSocketStream::connect,
    )
}

fn wake_graft_receiver_listener_with_budget_and_connector<C>(
    endpoint_path: &Path,
    helper_budget: &Arc<HelperThreadBudget>,
    connect: C,
) -> Result<(), AtmError>
where
    C: FnOnce(interprocess::local_socket::Name<'static>) -> std::io::Result<LocalSocketStream>
        + Send
        + 'static,
{
    let name = atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path)?;
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let helper_permit = acquire_listener_wake_helper_permit(endpoint_path, helper_budget)?;
    spawn_listener_wake_helper(name, helper_permit, result_tx, connect)?;
    receive_listener_wake_result(result_rx, endpoint_path, helper_budget)
}

fn acquire_listener_wake_helper_permit(
    endpoint_path: &Path,
    helper_budget: &Arc<HelperThreadBudget>,
) -> Result<HelperThreadPermit, AtmError> {
    helper_budget.try_acquire().ok_or_else(|| {
        let error = listener_wake_budget_exhausted_error(endpoint_path);
        warn_listener_wake_result(
            "helper_budget_exhausted",
            endpoint_path,
            &error,
            helper_budget,
            None,
        );
        error
    })
}

fn spawn_listener_wake_helper(
    name: interprocess::local_socket::Name<'static>,
    helper_permit: HelperThreadPermit,
    result_tx: SyncSender<std::io::Result<LocalSocketStream>>,
    connect: impl FnOnce(
        interprocess::local_socket::Name<'static>,
    ) -> std::io::Result<LocalSocketStream>
    + Send
    + 'static,
) -> Result<(), AtmError> {
    thread::Builder::new()
        .name("graft-listener-wake-connect".to_string())
        .spawn(move || {
            let _helper_permit = helper_permit;
            let _ = result_tx.send(connect(name));
        })
        .map(|_| ())
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn graft listener wake worker")
        })
}

fn receive_listener_wake_result(
    result_rx: Receiver<std::io::Result<LocalSocketStream>>,
    endpoint_path: &Path,
    helper_budget: &Arc<HelperThreadBudget>,
) -> Result<(), AtmError> {
    match result_rx.recv_timeout(LISTENER_WAKE_CONNECT_DEADLINE) {
        Ok(Ok(_stream)) => Ok(()),
        Ok(Err(_source)) => {
            let error = AtmError::daemon_unavailable(format!(
                "failed to wake graft receiver listener at {}",
                endpoint_path.display()
            ));
            warn_runtime_error("wake_graft_receiver_listener", Some(endpoint_path), &error);
            Err(error)
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            let error = AtmError::daemon_unavailable(format!(
                "timed out waking graft receiver listener at {}",
                endpoint_path.display()
            ));
            warn_listener_wake_result(
                "timeout",
                endpoint_path,
                &error,
                helper_budget,
                Some(LISTENER_WAKE_CONNECT_DEADLINE.as_millis()),
            );
            warn_runtime_error("wake_graft_receiver_listener", Some(endpoint_path), &error);
            Err(error)
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            let error = AtmError::daemon_unavailable(format!(
                "graft listener wake worker disconnected unexpectedly for {}",
                endpoint_path.display()
            ));
            warn_listener_wake_result("disconnected", endpoint_path, &error, helper_budget, None);
            warn_runtime_error("wake_graft_receiver_listener", Some(endpoint_path), &error);
            Err(error)
        }
    }
}

fn warn_listener_wake_result(
    outcome: &'static str,
    endpoint_path: &Path,
    error: &AtmError,
    helper_budget: &Arc<HelperThreadBudget>,
    timeout_ms: Option<u128>,
) {
    tracing::warn!(
        subsystem = "atm_graft.receiver_loop",
        action = "wake_graft_receiver_listener",
        outcome,
        endpoint = %endpoint_path.display(),
        timeout_ms,
        helper_budget_max = helper_budget.max_inflight(),
        helper_budget_inflight = helper_budget.inflight(),
        error_code = %error.code(),
        error_message = %error.message(),
        "graft listener wake helper error"
    );
}

fn stop_requested(stop_rx: &Receiver<()>) -> bool {
    matches!(
        stop_rx.try_recv(),
        Ok(()) | Err(std::sync::mpsc::TryRecvError::Disconnected)
    )
}

fn bind_graft_receiver_listener(endpoint_path: &Path) -> Result<LocalSocketListener, AtmError> {
    prepare_graft_receiver_endpoint(endpoint_path)?;
    ListenerOptions::new()
        .name(atm_core::protocol::daemon_local_ipc_name_from_path(
            endpoint_path,
        )?)
        .create_sync()
        .map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to bind graft receiver endpoint at {}",
                endpoint_path.display()
            ))
        })
}

fn prepare_graft_receiver_endpoint(endpoint_path: &Path) -> Result<(), AtmError> {
    if let Some(parent) = endpoint_path.parent() {
        fs::create_dir_all(parent).map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to prepare graft receiver directory {}",
                parent.display()
            ))
        })?;
    }
    #[cfg(unix)]
    if endpoint_path.exists() {
        fs::remove_file(endpoint_path).map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to remove stale graft receiver endpoint {}",
                endpoint_path.display()
            ))
        })?;
    }
    Ok(())
}

fn accept_graft_receiver_connection(
    listener: &LocalSocketListener,
    endpoint_path: &Path,
) -> Result<LocalSocketStream, AtmError> {
    listener.accept().map_err(|_source| {
        let error = AtmError::daemon_unavailable(format!(
            "failed while accepting graft receiver connection at {}",
            endpoint_path.display()
        ));
        warn_runtime_error(
            "accept_graft_receiver_connection",
            Some(endpoint_path),
            &error,
        );
        error
    })
}

fn handle_graft_receiver_connection(
    ctx: &GraftReceiverLoopContext,
    injector: &BoundedHostNudgeInjector,
    stream: LocalSocketStream,
) -> Result<(), AtmError> {
    let deadline_support = apply_receiver_deadlines(&stream)?;
    let (stream, request): (LocalSocketStream, GraftPostSendRequest) =
        read_graft_post_send_request_with_deadline(stream, deadline_support.recv)?;
    let event = request.event;
    let response = match (GraftNudgeSink {
        injector,
        snapshot: &ctx.snapshot,
        observability: ctx.observability.as_ref(),
    })
    .deliver(event)
    {
        Ok(()) => GraftPostSendResponse::Delivered,
        Err(error) => GraftPostSendResponse::Error(error),
    };
    write_graft_post_send_response_with_deadline(
        stream,
        &response,
        deadline_support.send,
        &ctx.endpoint_path,
    )
}

fn read_graft_post_send_request_with_deadline(
    mut stream: LocalSocketStream,
    _recv_deadline_support: LocalIpcDeadlineSupport,
) -> Result<(LocalSocketStream, GraftPostSendRequest), AtmError> {
    #[cfg(windows)]
    if _recv_deadline_support == LocalIpcDeadlineSupport::Unsupported {
        return read_graft_post_send_request_with_helper(stream);
    }

    let request: GraftPostSendRequest = read_graft_post_send_message(
        &mut stream,
        "failed to read graft post-send request",
        "graft post-send request exceeded the bounded payload cap",
    )?;
    Ok((stream, request))
}

fn write_graft_post_send_response_with_deadline(
    mut stream: LocalSocketStream,
    response: &GraftPostSendResponse,
    _send_deadline_support: LocalIpcDeadlineSupport,
    endpoint_path: &Path,
) -> Result<(), AtmError> {
    #[cfg(windows)]
    if _send_deadline_support == LocalIpcDeadlineSupport::Unsupported {
        return write_graft_post_send_response_with_helper(stream, response, endpoint_path);
    }

    write_graft_post_send_message(
        &mut stream,
        &response,
        "failed to write graft post-send response",
        "graft post-send response exceeded the bounded payload cap",
    )?;
    use std::io::Write as _;
    stream.flush().map_err(|_source| {
        let error = AtmError::daemon_unavailable("failed to flush graft post-send response");
        warn_runtime_error(
            "handle_graft_receiver_connection",
            Some(endpoint_path),
            &error,
        );
        error
    })
}

fn apply_receiver_deadlines(
    stream: &LocalSocketStream,
) -> Result<ReceiverDeadlineSupport, AtmError> {
    let recv = apply_receiver_deadline(
        stream.set_recv_timeout(Some(GRAFT_RECEIVER_IO_DEADLINE)),
        "failed to apply graft receiver receive timeout",
    )?;
    let send = apply_receiver_deadline(
        stream.set_send_timeout(Some(GRAFT_RECEIVER_IO_DEADLINE)),
        "failed to apply graft receiver send timeout",
    )?;
    Ok(ReceiverDeadlineSupport { recv, send })
}

fn apply_receiver_deadline(
    result: std::io::Result<()>,
    message: &'static str,
) -> Result<LocalIpcDeadlineSupport, AtmError> {
    match result {
        Ok(()) => Ok(LocalIpcDeadlineSupport::Applied),
        #[cfg(windows)]
        Err(source) if source.kind() == std::io::ErrorKind::Unsupported => {
            Ok(LocalIpcDeadlineSupport::Unsupported)
        }
        Err(_source) => {
            let error = AtmError::daemon_unavailable(message);
            warn_runtime_error("apply_receiver_deadline", None, &error);
            Err(error)
        }
    }
}

#[cfg(windows)]
fn read_graft_post_send_request_with_helper(
    stream: LocalSocketStream,
) -> Result<(LocalSocketStream, GraftPostSendRequest), AtmError> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("atm-graft-request-read-helper".to_string())
        .spawn(move || {
            let mut stream = stream;
            let result = read_graft_post_send_message(
                &mut stream,
                "failed to read graft post-send request",
                "graft post-send request exceeded the bounded payload cap",
            );
            if result_tx.send((stream, result)).is_err() {
                tracing::debug!(
                    timeout_ms = GRAFT_RECEIVER_IO_DEADLINE.as_millis(),
                    "graft request-read helper dropped its result because the bounded caller already timed out"
                );
            }
        })
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn graft request-read helper")


        })?;

    let started = Instant::now();
    loop {
        let remaining = GRAFT_RECEIVER_IO_DEADLINE.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(AtmError::daemon_unavailable(
                "timed out reading graft post-send request",
            ));
        }
        let poll = std::cmp::min(remaining, GRAFT_RECEIVER_HELPER_POLL_INTERVAL);
        match result_rx.recv_timeout(poll) {
            Ok((stream, result)) => return result.map(|request| (stream, request)),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AtmError::daemon_unavailable(
                    "graft request-read helper disconnected unexpectedly",
                ));
            }
        }
    }
}

#[cfg(windows)]
fn write_graft_post_send_response_with_helper(
    stream: LocalSocketStream,
    response: &GraftPostSendResponse,
    endpoint_path: &Path,
) -> Result<(), AtmError> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let response = response.clone();
    let endpoint = endpoint_path.to_path_buf();
    thread::Builder::new()
        .name("atm-graft-response-write-helper".to_string())
        .spawn(move || {
            let mut stream = stream;
            let result = (|| {
                write_graft_post_send_message(
                    &mut stream,
                    &response,
                    "failed to write graft post-send response",
                    "graft post-send response exceeded the bounded payload cap",
                )?;
                use std::io::Write as _;
                stream.flush().map_err(|_source| {
                    let error = AtmError::daemon_unavailable("failed to flush graft post-send response")

                        ;
                    warn_runtime_error("handle_graft_receiver_connection", Some(endpoint.as_path()), &error);
                    error
                })
            })();
            if result_tx.send(result).is_err() {
                tracing::debug!(
                    timeout_ms = GRAFT_RECEIVER_IO_DEADLINE.as_millis(),
                    "graft response-write helper dropped its result because the bounded caller already timed out"
                );
            }
        })
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn graft response-write helper")


        })?;

    let started = Instant::now();
    loop {
        let remaining = GRAFT_RECEIVER_IO_DEADLINE.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(AtmError::daemon_unavailable(
                "timed out writing graft post-send response",
            ));
        }
        let poll = std::cmp::min(remaining, GRAFT_RECEIVER_HELPER_POLL_INTERVAL);
        match result_rx.recv_timeout(poll) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AtmError::daemon_unavailable(
                    "graft response-write helper disconnected unexpectedly",
                ));
            }
        }
    }
}

#[cfg(test)]
#[cfg_attr(windows, allow(dead_code, unused_imports))]
mod tests {
    use atm_core::boundary::PostSendHookEvent;
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::graft::{
        GraftPostSendRequest, GraftPostSendResponse, read_graft_post_send_message,
        write_graft_post_send_message,
    };
    use atm_core::schema::AtmMessageId;
    use atm_core::test_support::{TEST_LEAD, TEST_QA, TEST_TEAM};
    use atm_core::types::{AgentName, TeamName};
    use interprocess::local_socket::Stream as LocalSocketStream;
    use interprocess::local_socket::traits::Stream as _;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex, RwLock};
    use tempfile::TempDir;

    use crate::{GraftObservability, HostNudgeInjector};

    use super::{
        BoundedHostNudgeInjector, GraftReceiverLoopContext, MAX_HOST_NUDGE_HELPERS,
        MAX_LISTENER_WAKE_HELPERS, RECEIVE_LOOP_READY_DEADLINE, ReceiverReadyLatch,
        apply_receiver_deadline, bind_graft_receiver_listener, join_receive_loop_with_deadline,
        load_graft_config, read_snapshot, run_graft_receiver_loop, wake_graft_receiver_listener,
        wake_graft_receiver_listener_with_budget_and_connector,
    };
    use crate::{GraftSessionState, SessionSnapshot};

    #[derive(Debug, Default)]
    struct RecordingInjector {
        nudges: Mutex<Vec<PostSendHookEvent>>,
    }

    impl HostNudgeInjector for RecordingInjector {
        fn inject_nudge(&self, nudge: &PostSendHookEvent) -> Result<(), AtmError> {
            self.nudges.lock().expect("nudges lock").push(nudge.clone());
            Ok(())
        }
    }

    #[derive(Debug)]
    struct FailingInjector;

    impl HostNudgeInjector for FailingInjector {
        fn inject_nudge(&self, _nudge: &PostSendHookEvent) -> Result<(), AtmError> {
            Err(AtmError::for_code(AtmErrorCode::PostSendGraftUnavailable))
        }
    }

    #[derive(Debug, Default)]
    struct NoopObservability;

    impl GraftObservability for NoopObservability {}

    #[derive(Debug)]
    struct FirstCallBlocksInjector {
        first_call_gate: Mutex<Option<mpsc::Receiver<()>>>,
        call_count: AtomicUsize,
    }

    impl HostNudgeInjector for FirstCallBlocksInjector {
        fn inject_nudge(&self, _nudge: &PostSendHookEvent) -> Result<(), AtmError> {
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
        fn inject_nudge(&self, _nudge: &PostSendHookEvent) -> Result<(), AtmError> {
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

    fn receiver_endpoint_path(paths: &TestPaths) -> PathBuf {
        atm_core::graft::graft_receiver_socket_path_from_home(
            &paths.workspace_root,
            &TeamName::from_validated(TEST_TEAM),
            &AgentName::from_validated(TEST_QA),
        )
    }

    fn connect_receiver(endpoint_path: &Path) -> LocalSocketStream {
        let name = atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path)
            .expect("receiver ipc name");
        LocalSocketStream::connect(name).unwrap_or_else(|error| {
            panic!("failed to connect to graft receiver after readiness signal: {error}")
        })
    }

    fn request_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: AgentName::from_validated(TEST_LEAD),
            sender_team: TeamName::from_validated(TEST_TEAM),
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

    fn spawn_receiver(
        endpoint_path: PathBuf,
        injector: Arc<dyn HostNudgeInjector>,
    ) -> SpawnedReceiver {
        let (stop_tx, stop_rx) = mpsc::channel();
        let ready_latch = ReceiverReadyLatch::new();
        let snapshot = Arc::new(RwLock::new(SessionSnapshot {
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_QA),
            state: GraftSessionState::Listening,
        }));
        let ctx = GraftReceiverLoopContext {
            endpoint_path,
            snapshot: Arc::clone(&snapshot),
            injector,
            observability: Arc::new(NoopObservability),
            stop_rx,
            ready_tx: Some(ready_latch.notifier()),
        };
        let join = std::thread::spawn(move || run_graft_receiver_loop(ctx));
        ready_latch
            .wait_until_listening(RECEIVE_LOOP_READY_DEADLINE)
            .expect("receiver ready");
        (stop_tx, join, snapshot)
    }

    fn stop_receiver(
        endpoint_path: &Path,
        stop_tx: std::sync::mpsc::Sender<()>,
        join: std::thread::JoinHandle<Result<(), AtmError>>,
    ) {
        stop_tx.send(()).expect("stop");
        let _ = wake_graft_receiver_listener(endpoint_path);
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

    #[cfg(not(windows))]
    #[test]
    fn receiver_listener_binds_at_expected_endpoint() {
        let paths = test_paths();
        let endpoint_path = receiver_endpoint_path(&paths);
        let listener = bind_graft_receiver_listener(&endpoint_path).expect("bind listener");
        drop(listener);
    }

    #[test]
    fn bounded_host_nudge_injector_timeout_does_not_wedge_future_delivery() {
        let (gate_tx, gate_rx) = mpsc::channel();
        let injector = BoundedHostNudgeInjector::spawn(Arc::new(FirstCallBlocksInjector {
            first_call_gate: Mutex::new(Some(gate_rx)),
            call_count: AtomicUsize::new(0),
        }) as Arc<dyn HostNudgeInjector>);

        let first_error = injector
            .inject_nudge(&request_event())
            .expect_err("first delivery should time out");
        assert_eq!(first_error.code(), AtmErrorCode::WaitTimeout);

        injector
            .inject_nudge(&request_event())
            .expect("second delivery should use a fresh helper thread");

        gate_tx.send(()).expect("release blocked first helper");
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
                .inject_nudge(&request_event())
                .expect_err("blocked helper should time out");
            assert_eq!(error.code(), AtmErrorCode::WaitTimeout);
        }

        let error = injector
            .inject_nudge(&request_event())
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

    #[cfg(not(windows))]
    #[test]
    fn listener_wake_caps_helper_growth_under_repeated_hangs() {
        let paths = test_paths();
        let endpoint_path = receiver_endpoint_path(&paths);
        let helper_budget = Arc::new(super::HelperThreadBudget::new(MAX_LISTENER_WAKE_HELPERS));
        let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let connect_count = Arc::new(AtomicUsize::new(0));

        for _ in 0..MAX_LISTENER_WAKE_HELPERS {
            let released = Arc::clone(&released);
            let connect_count = Arc::clone(&connect_count);
            let error = wake_graft_receiver_listener_with_budget_and_connector(
                &endpoint_path,
                &helper_budget,
                move |_name| {
                    connect_count.fetch_add(1, Ordering::SeqCst);
                    while !released.load(Ordering::SeqCst) {
                        std::thread::yield_now();
                    }
                    Err(io::Error::new(io::ErrorKind::TimedOut, "released"))
                },
            )
            .expect_err("hung listener wake helper should time out");
            assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
            assert!(
                error
                    .message()
                    .contains("timed out waking graft receiver listener"),
                "{error:?}"
            );
        }

        let error = wake_graft_receiver_listener_with_budget_and_connector(
            &endpoint_path,
            &helper_budget,
            |_name| panic!("helper budget should reject before connect"),
        )
        .expect_err("listener wake helper budget should cap repeated hangs");
        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
        assert!(
            error
                .message()
                .contains("graft listener wake helper budget is exhausted"),
            "{error:?}"
        );
        assert_eq!(
            connect_count.load(Ordering::SeqCst),
            MAX_LISTENER_WAKE_HELPERS
        );

        released.store(true, Ordering::SeqCst);
        let started = std::time::Instant::now();
        while helper_budget.inflight() != 0 && started.elapsed() < std::time::Duration::from_secs(1)
        {
            std::thread::yield_now();
        }
        assert_eq!(helper_budget.inflight(), 0, "helper threads should drain");
    }

    #[cfg(not(windows))]
    #[test]
    fn receiver_loop_delivers_direct_nudge_and_returns_ack_under_repeated_load() {
        let paths = test_paths();
        let endpoint_path = receiver_endpoint_path(&paths);
        let injector = Arc::new(RecordingInjector::default());
        let (stop_tx, join, snapshot) = spawn_receiver(
            endpoint_path.clone(),
            injector.clone() as Arc<dyn HostNudgeInjector>,
        );

        for _ in 0..100 {
            let request = GraftPostSendRequest {
                event: request_event(),
            };
            let mut stream = connect_receiver(&endpoint_path);
            write_graft_post_send_message(
                &mut stream,
                &request,
                "write request",
                "oversized request",
            )
            .expect("write request");
            let response: GraftPostSendResponse =
                read_graft_post_send_message(&mut stream, "read response", "oversized response")
                    .expect("read response");
            assert_eq!(response, GraftPostSendResponse::Delivered);
        }
        let nudges = injector.nudges.lock().expect("nudges lock");
        assert_eq!(nudges.len(), 100);
        assert_eq!(
            read_snapshot(&snapshot).expect("snapshot").state,
            GraftSessionState::Listening
        );

        stop_receiver(&endpoint_path, stop_tx, join);
    }

    #[cfg(not(windows))]
    #[test]
    fn receiver_loop_returns_typed_error_when_injector_fails() {
        let paths = test_paths();
        let endpoint_path = receiver_endpoint_path(&paths);
        let (stop_tx, join, snapshot) =
            spawn_receiver(endpoint_path.clone(), Arc::new(FailingInjector));

        let request = GraftPostSendRequest {
            event: request_event(),
        };
        let mut stream = connect_receiver(&endpoint_path);
        write_graft_post_send_message(&mut stream, &request, "write request", "oversized request")
            .expect("write request");
        let response: GraftPostSendResponse =
            read_graft_post_send_message(&mut stream, "read response", "oversized response")
                .expect("read response");

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

        stop_receiver(&endpoint_path, stop_tx, join);
    }

    #[test]
    fn receiver_deadline_unsupported_timeout_uses_platform_contract() {
        let result = apply_receiver_deadline(
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "local socket backend does not support I/O timeouts",
            )),
            "failed to apply graft receiver receive timeout",
        );

        #[cfg(windows)]
        assert_eq!(
            result.expect("windows helper fallback"),
            super::LocalIpcDeadlineSupport::Unsupported
        );

        #[cfg(not(windows))]
        {
            let error = result.expect_err(
                "non-Windows transports should keep unsupported receiver deadlines as hard errors",
            );
            assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
            assert!(
                error
                    .message()
                    .contains("failed to apply graft receiver receive timeout")
            );
        }
    }
}
