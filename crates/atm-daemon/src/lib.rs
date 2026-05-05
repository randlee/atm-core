#![forbid(unsafe_code)]
#![allow(dead_code)]

//! Skeleton crate for Phase R daemon runtime work.

pub(crate) mod composition;

use std::error::Error as StdError;
use std::fmt;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::net::UnixListener;
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant};

use atm_core::{
    RequestEnvelope, ResponseEnvelope,
    ack::ack_mail,
    boundary::{
        self, ConfigIngress, ConfigLoadRequest, ConfigLoadResponse, ConfigTeamLoadRequest,
        ConfigTeamLoadResponse, InboxExport, InboxExportRecordRequest, InboxExportRecordResponse,
        InboxExportReexportMessageRequest, InboxExportReexportMessageResponse, InboxIngress,
        InboxIngressDiagnosticsRequest, InboxIngressDiagnosticsResponse,
        InboxIngressIdentityFingerprintRequest, InboxIngressIdentityFingerprintResponse,
        InboxIngressImportRequest, InboxIngressImportResponse, NotificationEvent, ReconcileRequest,
        ReconcileResult, RequestDispatcher, RuntimeStatusSnapshot, WatchEventBatch,
        WatchSubscriptionRequest,
    },
    boundary_support,
    clear::clear_mail,
    doctor::run_doctor,
    error::AtmError,
    observability::{
        AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        CommandEvent, LogTailSession, ObservabilityPort,
    },
    protocol::{
        RequestEnvelope as ProtocolRequestEnvelope, SendRequestEnvelope, SendResponseEnvelope,
    },
    read::read_mail,
    send::send_mail,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DaemonBoundaryStubError {
    ServerTransport,
    RequestDispatcher,
    NotificationSink,
    StatusSource,
    WatchEventSource,
    ReconcileCoordinator,
    ConfigIngress,
    InboxIngress,
    InboxExport,
    PeerClientTransport,
}

impl fmt::Display for DaemonBoundaryStubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ServerTransport => "daemon server transport scaffold is not wired",
            Self::PeerClientTransport => "daemon peer client transport scaffold is not wired",
            Self::RequestDispatcher => "daemon request dispatcher scaffold is not wired",
            Self::NotificationSink => "daemon notification sink scaffold is not wired",
            Self::StatusSource => "daemon status source scaffold is not wired",
            Self::WatchEventSource => "daemon watch event source scaffold is not wired",
            Self::ReconcileCoordinator => "daemon reconcile coordinator scaffold is not wired",
            Self::ConfigIngress => "daemon config ingress scaffold is not wired",
            Self::InboxIngress => "daemon inbox ingress scaffold is not wired",
            Self::InboxExport => "daemon inbox export scaffold is not wired",
        };

        f.write_str(message)
    }
}

impl StdError for DaemonBoundaryStubError {}

fn daemon_boundary_stub_error(message: &'static str, source: DaemonBoundaryStubError) -> AtmError {
    AtmError::config(message)
        .with_recovery("Complete the Phase R daemon boundary wiring before invoking this path.")
        .with_source(source)
}

#[derive(Debug, Clone, Copy, Default)]
struct DaemonObservability;

impl boundary::sealed::Sealed for DaemonObservability {}

impl ObservabilityPort for DaemonObservability {
    fn emit(&self, _event: CommandEvent) -> Result<(), AtmError> {
        Ok(())
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Ok(AtmLogSnapshot::default())
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Ok(LogTailSession::empty())
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        let active_log_path = atm_core::home::atm_home()?
            .join(".local")
            .join("share")
            .join("logs")
            .join("atm.log.jsonl");
        let fault = std::env::var("ATM_OBSERVABILITY_RETAINED_SINK_FAULT")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase());
        let logging_state = match fault.as_deref() {
            Some("degraded") => AtmObservabilityHealthState::Degraded,
            Some("unavailable") => AtmObservabilityHealthState::Unavailable,
            _ => AtmObservabilityHealthState::Healthy,
        };
        Ok(AtmObservabilityHealth {
            active_log_path: Some(active_log_path),
            logging_state,
            query_state: Some(AtmObservabilityHealthState::Healthy),
            detail: None,
        })
    }
}

/// Placeholder runtime transport for the daemon server boundary.
#[derive(Debug, Default)]
pub(crate) struct LocalSocketServerTransport;

impl LocalSocketServerTransport {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for LocalSocketServerTransport {}

impl boundary::ServerTransport for LocalSocketServerTransport {
    #[cfg(unix)]
    fn serve(&self, _dispatcher: &dyn RequestDispatcher) -> Result<(), AtmError> {
        let socket_path = atm_core::protocol::daemon_socket_path()?;
        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to create daemon socket directory at {}",
                    parent.display()
                ))
                .with_source(source)
            })?;
        }
        if socket_path.exists() {
            fs::remove_file(&socket_path).map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to replace stale daemon socket at {}",
                    socket_path.display()
                ))
                .with_source(source)
            })?;
        }

        struct SocketGuard<'a>(&'a std::path::Path);
        impl Drop for SocketGuard<'_> {
            fn drop(&mut self) {
                let _ = fs::remove_file(self.0);
            }
        }

        let _guard = SocketGuard(&socket_path);
        let listener = UnixListener::bind(&socket_path).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to bind daemon socket at {}",
                socket_path.display()
            ))
            .with_source(source)
        })?;
        listener.set_nonblocking(true).map_err(|source| {
            AtmError::daemon_unavailable("failed to configure daemon socket listener")
                .with_source(source)
        })?;

        let idle_timeout = Duration::from_millis(250);
        let poll_interval = Duration::from_millis(25);
        let mut last_activity = Instant::now();
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    last_activity = Instant::now();
                    let mut bytes = Vec::new();
                    stream.read_to_end(&mut bytes).map_err(|source| {
                        AtmError::daemon_unavailable("failed to read daemon request frame")
                            .with_source(source)
                    })?;
                    if bytes.is_empty() {
                        continue;
                    }
                    let request: ProtocolRequestEnvelope =
                        serde_json::from_slice(&bytes).map_err(AtmError::from)?;
                    let response = _dispatcher.dispatch(request)?;
                    let encoded = serde_json::to_vec(&response).map_err(AtmError::from)?;
                    stream.write_all(&encoded).map_err(|source| {
                        AtmError::daemon_unavailable("failed to write daemon response frame")
                            .with_source(source)
                    })?;
                    stream.flush().map_err(|source| {
                        AtmError::daemon_unavailable("failed to flush daemon response frame")
                            .with_source(source)
                    })?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if last_activity.elapsed() >= idle_timeout {
                        return Ok(());
                    }
                    thread::sleep(poll_interval);
                }
                Err(source) => {
                    return Err(AtmError::daemon_unavailable(
                        "failed while accepting daemon connection",
                    )
                    .with_source(source));
                }
            }
        }
    }

    #[cfg(not(unix))]
    fn serve(&self, _dispatcher: &dyn RequestDispatcher) -> Result<(), AtmError> {
        Err(AtmError::daemon_unavailable(
            "atm-daemon socket transport requires a Unix platform",
        ))
    }
}

/// Placeholder runtime dispatcher for daemon-owned protocol routing.
#[derive(Debug, Default)]
struct DaemonRequestDispatcher;

impl DaemonRequestDispatcher {
    const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonRequestDispatcher {}

impl boundary::RequestDispatcher for DaemonRequestDispatcher {
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let observability = DaemonObservability;
        match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(
                    send_mail(request, &observability)?,
                )))
            }
            RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(
                    ack_mail(request, &observability)?,
                )))
            }
            RequestEnvelope::Receive(query) => {
                Ok(ResponseEnvelope::Receive(read_mail(query, &observability)?))
            }
            RequestEnvelope::Clear(query) => {
                Ok(ResponseEnvelope::Clear(clear_mail(query, &observability)?))
            }
            RequestEnvelope::Doctor(query) => {
                Ok(ResponseEnvelope::Doctor(run_doctor(query, &observability)?))
            }
        }
    }
}

/// Placeholder runtime sink for daemon-emitted notifications.
#[derive(Debug, Default)]
pub(crate) struct DaemonNotificationSink;

impl DaemonNotificationSink {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonNotificationSink {}

impl boundary::NotificationSink for DaemonNotificationSink {
    fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError> {
        boundary_support::deliver_notification(event)
    }
}

/// Placeholder runtime client transport for peer-to-peer daemon delivery.
#[derive(Debug, Default)]
struct PeerClientTransport;

impl PeerClientTransport {
    const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for PeerClientTransport {}

impl boundary::ClientTransport for PeerClientTransport {
    fn send(&self, _request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon peer client transport stub is not implemented yet",
            DaemonBoundaryStubError::PeerClientTransport,
        ))
    }
}

/// Placeholder runtime source for daemon status snapshots.
#[derive(Debug, Default)]
pub(crate) struct DaemonStatusSource;

impl DaemonStatusSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonStatusSource {}

impl boundary::StatusSource for DaemonStatusSource {
    fn snapshot(&self) -> Result<RuntimeStatusSnapshot, AtmError> {
        boundary_support::snapshot_status()
    }
}

/// Placeholder runtime source for daemon watch events.
#[derive(Debug, Default)]
pub(crate) struct FileWatchEventSource;

impl FileWatchEventSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for FileWatchEventSource {}

impl boundary::WatchEventSource for FileWatchEventSource {
    fn poll(&self, request: WatchSubscriptionRequest) -> Result<WatchEventBatch, AtmError> {
        boundary_support::poll_watch(request)
    }
}

/// Placeholder runtime coordinator for daemon reconcile work.
#[derive(Debug, Default)]
pub(crate) struct DaemonReconcileCoordinator;

impl DaemonReconcileCoordinator {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonReconcileCoordinator {}

impl boundary::ReconcileCoordinator for DaemonReconcileCoordinator {
    fn reconcile(&self, request: ReconcileRequest) -> Result<ReconcileResult, AtmError> {
        let batch = boundary_support::poll_watch(WatchSubscriptionRequest {
            home_dir: request.home_dir.clone(),
            team: request.team.clone(),
            agent: request.agent.clone(),
        })?;
        let ingress = DaemonInboxIngress::new();
        let import = ingress.import_inbox_source(InboxIngressImportRequest {
            home_dir: request.home_dir,
            team: request.team,
            agent: request.agent,
        })?;
        Ok(ReconcileResult {
            observed_paths: batch.paths.len(),
            imported_sources: import.source_files.len(),
        })
    }
}

/// Placeholder runtime config ingress for daemon-owned config loading.
#[derive(Debug, Default)]
pub(crate) struct DaemonConfigIngress;

impl DaemonConfigIngress {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonConfigIngress {}

impl ConfigIngress for DaemonConfigIngress {
    fn load_config(&self, request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError> {
        boundary_support::load_workspace_config(request)
    }

    fn load_team_config(
        &self,
        request: ConfigTeamLoadRequest,
    ) -> Result<ConfigTeamLoadResponse, AtmError> {
        boundary_support::load_team_config(request)
    }
}

/// Placeholder runtime inbox ingress for daemon-owned import workflows.
#[derive(Debug, Default)]
pub(crate) struct DaemonInboxIngress;

impl DaemonInboxIngress {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonInboxIngress {}

impl InboxIngress for DaemonInboxIngress {
    fn import_inbox_source(
        &self,
        request: InboxIngressImportRequest,
    ) -> Result<InboxIngressImportResponse, AtmError> {
        boundary_support::import_inbox_source(request)
    }

    fn compute_identity_fingerprint(
        &self,
        request: InboxIngressIdentityFingerprintRequest,
    ) -> Result<InboxIngressIdentityFingerprintResponse, AtmError> {
        boundary_support::compute_identity_fingerprint(request)
    }

    fn report_diagnostics(
        &self,
        request: InboxIngressDiagnosticsRequest,
    ) -> Result<InboxIngressDiagnosticsResponse, AtmError> {
        boundary_support::report_inbox_diagnostics(request)
    }
}

/// Placeholder runtime inbox export for daemon-owned export workflows.
#[derive(Debug, Default)]
pub(crate) struct DaemonInboxExport;

impl DaemonInboxExport {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonInboxExport {}

impl InboxExport for DaemonInboxExport {
    fn export_record(
        &self,
        request: InboxExportRecordRequest,
    ) -> Result<InboxExportRecordResponse, AtmError> {
        boundary_support::export_source_files(request)
    }

    fn reexport_message(
        &self,
        request: InboxExportReexportMessageRequest,
    ) -> Result<InboxExportReexportMessageResponse, AtmError> {
        boundary_support::reexport_messages(request)
    }
}

/// Run the daemon entrypoint with the currently assembled runtime composition.
///
/// # Errors
///
/// Returns [`AtmError`] when the daemon transport cannot start or serve.
pub fn run_daemon() -> Result<(), AtmError> {
    composition::compose_runtime().serve()
}
