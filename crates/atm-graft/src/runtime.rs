use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use std::sync::mpsc::TrySendError;

use atm_core::GraftConfig;
use atm_core::boundary::{
    BuiltInPostSendDispatch, GraftNudgeTarget, MessageReceivedHookEmitter, PostSendBuiltInTarget,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::graft::{
    GRAFT_RECEIVER_ACCEPT_POLL_INTERVAL, GraftPostSendResponse, GraftReceiverListener,
};
use atm_core::types::ChatId;

use crate::nudge_sink::GraftReceiveHook;
use crate::{
    GraftObservability, GraftSessionState, HostNudge, HostNudgeInjector,
    RECEIVE_LOOP_JOIN_DEADLINE, SessionSnapshot,
};

pub(crate) const RECEIVE_LOOP_READY_DEADLINE: Duration = Duration::from_secs(3);
const HOST_NUDGE_INJECTION_DEADLINE: Duration = Duration::from_millis(250);
const GRAFT_RECEIVER_IO_DEADLINE: Duration = Duration::from_secs(3);
const MAX_HOST_NUDGE_HELPERS: usize = 8;

type ReceiveLoopJoinHelper = (
    Receiver<Result<(), AtmError>>,
    JoinHandle<()>,
    std::thread::ThreadId,
);

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
    fn inject_nudge(&self, nudge: &HostNudge) -> Result<(), AtmError> {
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
    pub(crate) endpoint_path: PathBuf,
    pub(crate) owner_chat_id: Option<ChatId>,
    pub(crate) snapshot: SharedSessionSnapshot,
    pub(crate) injector: Arc<dyn HostNudgeInjector>,
    pub(crate) observability: Arc<dyn GraftObservability>,
    pub(crate) stop_rx: Receiver<()>,
    pub(crate) ready_tx: Option<SyncSender<()>>,
}

pub(crate) fn run_graft_receiver_loop(ctx: GraftReceiverLoopContext) -> Result<(), AtmError> {
    let injector = BoundedHostNudgeInjector::spawn(Arc::clone(&ctx.injector));
    let result = (|| {
        let listener =
            match GraftReceiverListener::bind(&ctx.endpoint_path, ctx.owner_chat_id.clone()) {
                Ok(listener) => {
                    let snapshot = read_snapshot(&ctx.snapshot)?;
                    ctx.observability.receiver_ownership(
                        &snapshot,
                        "activate_receiver_owner",
                        "ok",
                    );
                    listener
                }
                Err(error) => {
                    let snapshot = read_snapshot(&ctx.snapshot)?;
                    let outcome = if error.code() == AtmErrorCode::GraftReceiverAlreadyActive {
                        "conflict"
                    } else {
                        "error"
                    };
                    ctx.observability.receiver_ownership(
                        &snapshot,
                        "activate_receiver_owner",
                        outcome,
                    );
                    return Err(error);
                }
            };
        if let Some(ready_tx) = ctx.ready_tx.as_ref() {
            signal_ready_sender(ready_tx)?;
        }
        // Non-blocking accept + poll: the loop re-checks its stop signal every
        // ACCEPT_POLL_INTERVAL instead of parking in a blocking accept, so no
        // wake-by-connect machinery is needed to unblock shutdown.
        loop {
            if stop_requested(&ctx.stop_rx) {
                return Ok(());
            }
            match listener.poll_accept()? {
                Some(mut stream) => {
                    if stop_requested(&ctx.stop_rx) {
                        return Ok(());
                    }
                    if let Err(error) =
                        handle_graft_receiver_connection(&ctx, &injector, &listener, &mut stream)
                    {
                        warn_runtime_error(
                            "handle_graft_receiver_connection",
                            Some(&ctx.endpoint_path),
                            &error,
                        );
                    }
                }
                None => thread::sleep(GRAFT_RECEIVER_ACCEPT_POLL_INTERVAL),
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
    let dispatch = BuiltInPostSendDispatch {
        target: PostSendBuiltInTarget::Graft(GraftNudgeTarget {
            recipient: event.recipient.clone(),
            recipient_team: event.recipient_team.clone(),
            rendered_nudge,
        }),
        event,
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
        GraftPostSendRequest, GraftPostSendResponse, GraftReceiverListener, deliver_graft_post_send,
    };
    use atm_core::schema::AtmMessageId;
    use atm_core::test_support::{TEST_LEAD, TEST_QA, TEST_TEAM};
    use atm_core::types::{AgentName, TeamName};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex, RwLock};
    use std::time::Duration;
    use tempfile::TempDir;

    use crate::{GraftObservability, HostNudge, HostNudgeInjector};

    use super::{
        BoundedHostNudgeInjector, GraftReceiverLoopContext, MAX_HOST_NUDGE_HELPERS,
        RECEIVE_LOOP_READY_DEADLINE, ReceiverReadyLatch, join_receive_loop_with_deadline,
        load_graft_config, read_snapshot, run_graft_receiver_loop,
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
        atm_core::graft::graft_receiver_record_path_from_home(
            &paths.workspace_root,
            &TeamName::from_validated(TEST_TEAM),
            &AgentName::from_validated(TEST_QA),
        )
    }

    fn deliver_request(record_path: &Path, event: PostSendHookEvent) -> GraftPostSendResponse {
        deliver_graft_post_send(
            record_path,
            &GraftPostSendRequest {
                event,
                rendered_nudge: "<atm>test nudge</atm>".to_string(),
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
            owner_chat_id: None,
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
    fn receiver_listener_binds_at_expected_endpoint() {
        let paths = test_paths();
        let endpoint_path = receiver_endpoint_path(&paths);
        let listener = GraftReceiverListener::bind(&endpoint_path, None).expect("bind listener");
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
        let endpoint_path = receiver_endpoint_path(&paths);
        let injector = Arc::new(RecordingInjector::default());
        let (stop_tx, join, snapshot) = spawn_receiver(
            endpoint_path.clone(),
            injector.clone() as Arc<dyn HostNudgeInjector>,
        );

        for _ in 0..100 {
            let response = deliver_request(&endpoint_path, request_event());
            assert_eq!(response, GraftPostSendResponse::Delivered);
        }
        let nudges = injector.nudges.lock().expect("nudges lock");
        assert_eq!(nudges.len(), 100);
        assert_eq!(
            read_snapshot(&snapshot).expect("snapshot").state,
            GraftSessionState::Listening
        );

        stop_receiver(stop_tx, join);
    }

    #[test]
    fn receiver_loop_returns_typed_error_when_injector_fails() {
        let paths = test_paths();
        let endpoint_path = receiver_endpoint_path(&paths);
        let (stop_tx, join, snapshot) =
            spawn_receiver(endpoint_path.clone(), Arc::new(FailingInjector));

        let response = deliver_request(&endpoint_path, request_event());

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
