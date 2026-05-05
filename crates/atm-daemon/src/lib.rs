#![forbid(unsafe_code)]
#![allow(dead_code)]

//! Skeleton crate for Phase R daemon runtime work.

pub(crate) mod composition;

use std::error::Error as StdError;
use std::fmt;

use atm_core::{
    RequestEnvelope, ResponseEnvelope,
    boundary::{
        self, ConfigIngress, ConfigLoadRequest, ConfigLoadResponse, ConfigTeamLoadRequest,
        ConfigTeamLoadResponse, InboxExport, InboxExportRecordRequest,
        InboxExportRecordResponse, InboxExportReexportMessageRequest,
        InboxExportReexportMessageResponse, InboxIngress, InboxIngressDiagnosticsRequest,
        InboxIngressDiagnosticsResponse, InboxIngressIdentityFingerprintRequest,
        InboxIngressIdentityFingerprintResponse, InboxIngressImportRequest,
        InboxIngressImportResponse, NotificationEvent, ReconcileRequest, ReconcileResult,
        RequestDispatcher, RuntimeStatusSnapshot, WatchEventBatch, WatchSubscriptionRequest,
    },
    error::AtmError,
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
    fn serve(&self, _dispatcher: &dyn RequestDispatcher) -> Result<(), AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon server transport stub is not implemented yet",
            DaemonBoundaryStubError::ServerTransport,
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
    fn dispatch(&self, _request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon request dispatcher stub is not implemented yet",
            DaemonBoundaryStubError::RequestDispatcher,
        ))
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
    fn deliver(&self, _event: NotificationEvent) -> Result<(), AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon notification sink stub is not implemented yet",
            DaemonBoundaryStubError::NotificationSink,
        ))
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
        Err(daemon_boundary_stub_error(
            "daemon status source stub is not implemented yet",
            DaemonBoundaryStubError::StatusSource,
        ))
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
    fn poll(&self, _request: WatchSubscriptionRequest) -> Result<WatchEventBatch, AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon watch event source stub is not implemented yet",
            DaemonBoundaryStubError::WatchEventSource,
        ))
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
    fn reconcile(&self, _request: ReconcileRequest) -> Result<ReconcileResult, AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon reconcile coordinator stub is not implemented yet",
            DaemonBoundaryStubError::ReconcileCoordinator,
        ))
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
    fn load_config(&self, _request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon config ingress stub is not implemented yet",
            DaemonBoundaryStubError::ConfigIngress,
        ))
    }

    fn load_team_config(
        &self,
        _request: ConfigTeamLoadRequest,
    ) -> Result<ConfigTeamLoadResponse, AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon config ingress team-config stub is not implemented yet",
            DaemonBoundaryStubError::ConfigIngress,
        ))
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
        _request: InboxIngressImportRequest,
    ) -> Result<InboxIngressImportResponse, AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon inbox ingress import stub is not implemented yet",
            DaemonBoundaryStubError::InboxIngress,
        ))
    }

    fn compute_identity_fingerprint(
        &self,
        _request: InboxIngressIdentityFingerprintRequest,
    ) -> Result<InboxIngressIdentityFingerprintResponse, AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon inbox ingress fingerprint stub is not implemented yet",
            DaemonBoundaryStubError::InboxIngress,
        ))
    }

    fn report_diagnostics(
        &self,
        _request: InboxIngressDiagnosticsRequest,
    ) -> Result<InboxIngressDiagnosticsResponse, AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon inbox ingress diagnostics stub is not implemented yet",
            DaemonBoundaryStubError::InboxIngress,
        ))
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
        _request: InboxExportRecordRequest,
    ) -> Result<InboxExportRecordResponse, AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon inbox export record stub is not implemented yet",
            DaemonBoundaryStubError::InboxExport,
        ))
    }

    fn reexport_message(
        &self,
        _request: InboxExportReexportMessageRequest,
    ) -> Result<InboxExportReexportMessageResponse, AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon inbox re-export stub is not implemented yet",
            DaemonBoundaryStubError::InboxExport,
        ))
    }
}
