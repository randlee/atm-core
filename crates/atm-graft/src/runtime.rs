use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};

use atm_core::GraftConfig;
use atm_core::boundary::PostSendHookEvent;
use atm_core::error::{AtmError, AtmErrorKind};
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

#[cfg(test)]
const HOST_NUDGE_INJECTION_DEADLINE: std::time::Duration = std::time::Duration::from_millis(50);
#[cfg(not(test))]
const HOST_NUDGE_INJECTION_DEADLINE: std::time::Duration = std::time::Duration::from_millis(250);
const LISTENER_WAKE_CONNECT_DEADLINE: std::time::Duration = std::time::Duration::from_millis(250);
const GRAFT_RECEIVER_IO_DEADLINE: std::time::Duration = std::time::Duration::from_secs(3);

type ReceiveLoopJoinHelper = (
    Receiver<Result<(), AtmError>>,
    JoinHandle<()>,
    std::thread::ThreadId,
);

struct InjectRequest {
    event: PostSendHookEvent,
    result_tx: SyncSender<Result<(), AtmError>>,
}

struct BoundedHostNudgeInjector {
    request_tx: SyncSender<InjectRequest>,
}

impl crate::HostNudgeInjector for BoundedHostNudgeInjector {
    fn inject_nudge(&self, nudge: PostSendHookEvent) -> Result<(), AtmError> {
        Self::inject_nudge(self, nudge)
    }
}

impl BoundedHostNudgeInjector {
    fn spawn(injector: Arc<dyn HostNudgeInjector>) -> Result<Self, AtmError> {
        let (request_tx, request_rx) = mpsc::sync_channel::<InjectRequest>(0);
        thread::Builder::new()
            .name("atm-graft-host-nudge".to_string())
            .spawn(move || {
                while let Ok(request) = request_rx.recv() {
                    let result = injector.inject_nudge(request.event);
                    let _ = request.result_tx.send(result);
                }
            })
            .map_err(|source| {
                AtmError::new(
                    AtmErrorKind::Internal,
                    "failed to spawn graft host nudge worker",
                )
                .with_source(source)
                .with_recovery(
                    "Retry graft activation after the embedding host can spawn one bounded nudge worker thread.",
                )
            })?;
        Ok(Self { request_tx })
    }

    fn inject_nudge(&self, event: PostSendHookEvent) -> Result<(), AtmError> {
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        self.request_tx
            .try_send(InjectRequest { event, result_tx })
            .map_err(inject_request_enqueue_error)?;
        match result_rx.recv_timeout(HOST_NUDGE_INJECTION_DEADLINE) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(AtmError::new(
                AtmErrorKind::Timeout,
                format!(
                    "graft host nudge injection exceeded the {:?} delivery deadline",
                    HOST_NUDGE_INJECTION_DEADLINE
                ),
            )
            .with_recovery(
                "Fix or restart the embedding host nudge receiver before retrying graft delivery.",
            )),
            Err(RecvTimeoutError::Disconnected) => Err(AtmError::new(
                AtmErrorKind::Internal,
                "graft host nudge worker disconnected before returning a delivery result",
            )
            .with_recovery("Restart the embedding host before retrying graft delivery.")),
        }
    }
}

fn inject_request_enqueue_error(error: TrySendError<InjectRequest>) -> AtmError {
    match error {
        TrySendError::Full(_) => AtmError::new(
            AtmErrorKind::Timeout,
            "graft host nudge worker is still busy past the bounded delivery deadline",
        )
        .with_recovery(
            "Fix or restart the embedding host nudge receiver before retrying graft delivery.",
        ),
        TrySendError::Disconnected(_) => AtmError::new(
            AtmErrorKind::Internal,
            "graft host nudge worker is unavailable",
        )
        .with_recovery("Restart the embedding host before retrying graft delivery."),
    }
}

pub(crate) fn load_graft_config(workspace_root: &Path) -> Result<Option<GraftConfig>, AtmError> {
    let config = atm_core::load_atm_config(workspace_root)?;
    Ok(config.map(|config| config.graft))
}

pub(crate) fn read_snapshot(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
) -> Result<SessionSnapshot, AtmError> {
    snapshot
        .read()
        .map(|snapshot| snapshot.clone())
        .map_err(|_| {
            AtmError::daemon_unavailable("graft session snapshot lock poisoned").with_recovery(
                "Restart the embedding host before retrying graft session lifecycle operations.",
            )
        })
}

fn write_snapshot(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
    state: GraftSessionState,
) -> Result<(), AtmError> {
    let mut snapshot = snapshot.write().map_err(|_| {
        AtmError::daemon_unavailable("graft session snapshot lock poisoned").with_recovery(
            "Restart the embedding host before retrying graft session lifecycle operations.",
        )
    })?;
    snapshot.state = state;
    Ok(())
}

pub(crate) fn set_session_state(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
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
    join_helper.join().map_err(|_| join_helper_panic_error())?;
    result
}

fn handle_join_helper_disconnect(join_helper: JoinHandle<()>) -> Result<(), AtmError> {
    join_helper.join().map_or_else(
        |_| Err(join_helper_panic_error()),
        |_| Err(join_helper_disconnect_error()),
    )
}

fn receive_loop_panic_error() -> AtmError {
    AtmError::daemon_unavailable("graft receiver loop panicked")
        .with_recovery("Restart the embedding host and atm-daemon before retrying graft mode.")
}

fn join_helper_spawn_error(source: std::io::Error) -> AtmError {
    AtmError::daemon_unavailable("failed to spawn graft receive-loop join helper")
        .with_source(source)
        .with_recovery(
            "Retry graft shutdown after the embedding host can spawn one bounded join helper thread.",
        )
}

fn join_helper_panic_error() -> AtmError {
    AtmError::daemon_unavailable("graft receive-loop join helper panicked")
        .with_recovery("Restart the embedding host and atm-daemon before retrying graft mode.")
}

fn join_helper_disconnect_error() -> AtmError {
    AtmError::daemon_unavailable("graft receive-loop join helper disconnected unexpectedly")
        .with_recovery("Restart the embedding host and atm-daemon before retrying graft mode.")
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
    .with_recovery(
        "Restart the embedding host if the graft receive loop does not shut down within the bounded join deadline.",
    )
}

pub(crate) struct GraftReceiverLoopContext {
    pub(crate) endpoint_path: PathBuf,
    pub(crate) snapshot: Arc<RwLock<SessionSnapshot>>,
    pub(crate) injector: Arc<dyn HostNudgeInjector>,
    pub(crate) observability: Arc<dyn GraftObservability>,
    pub(crate) stop_rx: Receiver<()>,
}

pub(crate) fn run_graft_receiver_loop(ctx: GraftReceiverLoopContext) -> Result<(), AtmError> {
    let injector = BoundedHostNudgeInjector::spawn(Arc::clone(&ctx.injector))?;
    let listener = bind_graft_receiver_listener(&ctx.endpoint_path)?;
    loop {
        if stop_requested(&ctx.stop_rx) {
            return Ok(());
        }
        let mut stream = accept_graft_receiver_connection(&listener, &ctx.endpoint_path)?;
        if stop_requested(&ctx.stop_rx) {
            return Ok(());
        }
        handle_graft_receiver_connection(&ctx, &injector, &mut stream)?;
    }
}

pub(crate) fn wake_graft_receiver_listener(endpoint_path: &Path) -> Result<(), AtmError> {
    let name = atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path)?;
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    thread::Builder::new()
        .name("graft-listener-wake-connect".to_string())
        .spawn(move || {
            let _ = result_tx.send(LocalSocketStream::connect(name));
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn graft listener wake worker")
                .with_recovery(
                    "Restart the graft-enabled host; the same-host listener wake path could not create its bounded connect helper.",
                )
                .with_source(source)
        })?;
    match result_rx.recv_timeout(LISTENER_WAKE_CONNECT_DEADLINE) {
        Ok(Ok(_stream)) => Ok(()),
        Ok(Err(source)) => Err(
            AtmError::daemon_unavailable(format!(
                "failed to wake graft receiver listener at {}",
                endpoint_path.display()
            ))
            .with_recovery(
                "Restart the graft-enabled host; the receiver listener could not be nudged out of accept cleanly during shutdown.",
            )
            .with_source(source),
        ),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(
            AtmError::daemon_unavailable(format!(
                "timed out waking graft receiver listener at {}",
                endpoint_path.display()
            ))
            .with_recovery(
                "Restart the graft-enabled host; the listener wake connection exceeded the bounded shutdown budget.",
            ),
        ),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(
            AtmError::daemon_unavailable(format!(
                "graft listener wake worker disconnected unexpectedly for {}",
                endpoint_path.display()
            ))
            .with_recovery(
                "Restart the graft-enabled host; the local IPC wake path aborted before it could connect to the listener.",
            ),
        ),
    }
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
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to bind graft receiver endpoint at {}",
                endpoint_path.display()
            ))
            .with_recovery(
                "Confirm the graft receiver endpoint path is writable and no conflicting graft listener still owns the same-host address before retrying activation.",
            )
            .with_source(source)
        })
}

fn prepare_graft_receiver_endpoint(endpoint_path: &Path) -> Result<(), AtmError> {
    if let Some(parent) = endpoint_path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to prepare graft receiver directory {}",
                parent.display()
            ))
            .with_recovery(
                "Create the graft receiver runtime directory or repair its permissions before retrying activation.",
            )
            .with_source(source)
        })?;
    }
    #[cfg(unix)]
    if endpoint_path.exists() {
        fs::remove_file(endpoint_path).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to remove stale graft receiver endpoint {}",
                endpoint_path.display()
            ))
            .with_recovery(
                "Remove the stale graft receiver socket path or restart the graft-enabled host before retrying activation.",
            )
            .with_source(source)
        })?;
    }
    Ok(())
}

fn accept_graft_receiver_connection(
    listener: &LocalSocketListener,
    endpoint_path: &Path,
) -> Result<LocalSocketStream, AtmError> {
    listener.accept().map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed while accepting graft receiver connection at {}",
            endpoint_path.display()
        ))
        .with_recovery(
            "Restart the graft-enabled host; the same-host graft receiver listener stopped accepting connections unexpectedly.",
        )
        .with_source(source)
    })
}

fn handle_graft_receiver_connection(
    ctx: &GraftReceiverLoopContext,
    injector: &BoundedHostNudgeInjector,
    stream: &mut LocalSocketStream,
) -> Result<(), AtmError> {
    apply_receiver_deadlines(stream)?;
    let request: GraftPostSendRequest = read_graft_post_send_message(
        stream,
        "failed to read graft post-send request",
        "graft post-send request exceeded the bounded payload cap",
    )?;
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
    write_graft_post_send_message(
        stream,
        &response,
        "failed to write graft post-send response",
        "graft post-send response exceeded the bounded payload cap",
    )?;
    use std::io::Write as _;
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush graft post-send response")
            .with_recovery(
                "Restart the graft-enabled host; the direct graft post-send response could not be flushed to the caller.",
            )
            .with_source(source)
    })
}

fn apply_receiver_deadlines(stream: &LocalSocketStream) -> Result<(), AtmError> {
    apply_receiver_deadline(
        stream.set_recv_timeout(Some(GRAFT_RECEIVER_IO_DEADLINE)),
        "failed to apply graft receiver receive timeout",
    )?;
    apply_receiver_deadline(
        stream.set_send_timeout(Some(GRAFT_RECEIVER_IO_DEADLINE)),
        "failed to apply graft receiver send timeout",
    )?;
    Ok(())
}

fn apply_receiver_deadline(
    result: std::io::Result<()>,
    message: &'static str,
) -> Result<(), AtmError> {
    match result {
        Ok(()) => Ok(()),
        #[cfg(windows)]
        Err(source) if source.kind() == std::io::ErrorKind::Unsupported => Ok(()),
        Err(source) => Err(AtmError::daemon_unavailable(message)
            .with_recovery(
                "Restart the graft-enabled host; the graft receiver could not apply its bounded local-socket I/O deadline.",
            )
            .with_source(source)),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex, RwLock};
    use std::time::{Duration, Instant};

    use atm_core::boundary::PostSendHookEvent;
    use atm_core::error::{AtmError, AtmErrorKind};
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::graft::{
        GraftPostSendRequest, GraftPostSendResponse, read_graft_post_send_message,
        write_graft_post_send_message,
    };
    use atm_core::schema::AtmMessageId;
    use atm_core::test_support::{TEST_LEAD, TEST_QA, TEST_TEAM};
    use atm_core::types::{AgentName, TeamName};
    use interprocess::local_socket::Stream as LocalSocketStream;
    use interprocess::local_socket::traits::Stream as _;
    use tempfile::TempDir;

    use crate::{GraftObservability, HostNudgeInjector};

    use super::{
        GraftReceiverLoopContext, bind_graft_receiver_listener, join_receive_loop_with_deadline,
        load_graft_config, read_snapshot, run_graft_receiver_loop, wake_graft_receiver_listener,
    };
    use crate::{GraftSessionState, SessionSnapshot};

    #[derive(Debug, Default)]
    struct RecordingInjector {
        nudges: Mutex<Vec<PostSendHookEvent>>,
    }

    impl HostNudgeInjector for RecordingInjector {
        fn inject_nudge(&self, nudge: PostSendHookEvent) -> Result<(), AtmError> {
            self.nudges.lock().expect("nudges lock").push(nudge);
            Ok(())
        }
    }

    #[derive(Debug)]
    struct FailingInjector;

    impl HostNudgeInjector for FailingInjector {
        fn inject_nudge(&self, _nudge: PostSendHookEvent) -> Result<(), AtmError> {
            Err(AtmError::new_with_code(
                AtmErrorCode::PostSendGraftUnavailable,
                AtmErrorKind::DaemonUnavailable,
                "synthetic graft receiver unavailable",
            )
            .with_recovery("restart the graft host"))
        }
    }

    #[derive(Debug, Default)]
    struct NoopObservability;

    impl GraftObservability for NoopObservability {}

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
        let deadline = Instant::now() + Duration::from_secs(1);
        let name = atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path)
            .expect("receiver ipc name");
        loop {
            match LocalSocketStream::connect(name.clone()) {
                Ok(stream) => return stream,
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                    std::thread::yield_now();
                }
                Err(error) => panic!("failed to connect to graft receiver: {error}"),
            }
        }
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
        };
        let join = std::thread::spawn(move || run_graft_receiver_loop(ctx));
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
    fn receiver_listener_binds_at_expected_endpoint() {
        let paths = test_paths();
        let endpoint_path = receiver_endpoint_path(&paths);
        let listener = bind_graft_receiver_listener(&endpoint_path).expect("bind listener");
        drop(listener);
    }

    #[test]
    fn receiver_loop_delivers_direct_nudge_and_returns_ack() {
        let paths = test_paths();
        let endpoint_path = receiver_endpoint_path(&paths);
        let injector = Arc::new(RecordingInjector::default());
        let (stop_tx, join, snapshot) = spawn_receiver(
            endpoint_path.clone(),
            injector.clone() as Arc<dyn HostNudgeInjector>,
        );

        let request = GraftPostSendRequest {
            event: request_event(),
        };
        let mut stream = connect_receiver(&endpoint_path);
        write_graft_post_send_message(&mut stream, &request, "write request", "oversized request")
            .expect("write request");
        let response: GraftPostSendResponse =
            read_graft_post_send_message(&mut stream, "read response", "oversized response")
                .expect("read response");

        assert_eq!(response, GraftPostSendResponse::Delivered);
        let nudges = injector.nudges.lock().expect("nudges lock");
        assert_eq!(nudges.len(), 1);
        assert_eq!(nudges[0], request.event);
        assert_eq!(
            read_snapshot(&snapshot).expect("snapshot").state,
            GraftSessionState::Listening
        );

        stop_receiver(&endpoint_path, stop_tx, join);
    }

    #[test]
    fn receiver_loop_returns_typed_error_when_injector_fails() {
        let paths = test_paths();
        let endpoint_path = receiver_endpoint_path(&paths);
        let (stop_tx, join, _snapshot) =
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
                assert_eq!(error.code, AtmErrorCode::PostSendGraftUnavailable);
                assert!(
                    error
                        .message
                        .contains("synthetic graft receiver unavailable")
                );
            }
        }

        stop_receiver(&endpoint_path, stop_tx, join);
    }
}
