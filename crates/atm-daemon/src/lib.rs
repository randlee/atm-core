#![forbid(unsafe_code)]
#![allow(dead_code)]
//! Skeleton crate for Phase R daemon runtime work.

pub(crate) mod composition;

use std::collections::HashSet;
use std::error::Error as StdError;
use std::fmt;
use std::fs;
#[cfg(unix)]
use std::io::Read;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
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
    schema::{MessageEnvelope, TeamConfig},
    send::send_mail,
};
use uuid::Uuid;

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

fn team_dir_from_home(home_dir: &Path, team: &str) -> Result<PathBuf, AtmError> {
    atm_core::home::team_dir_from_home(home_dir, team)
}

fn inbox_path_from_home(home_dir: &Path, team: &str, agent: &str) -> Result<PathBuf, AtmError> {
    atm_core::home::inbox_path_from_home(home_dir, team, agent)
}

fn read_team_config(path: &Path) -> Result<TeamConfig, AtmError> {
    let raw = fs::read_to_string(path).map_err(|source| {
        AtmError::config(format!("failed to read team config at {}", path.display()))
            .with_recovery("Restore config.json or repair its permissions before retrying.")
            .with_source(source)
    })?;
    serde_json::from_str(&raw).map_err(|source| {
        AtmError::config(format!("failed to parse team config at {}", path.display()))
            .with_recovery("Repair the team config JSON and retry the daemon-owned ingress.")
            .with_source(source)
    })
}

fn read_mailbox_messages(path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
    let raw = fs::read_to_string(path).map_err(|source| {
        AtmError::mailbox_read(format!("failed to read mailbox at {}", path.display()))
            .with_recovery("Restore the mailbox file or repair its permissions before retrying.")
            .with_source(source)
    })?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    if let Ok(messages) = serde_json::from_str::<Vec<MessageEnvelope>>(&raw) {
        return Ok(messages);
    }
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<MessageEnvelope>(line).map_err(|source| {
                AtmError::mailbox_read("failed to parse mailbox record")
                    .with_recovery("Repair the malformed mailbox JSON entry and retry.")
                    .with_source(source)
            })
        })
        .collect()
}

fn write_mailbox_messages(path: &Path, messages: &[MessageEnvelope]) -> Result<(), AtmError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            AtmError::mailbox_write(format!(
                "failed to create mailbox parent directory {}",
                parent.display()
            ))
            .with_recovery("Repair mailbox directory permissions and retry.")
            .with_source(source)
        })?;
    }
    let encoded = serde_json::to_vec_pretty(messages).map_err(|source| {
        AtmError::mailbox_write(format!(
            "failed to encode mailbox payload for {}",
            path.display()
        ))
        .with_recovery(
            "Inspect the mailbox payload and repair invalid message data before retrying.",
        )
        .with_source(source)
    })?;
    let parent = path.parent().ok_or_else(|| {
        AtmError::mailbox_write(format!(
            "failed to resolve mailbox parent directory for {}",
            path.display()
        ))
        .with_recovery("Repair the mailbox path and retry.")
    })?;
    let temp_name = format!(
        "{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("mailbox.json"),
        Uuid::new_v4()
    );
    let temp_path = parent.join(temp_name);
    let mut temp_file = fs::File::create(&temp_path).map_err(|source| {
        AtmError::mailbox_write(format!(
            "failed to create mailbox temp file {}",
            temp_path.display()
        ))
        .with_recovery("Repair mailbox directory permissions and retry.")
        .with_source(source)
    })?;
    if let Err(source) = temp_file.write_all(&encoded) {
        let _ = fs::remove_file(&temp_path);
        return Err(AtmError::mailbox_write(format!(
            "failed to write mailbox temp file {}",
            temp_path.display()
        ))
        .with_recovery("Repair mailbox directory permissions and retry.")
        .with_source(source));
    }
    if let Err(source) = temp_file.sync_all() {
        let _ = fs::remove_file(&temp_path);
        return Err(AtmError::mailbox_write(format!(
            "failed to sync mailbox temp file {}",
            temp_path.display()
        ))
        .with_recovery("Repair mailbox directory permissions and retry.")
        .with_source(source));
    }
    drop(temp_file);
    fs::rename(&temp_path, path).map_err(|source| {
        let _ = fs::remove_file(&temp_path);
        AtmError::mailbox_write(format!(
            "failed to atomically replace mailbox at {}",
            path.display()
        ))
        .with_recovery("Repair mailbox directory permissions and retry.")
        .with_source(source)
    })
}

fn discover_source_paths(request: &WatchSubscriptionRequest) -> Result<Vec<PathBuf>, AtmError> {
    let primary = inbox_path_from_home(
        &request.home_dir,
        request.team.as_str(),
        request.agent.as_str(),
    )?;
    let inbox_dir = primary
        .parent()
        .ok_or_else(|| AtmError::validation("mailbox path is missing an inbox directory parent"))?;
    let mut paths = Vec::new();
    if primary.exists() {
        paths.push(primary.clone());
    }
    if inbox_dir.is_dir() {
        let prefix = format!("{}.", request.agent.as_str());
        for entry in fs::read_dir(inbox_dir).map_err(|source| {
            AtmError::mailbox_read(format!(
                "failed to enumerate inbox directory {}",
                inbox_dir.display()
            ))
            .with_recovery("Repair inbox directory permissions and retry.")
            .with_source(source)
        })? {
            let entry = entry.map_err(|source| {
                AtmError::mailbox_read(format!(
                    "failed to read inbox entry under {}",
                    inbox_dir.display()
                ))
                .with_recovery("Repair inbox directory permissions and retry.")
                .with_source(source)
            })?;
            let path = entry.path();
            if path == primary {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name.starts_with(&prefix) && name.ends_with(".json") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn load_workspace_config_direct(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    fs::metadata(&request.current_dir).map_err(|source| {
        AtmError::config(format!(
            "failed to inspect workspace directory {}",
            request.current_dir.display()
        ))
        .with_recovery("Repair the workspace path before retrying daemon-owned config ingress.")
        .with_source(source)
    })?;
    Err(AtmError::config(format!(
        "daemon workspace config ingress is not implemented for {}",
        request.current_dir.display()
    ))
    .with_recovery(
        "Use the in-process ATM config loader or complete daemon workspace config ingress before routing this path through atm-daemon.",
    ))
}

fn load_team_config_direct(
    request: ConfigTeamLoadRequest,
) -> Result<ConfigTeamLoadResponse, AtmError> {
    let team_dir = team_dir_from_home(&request.home_dir, request.team.as_str())?;
    let team_config = read_team_config(&team_dir.join("config.json"))?;
    Ok(ConfigTeamLoadResponse {
        team_dir,
        team_config,
    })
}

fn import_inbox_source_direct(
    request: InboxIngressImportRequest,
) -> Result<InboxIngressImportResponse, AtmError> {
    let paths = discover_source_paths(&WatchSubscriptionRequest {
        home_dir: request.home_dir,
        team: request.team,
        agent: request.agent,
    })?;
    let source_files = paths
        .into_iter()
        .map(|path| {
            read_mailbox_messages(&path)
                .map(|messages| atm_core::InboxSourceFileRecord { path, messages })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(InboxIngressImportResponse { source_files })
}

fn compute_identity_fingerprint_direct(
    request: InboxIngressIdentityFingerprintRequest,
) -> Result<InboxIngressIdentityFingerprintResponse, AtmError> {
    let fingerprint = request
        .message
        .message_id
        .map(|message_id| message_id.to_string())
        .or_else(|| {
            Some(format!(
                "{}:{}",
                request.message.from,
                request.message.timestamp.into_inner().to_rfc3339()
            ))
        });
    Ok(InboxIngressIdentityFingerprintResponse { fingerprint })
}

fn report_inbox_diagnostics_direct(
    request: InboxIngressDiagnosticsRequest,
) -> Result<InboxIngressDiagnosticsResponse, AtmError> {
    let mut seen = HashSet::new();
    let mut duplicate_legacy_message_ids = 0usize;
    let mut messages_without_ids = 0usize;

    for source in request.source_files {
        for message in source.messages {
            if let Some(message_id) = message.message_id {
                if !seen.insert(message_id) {
                    duplicate_legacy_message_ids += 1;
                }
            } else {
                messages_without_ids += 1;
            }
        }
    }

    Ok(InboxIngressDiagnosticsResponse {
        duplicate_legacy_message_ids,
        messages_without_ids,
    })
}

fn export_source_files_direct(
    request: InboxExportRecordRequest,
) -> Result<InboxExportRecordResponse, AtmError> {
    let committed_paths = request.source_files.len();
    for source in request.source_files {
        write_mailbox_messages(&source.path, &source.messages)?;
    }
    Ok(InboxExportRecordResponse { committed_paths })
}

fn reexport_messages_direct(
    request: InboxExportReexportMessageRequest,
) -> Result<InboxExportReexportMessageResponse, AtmError> {
    let wrote_messages = request.messages.len();
    write_mailbox_messages(&request.path, &request.messages)?;
    Ok(InboxExportReexportMessageResponse { wrote_messages })
}

#[derive(Debug, Clone, Copy, Default)]
struct DaemonObservability;

impl boundary::sealed::Sealed for DaemonObservability {}

impl ObservabilityPort for DaemonObservability {
    fn emit(&self, _event: CommandEvent) -> Result<(), AtmError> {
        // The daemon currently uses a null-object observability sink until the retained log adapter lands.
        Ok(())
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        // Query returns an empty snapshot because the daemon does not expose a retained log backend yet.
        Ok(AtmLogSnapshot::default())
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        // Follow returns an empty session because live daemon log streaming is not implemented yet.
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

        let idle_timeout = Duration::from_secs(1);
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
                    let response = match _dispatcher.dispatch(request) {
                        Ok(response) => response,
                        Err(error) => ResponseEnvelope::Error(
                            atm_core::protocol::ProtocolErrorEnvelope::from_error(&error),
                        ),
                    };
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
        let mut stderr = std::io::stderr().lock();
        serde_json::to_writer(&mut stderr, &event).map_err(|source| {
            AtmError::observability_emit("failed to encode daemon notification event")
                .with_source(source)
        })?;
        writeln!(&mut stderr).map_err(|source| {
            AtmError::observability_emit("failed to write daemon notification event")
                .with_source(source)
        })?;
        Ok(())
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
        let socket_path = atm_core::protocol::daemon_socket_path()?;
        Ok(RuntimeStatusSnapshot {
            status: "ready".to_string(),
            detail: Some(format!(
                "daemon runtime adapters are active on {}",
                socket_path.display()
            )),
        })
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
        Ok(WatchEventBatch {
            paths: discover_source_paths(&request)?,
        })
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
        let batch = WatchEventBatch {
            paths: discover_source_paths(&WatchSubscriptionRequest {
                home_dir: request.home_dir.clone(),
                team: request.team.clone(),
                agent: request.agent.clone(),
            })?,
        };
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
        load_workspace_config_direct(request)
    }

    fn load_team_config(
        &self,
        request: ConfigTeamLoadRequest,
    ) -> Result<ConfigTeamLoadResponse, AtmError> {
        load_team_config_direct(request)
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
        import_inbox_source_direct(request)
    }

    fn compute_identity_fingerprint(
        &self,
        request: InboxIngressIdentityFingerprintRequest,
    ) -> Result<InboxIngressIdentityFingerprintResponse, AtmError> {
        compute_identity_fingerprint_direct(request)
    }

    fn report_diagnostics(
        &self,
        request: InboxIngressDiagnosticsRequest,
    ) -> Result<InboxIngressDiagnosticsResponse, AtmError> {
        report_inbox_diagnostics_direct(request)
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
        export_source_files_direct(request)
    }

    fn reexport_message(
        &self,
        request: InboxExportReexportMessageRequest,
    ) -> Result<InboxExportReexportMessageResponse, AtmError> {
        reexport_messages_direct(request)
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
