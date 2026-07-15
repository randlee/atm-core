use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use atm_core::boundary::{AtmProtocol, RequestDispatcher};
use atm_core::error::AtmError;
use atm_core::protocol::{JsonAtmProtocolCodec, RequestEnvelope, ResponseEnvelope};

use crate::SubsystemObservability;
use crate::active_connection_registry::{ActiveConnectionRegistry, TrackedDispatchHandle};
use crate::runtime_status_cache::RuntimeStatusCache;

use super::{
    MAX_CONCURRENT_PEER_CONNECTIONS, MAX_TRACKED_PEER_DISPATCH_HANDLES,
    PEER_ACCEPT_ERROR_RETRY_BACKOFF, PEER_CONNECTION_IO_SLICE, PEER_LISTENER_ACCEPT_POLL_INTERVAL,
    PEER_LISTENER_SHUTDOWN_DEADLINE, PEER_REQUEST_DEADLINE,
};

#[derive(Debug)]
struct PeerServerHandle {
    terminate: Arc<AtomicBool>,
    join_handle: std::thread::JoinHandle<Result<(), AtmError>>,
    #[cfg_attr(not(test), allow(dead_code))]
    bound_addr: SocketAddr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerListenerOutcome {
    pub(crate) listen_addr: SocketAddr,
    pub(crate) bound_addr: Option<SocketAddr>,
    pub(crate) error_message: Option<String>,
}

#[derive(Debug)]
pub(super) struct PeerServerTransport {
    listen_addrs: Mutex<Vec<SocketAddr>>,
    observability: SubsystemObservability,
    state: Mutex<BTreeMap<SocketAddr, PeerServerHandle>>,
    status_cache: RuntimeStatusCache,
}

impl PeerServerTransport {
    pub(super) fn new(
        listen_addr: Option<SocketAddr>,
        observability: SubsystemObservability,
        status_cache: RuntimeStatusCache,
    ) -> Self {
        Self {
            listen_addrs: Mutex::new(listen_addr.into_iter().collect()),
            observability,
            state: Mutex::new(BTreeMap::new()),
            status_cache,
        }
    }

    #[allow(
        dead_code,
        reason = "retained for transitional peer-runtime entrypoints"
    )]
    pub(super) fn start(
        &self,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    ) -> Result<Vec<PeerListenerOutcome>, AtmError> {
        let listen_addrs = self.configured_listen_addrs()?;
        self.reload(listen_addrs, dispatcher)
    }

    pub(super) fn shutdown(&self) -> Result<(), AtmError> {
        let handles = self
            .state
            .lock()
            .map_err(|_| {
                AtmError::daemon_unavailable("peer listener state lock poisoned")
                    .with_recovery("Restart atm-daemon before retrying cross-host peer shutdown.")
            })?
            .values()
            .map(|handle| handle.bound_addr)
            .collect::<Vec<_>>();
        if handles.is_empty() {
            return Ok(());
        }
        let drained = {
            let mut state = self.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("peer listener state lock poisoned")
                    .with_recovery("Restart atm-daemon before retrying cross-host peer shutdown.")
            })?;
            std::mem::take(&mut *state)
                .into_iter()
                .map(|(_, handle)| handle)
                .collect::<Vec<_>>()
        };
        for handle in drained {
            shutdown_listener_handle(handle)?;
        }
        self.status_cache.clear_peer_listener_degraded();
        Ok(())
    }

    pub(super) fn reload(
        &self,
        listen_addrs: Vec<SocketAddr>,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    ) -> Result<Vec<PeerListenerOutcome>, AtmError> {
        {
            let mut configured = self.listen_addrs.lock().map_err(|_| {
                AtmError::daemon_unavailable("peer listener config lock poisoned").with_recovery(
                    "Restart atm-daemon before retrying cross-host peer listener reload.",
                )
            })?;
            *configured = listen_addrs.clone();
        }

        let desired = listen_addrs
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let mut state = self.state.lock().map_err(|_| {
            AtmError::daemon_unavailable("peer listener state lock poisoned").with_recovery(
                "Restart atm-daemon before retrying cross-host peer listener reload.",
            )
        })?;

        let stale_addrs = state
            .keys()
            .copied()
            .filter(|addr| !desired.contains(addr))
            .collect::<Vec<_>>();
        for stale_addr in stale_addrs {
            if let Some(handle) = state.remove(&stale_addr) {
                shutdown_listener_handle(handle)?;
            }
        }

        let mut outcomes = Vec::new();
        let mut failures = Vec::new();
        for listen_addr in listen_addrs {
            if let Some(handle) = state.get(&listen_addr) {
                outcomes.push(PeerListenerOutcome {
                    listen_addr,
                    bound_addr: Some(handle.bound_addr),
                    error_message: None,
                });
                continue;
            }
            match self.bind_and_spawn_listener(listen_addr, dispatcher.clone()) {
                Ok((bound_addr, handle)) => {
                    self.observability.emit_or_warn(
                        "peer_listener_start",
                        "ok",
                        format!("daemon peer listener bound at {bound_addr}"),
                    );
                    state.insert(listen_addr, handle);
                    outcomes.push(PeerListenerOutcome {
                        listen_addr,
                        bound_addr: Some(bound_addr),
                        error_message: None,
                    });
                }
                Err(error) => {
                    tracing::warn!(
                        subsystem = "peer_transport",
                        action = "reload_listener",
                        outcome = "degraded",
                        %listen_addr,
                        %error,
                        "daemon peer listener reload failed to rebind the configured address"
                    );
                    self.observability.emit_or_warn(
                        "peer_listener_reload",
                        "degraded",
                        format!("daemon peer listener reload failed for {listen_addr}"),
                    );
                    self.record_degraded(listen_addr, &error);
                    failures.push(format!("{listen_addr}: {}", error.message));
                    outcomes.push(PeerListenerOutcome {
                        listen_addr,
                        bound_addr: None,
                        error_message: Some(error.message.clone()),
                    });
                }
            }
        }

        if failures.is_empty() {
            self.status_cache.clear_peer_listener_degraded();
        } else {
            self.status_cache.record_peer_listener_degraded(format!(
                "daemon peer listener reload has degraded rows: {}",
                failures.join("; ")
            ));
        }

        Ok(outcomes)
    }

    pub(super) fn bound_addr(&self) -> Result<Option<SocketAddr>, AtmError> {
        self.state
            .lock()
            .map(|state| state.values().next().map(|handle| handle.bound_addr))
            .map_err(|_| {
                AtmError::daemon_unavailable("peer listener state lock poisoned")
                    .with_recovery("Restart atm-daemon before retrying cross-host peer inspection.")
            })
    }

    #[cfg(test)]
    pub(super) fn bound_addr_for_test(&self) -> Option<SocketAddr> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.values().next().map(|handle| handle.bound_addr))
    }

    fn record_degraded(&self, listen_addr: SocketAddr, error: &AtmError) {
        self.status_cache.record_peer_listener_degraded(format!(
            "daemon peer listener at {listen_addr} is degraded: {}",
            error.message
        ));
    }

    #[allow(
        dead_code,
        reason = "used by the retained single-entry listener start path"
    )]
    fn configured_listen_addrs(&self) -> Result<Vec<SocketAddr>, AtmError> {
        self.listen_addrs
            .lock()
            .map(|addr| addr.clone())
            .map_err(|_| {
                AtmError::daemon_unavailable("peer listener config lock poisoned")
                    .with_recovery("Restart atm-daemon before retrying cross-host peer startup.")
            })
    }

    fn bind_and_spawn_listener(
        &self,
        listen_addr: SocketAddr,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    ) -> Result<(SocketAddr, PeerServerHandle), AtmError> {
        let listener = self.bind_listener(listen_addr)?;
        let bound_addr = self.bound_listener_addr(&listener, listen_addr)?;
        let handle = self.spawn_listener_thread(listener, listen_addr, dispatcher, bound_addr)?;
        Ok((bound_addr, handle))
    }

    fn bind_listener(&self, listen_addr: SocketAddr) -> Result<TcpListener, AtmError> {
        let listener = TcpListener::bind(listen_addr).map_err(|source| {
            let error = AtmError::daemon_unavailable(format!(
                "failed to bind daemon peer listener at {listen_addr}"
            ))
            .with_recovery(
                "Choose an available literal IP:port for `atm daemon interfaces add ...` or update the existing daemon interface row before restarting atm-daemon.",
            )
            .with_source(source);
            self.record_degraded(listen_addr, &error);
            error
        })?;
        listener.set_nonblocking(true).map_err(|source| {
            let error =
                AtmError::daemon_unavailable("failed to configure daemon peer listener as nonblocking")
                    .with_recovery(
                        "Restart atm-daemon after confirming the host allows nonblocking TCP listeners for cross-host peer transport.",
                    )
                    .with_source(source);
            self.record_degraded(listen_addr, &error);
            error
        })?;
        Ok(listener)
    }

    fn bound_listener_addr(
        &self,
        listener: &TcpListener,
        listen_addr: SocketAddr,
    ) -> Result<SocketAddr, AtmError> {
        listener.local_addr().map_err(|source| {
            let error = AtmError::daemon_unavailable(
                "failed to resolve daemon peer listener bound address",
            )
            .with_recovery(
                "Restart atm-daemon after confirming the configured daemon interface row is valid on this host.",
            )
            .with_source(source);
            self.record_degraded(listen_addr, &error);
            error
        })
    }

    fn spawn_listener_thread(
        &self,
        listener: TcpListener,
        listen_addr: SocketAddr,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
        bound_addr: SocketAddr,
    ) -> Result<PeerServerHandle, AtmError> {
        let terminate = Arc::new(AtomicBool::new(false));
        let terminate_for_thread = Arc::clone(&terminate);
        let observability = self.observability.clone();
        let status_cache = self.status_cache.clone();
        let join_handle = thread::Builder::new()
            .name("peer-transport-listener".to_string())
            .spawn(move || {
                let result = serve_peer_listener(
                    listener,
                    dispatcher,
                    terminate_for_thread,
                    observability.clone(),
                );
                if let Err(error) = &result {
                    status_cache.record_peer_listener_degraded(format!(
                        "daemon peer listener at {listen_addr} is degraded: {}",
                        error.message
                    ));
                }
                result
            })
            .map_err(|source| {
                let error = AtmError::daemon_unavailable("failed to spawn daemon peer listener thread")
                    .with_recovery(
                        "Restart atm-daemon after confirming the host can create the cross-host peer listener worker.",
                    )
                    .with_source(source);
                self.record_degraded(listen_addr, &error);
                error
            })?;
        Ok(PeerServerHandle {
            terminate,
            join_handle,
            bound_addr,
        })
    }
}

fn shutdown_listener_handle(handle: PeerServerHandle) -> Result<(), AtmError> {
    handle.terminate.store(true, Ordering::SeqCst);
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("peer-transport-join-helper".to_string())
        .spawn(move || {
            let result = match handle.join_handle.join() {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(AtmError::daemon_unavailable(
                    "daemon peer listener thread panicked unexpectedly",
                )
                .with_recovery("Restart atm-daemon before retrying cross-host peer transport.")),
            };
            let _ = result_tx.send(result);
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn peer listener join helper")
                .with_recovery(
                    "Restart atm-daemon before retrying cross-host peer listener shutdown.",
                )
                .with_source(source)
        })?;
    match result_rx.recv_timeout(PEER_LISTENER_SHUTDOWN_DEADLINE) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(AtmError::daemon_unavailable(
            "daemon peer listener shutdown exceeded the bounded join deadline",
        )
        .with_recovery(
            "Restart atm-daemon after confirming no inbound peer connection is stalled beyond the bounded deadline.",
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(AtmError::daemon_unavailable(
            "daemon peer listener join helper disconnected unexpectedly",
        )
        .with_recovery("Restart atm-daemon before retrying cross-host peer listener shutdown.")),
    }
}

fn serve_peer_listener(
    listener: TcpListener,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    terminate: Arc<AtomicBool>,
    observability: SubsystemObservability,
) -> Result<(), AtmError> {
    let codec = JsonAtmProtocolCodec;
    let registry = Arc::new(ActiveConnectionRegistry::default());
    thread::scope(|scope| {
        while !terminate.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _peer_addr)) => spawn_peer_connection_worker(
                    scope,
                    stream,
                    &dispatcher,
                    &terminate,
                    &observability,
                    &codec,
                    &registry,
                ),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(PEER_LISTENER_ACCEPT_POLL_INTERVAL);
                }
                Err(error) => handle_peer_accept_error(&observability, &error),
            }
        }
        registry.interrupt_all();
        registry.join_tracked_dispatches(PEER_REQUEST_DEADLINE)
    })
}

fn spawn_peer_connection_worker<'scope, 'env>(
    scope: &'scope thread::Scope<'scope, 'env>,
    stream: TcpStream,
    dispatcher: &Arc<dyn RequestDispatcher + Send + Sync>,
    terminate: &Arc<AtomicBool>,
    observability: &SubsystemObservability,
    codec: &JsonAtmProtocolCodec,
    registry: &Arc<ActiveConnectionRegistry>,
) {
    if registry.active_connections() >= MAX_CONCURRENT_PEER_CONNECTIONS {
        observability.emit_or_warn(
            "peer_listener_capacity",
            "degraded",
            "daemon peer listener rejected a connection at the bounded concurrency cap",
        );
        return;
    }

    let dispatcher = Arc::clone(dispatcher);
    let terminate = Arc::clone(terminate);
    let observability = observability.clone();
    let codec = codec.clone();
    let registry = Arc::clone(registry);
    let connection_guard = registry.register();
    scope.spawn(move || {
        let _connection_guard = connection_guard;
        let result = catch_unwind(AssertUnwindSafe(|| {
            handle_peer_connection(stream, dispatcher, &codec, terminate, registry)
        }));
        report_peer_connection_result(&observability, result);
    });
}

fn handle_peer_accept_error(observability: &SubsystemObservability, error: &io::Error) {
    tracing::error!(
        subsystem = "peer_transport",
        action = "accept_connection",
        outcome = "retrying",
        kind = ?error.kind(),
        %error,
        "daemon peer listener accept failed; keeping the listener alive and retrying"
    );
    observability.emit_or_warn(
        "peer_listener_accept",
        "degraded",
        "daemon peer listener accept failed; keeping the listener alive and retrying",
    );
    thread::sleep(PEER_ACCEPT_ERROR_RETRY_BACKOFF);
}

fn report_peer_connection_result(
    observability: &SubsystemObservability,
    result: std::thread::Result<Result<(), AtmError>>,
) {
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            emit_peer_connection_failure_event(observability, &error, None, "peer_connection");
        }
        Err(_) => {
            let error = AtmError::daemon_unavailable(
                "daemon peer listener worker panicked while handling a peer connection",
            )
            .with_recovery(
                "Restart atm-daemon after reviewing the retained log for the panicking peer request.",
            );
            emit_peer_connection_failure_event(observability, &error, None, "peer_connection");
        }
    }
}

fn handle_peer_connection(
    mut stream: TcpStream,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    codec: &JsonAtmProtocolCodec,
    terminate: Arc<AtomicBool>,
    registry: Arc<ActiveConnectionRegistry>,
) -> Result<(), AtmError> {
    let deadline = Instant::now() + PEER_REQUEST_DEADLINE;
    let Some(header_bytes) = read_peer_frame_header_until(
        &mut stream,
        deadline,
        terminate.as_ref(),
        "failed to read remote peer request frame",
    )?
    else {
        return Ok(());
    };
    let header = atm_core::protocol::decode_frame_header(
        header_bytes,
        "remote peer request frame exceeded the maximum supported size",
    )?;
    let frame = read_peer_frame_payload_until(
        &mut stream,
        header,
        deadline,
        terminate.as_ref(),
        "failed to read remote peer request frame",
    )?;
    let request_id = frame.request_id;
    let (_, request) = codec.request_from_frame(frame)?;
    let response = dispatch_peer_request(request, dispatcher, registry, deadline)?;
    let response_frame = codec.response_to_frame(request_id, response)?;
    write_peer_frame_until(
        &mut stream,
        &response_frame,
        deadline,
        terminate.as_ref(),
        "failed to write remote peer response frame",
    )?;
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush remote peer response frame")
            .with_recovery("Retry after the remote daemon reconnects to the peer listener.")
            .with_source(source)
    })?;
    Ok(())
}

fn dispatch_peer_request(
    request: RequestEnvelope,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    registry: Arc<ActiveConnectionRegistry>,
    deadline: Instant,
) -> Result<ResponseEnvelope, AtmError> {
    let (result_tx, result_rx): (SyncSender<Result<ResponseEnvelope, AtmError>>, Receiver<_>) =
        mpsc::sync_channel(1);
    let (completion_tx, completion_rx) = mpsc::sync_channel(1);
    let dispatch_registry = Arc::clone(&registry);
    let join_handle = thread::Builder::new()
        .name("peer-transport-dispatch".to_string())
        .spawn(move || {
            let _dispatch = dispatch_registry.register_dispatch_work();
            let result = catch_unwind(AssertUnwindSafe(|| dispatcher.dispatch(request)));
            let _ = match result {
                Ok(result) => result_tx.send(result),
                Err(_) => result_tx.send(Err(AtmError::daemon_unavailable(
                    "daemon peer dispatch worker panicked unexpectedly",
                )
                .with_recovery(
                    "Restart atm-daemon after confirming the destination daemon can complete peer request dispatches safely.",
                ))),
            };
            let _ = completion_tx.send(());
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn daemon peer dispatch worker")
                .with_recovery(
                    "Restart atm-daemon after confirming the host can create cross-host peer dispatch workers.",
                )
                .with_source(source)
        })?;
    registry.push_dispatch_handle(
        TrackedDispatchHandle {
            completion_rx,
            join_handle,
        },
        MAX_TRACKED_PEER_DISPATCH_HANDLES,
    )?;
    let remaining = deadline.checked_duration_since(Instant::now()).ok_or_else(|| {
        AtmError::daemon_unavailable("remote peer connection exceeded the bounded lifetime deadline")
            .with_recovery(
                "Retry the cross-host peer operation after confirming the destination daemon is healthy and not stalled on request handling.",
            )
    })?;
    match result_rx.recv_timeout(remaining) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(AtmError::daemon_unavailable(
            "remote peer request dispatch exceeded the bounded daemon deadline",
        )
        .with_recovery(
            "Retry the cross-host peer operation after confirming the destination daemon is healthy and not stalled on request dispatch.",
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(
            AtmError::daemon_unavailable("daemon peer dispatch worker disconnected unexpectedly")
                .with_recovery(
                    "Restart atm-daemon after confirming the destination daemon can complete peer request dispatches.",
                ),
        ),
    }
}

fn emit_peer_connection_failure_event(
    observability: &SubsystemObservability,
    error: &AtmError,
    request_id: Option<atm_core::protocol::RequestId>,
    transport_context: &'static str,
) {
    let classification = if error.is_validation() {
        atm_core::observability::ConnectionFailureClassification::MalformedRequest
    } else if error.is_daemon_unavailable() || error.is_timeout() {
        atm_core::observability::ConnectionFailureClassification::TransportFailure
    } else {
        atm_core::observability::ConnectionFailureClassification::RequestFailure
    };
    observability.emit_event_or_warn(
        observability
            .event(
                "peer_connection_worker",
                classification.as_str(),
                error.message.clone(),
            )
            .with_connection_failure(atm_core::observability::DaemonConnectionFailureFields {
                code: error.code,
                request_id,
                classification,
            })
            .with_transport_context(transport_context),
    );
}

fn read_peer_frame_header_until(
    stream: &mut TcpStream,
    deadline: Instant,
    terminate: &AtomicBool,
    read_error: &'static str,
) -> Result<Option<[u8; atm_core::protocol::ATM_FRAME_HEADER_BYTES]>, AtmError> {
    let mut header = [0u8; atm_core::protocol::ATM_FRAME_HEADER_BYTES];
    let read = read_with_deadline(
        stream,
        &mut header[..1],
        deadline,
        terminate,
        read_error,
        true,
    )?;
    if read == 0 {
        return Ok(None);
    }
    read_exact_with_deadline(stream, &mut header[1..], deadline, terminate, read_error)?;
    Ok(Some(header))
}

fn read_peer_frame_payload_until(
    stream: &mut TcpStream,
    header: atm_core::protocol::FrameHeader,
    deadline: Instant,
    terminate: &AtomicBool,
    read_error: &'static str,
) -> Result<atm_core::protocol::FramePayload, AtmError> {
    let mut bytes = vec![0u8; header.payload_length];
    read_exact_with_deadline(stream, &mut bytes, deadline, terminate, read_error)?;
    Ok(atm_core::protocol::FramePayload {
        request_id: header.request_id,
        message_kind: header.message_kind,
        flags: header.flags,
        bytes,
    })
}

fn write_peer_frame_until(
    stream: &mut TcpStream,
    frame: &atm_core::protocol::FramePayload,
    deadline: Instant,
    terminate: &AtomicBool,
    write_error: &'static str,
) -> Result<(), AtmError> {
    if frame.flags != atm_core::protocol::ATM_FRAME_FLAGS_V1 {
        return Err(AtmError::validation(format!(
            "unsupported ATM daemon frame flags 0x{:04x} for version {}",
            frame.flags,
            atm_core::protocol::ATM_FRAME_VERSION_V1
        ))
        .with_recovery(
            "Retry with a supported ATM daemon client/server build that uses protocol version 1 flags.",
        ));
    }
    if frame.bytes.len() > atm_core::protocol::MAX_DAEMON_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(
            "daemon frame exceeded the maximum supported size",
        )
        .with_recovery(
            "Reduce the daemon request/response payload size before retrying the ATM command.",
        ));
    }
    let mut header = [0u8; atm_core::protocol::ATM_FRAME_HEADER_BYTES];
    header[0..4].copy_from_slice(&atm_core::protocol::ATM_FRAME_MAGIC.to_be_bytes());
    header[4..6].copy_from_slice(&atm_core::protocol::ATM_FRAME_VERSION_V1.to_be_bytes());
    header[6..8].copy_from_slice(&frame.message_kind.code().to_be_bytes());
    header[8..10].copy_from_slice(&frame.flags.to_be_bytes());
    header[10..18].copy_from_slice(&frame.request_id.into_inner().to_be_bytes());
    header[18..22].copy_from_slice(&(frame.bytes.len() as u32).to_be_bytes());
    write_all_with_deadline(stream, &header, deadline, terminate, write_error)?;
    write_all_with_deadline(stream, &frame.bytes, deadline, terminate, write_error)
}

fn read_exact_with_deadline(
    stream: &mut TcpStream,
    mut buffer: &mut [u8],
    deadline: Instant,
    terminate: &AtomicBool,
    read_error: &'static str,
) -> Result<(), AtmError> {
    while !buffer.is_empty() {
        let read = read_with_deadline(stream, buffer, deadline, terminate, read_error, false)?;
        if read == 0 {
            return Err(AtmError::daemon_unavailable(read_error).with_recovery(
                "Retry after the remote daemon reconnects and completes a bounded peer request/response exchange.",
            ));
        }
        let (_, rest) = buffer.split_at_mut(read);
        buffer = rest;
    }
    Ok(())
}

fn write_all_with_deadline(
    stream: &mut TcpStream,
    mut buffer: &[u8],
    deadline: Instant,
    terminate: &AtomicBool,
    write_error: &'static str,
) -> Result<(), AtmError> {
    while !buffer.is_empty() {
        apply_peer_io_slice_deadline(stream, deadline, terminate)?;
        match stream.write(buffer) {
            Ok(0) => {
                return Err(AtmError::daemon_unavailable(write_error).with_recovery(
                    "Retry after the remote daemon reconnects and completes a bounded peer request/response exchange.",
                ));
            }
            Ok(written) => buffer = &buffer[written..],
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(source) => {
                return Err(AtmError::daemon_unavailable(write_error)
                    .with_recovery("Retry after the remote daemon reconnects to the peer listener.")
                    .with_source(source));
            }
        }
    }
    Ok(())
}

fn read_with_deadline(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    deadline: Instant,
    terminate: &AtomicBool,
    read_error: &'static str,
    allow_eof: bool,
) -> Result<usize, AtmError> {
    loop {
        apply_peer_io_slice_deadline(stream, deadline, terminate)?;
        match stream.read(buffer) {
            Ok(0) if allow_eof => return Ok(0),
            Ok(0) => {
                return Err(AtmError::daemon_unavailable(read_error).with_recovery(
                    "Retry after the remote daemon reconnects and completes a bounded peer request/response exchange.",
                ));
            }
            Ok(read) => return Ok(read),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(source) => {
                return Err(AtmError::daemon_unavailable(read_error)
                    .with_recovery("Retry after the remote daemon reconnects to the peer listener.")
                    .with_source(source));
            }
        }
    }
}

fn apply_peer_io_slice_deadline(
    stream: &TcpStream,
    deadline: Instant,
    terminate: &AtomicBool,
) -> Result<(), AtmError> {
    if terminate.load(Ordering::SeqCst) {
        return Err(AtmError::daemon_unavailable(
            "daemon shutdown interrupted an in-flight peer connection",
        )
        .with_recovery(
            "Retry the cross-host operation after atm-daemon restarts and resumes pending remote replay work.",
        ));
    }
    let remaining = deadline.checked_duration_since(Instant::now()).ok_or_else(|| {
        AtmError::daemon_unavailable("remote peer connection exceeded the bounded lifetime deadline")
            .with_recovery(
                "Retry the cross-host peer operation after confirming the destination daemon is healthy and not stalled on request handling.",
            )
    })?;
    let slice = remaining.min(PEER_CONNECTION_IO_SLICE);
    stream.set_read_timeout(Some(slice)).map_err(|source| {
        AtmError::daemon_unavailable("failed to apply remote peer read deadline")
            .with_recovery(
                "Restart atm-daemon after confirming the host permits bounded TCP read deadlines for peer transport.",
            )
            .with_source(source)
    })?;
    stream.set_write_timeout(Some(slice)).map_err(|source| {
        AtmError::daemon_unavailable("failed to apply remote peer write deadline")
            .with_recovery(
                "Restart atm-daemon after confirming the host permits bounded TCP write deadlines for peer transport.",
            )
            .with_source(source)
    })?;
    Ok(())
}
