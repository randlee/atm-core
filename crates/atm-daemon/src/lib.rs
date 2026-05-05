#![forbid(unsafe_code)]
#![allow(dead_code)]

//! Skeleton crate for Phase R daemon runtime work.

pub(crate) mod composition;

use std::error::Error as StdError;
use std::fmt;

use atm_core::{
    boundary::{
        self, ConfigIngress, ConfigLoadRequest, ConfigLoadResponse, InboxExport,
        InboxExportRecordRequest, InboxExportRecordResponse, InboxExportReexportMessageRequest,
        InboxExportReexportMessageResponse, InboxIngress, InboxIngressDiagnosticsRequest,
        InboxIngressDiagnosticsResponse, InboxIngressIdentityFingerprintRequest,
        InboxIngressIdentityFingerprintResponse, InboxIngressImportRequest,
        InboxIngressImportResponse,
    },
    error::AtmError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DaemonBoundaryStubError {
    ConfigIngress,
    InboxIngress,
    InboxExport,
}

impl fmt::Display for DaemonBoundaryStubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ConfigIngress => "daemon config ingress scaffold is not wired",
            Self::InboxIngress => "daemon inbox ingress scaffold is not wired",
            Self::InboxExport => "daemon inbox export scaffold is not wired",
        };

        f.write_str(message)
    }
}

impl StdError for DaemonBoundaryStubError {}

fn daemon_boundary_stub_error(message: &'static str, source: DaemonBoundaryStubError) -> AtmError {
    AtmError::validation(message)
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

impl boundary::ServerTransport for LocalSocketServerTransport {}

/// Placeholder runtime sink for daemon-emitted notifications.
#[derive(Debug, Default)]
pub(crate) struct DaemonNotificationSink;

impl DaemonNotificationSink {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonNotificationSink {}

impl boundary::NotificationSink for DaemonNotificationSink {}

/// Placeholder runtime source for daemon status snapshots.
#[derive(Debug, Default)]
pub(crate) struct DaemonStatusSource;

impl DaemonStatusSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonStatusSource {}

impl boundary::StatusSource for DaemonStatusSource {}

/// Placeholder runtime source for daemon watch events.
#[derive(Debug, Default)]
pub(crate) struct FileWatchEventSource;

impl FileWatchEventSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for FileWatchEventSource {}

impl boundary::WatchEventSource for FileWatchEventSource {}

/// Placeholder runtime coordinator for daemon reconcile work.
#[derive(Debug, Default)]
pub(crate) struct DaemonReconcileCoordinator;

impl DaemonReconcileCoordinator {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonReconcileCoordinator {}

impl boundary::ReconcileCoordinator for DaemonReconcileCoordinator {}

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
