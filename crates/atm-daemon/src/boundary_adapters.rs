use atm_core::{
    boundary::{
        self, ConfigIngress, ConfigLoadRequest, ConfigLoadResponse, ConfigTeamLoadRequest,
        ConfigTeamLoadResponse, InboxExport, InboxExportRecordRequest, InboxExportRecordResponse,
        InboxExportReexportMessageRequest, InboxExportReexportMessageResponse, InboxIngress,
        InboxIngressDiagnosticsRequest, InboxIngressDiagnosticsResponse,
        InboxIngressIdentityFingerprintRequest, InboxIngressIdentityFingerprintResponse,
        InboxIngressImportRequest, InboxIngressImportResponse, NotificationEvent, ReconcileRequest,
        ReconcileResult, WatchEventBatch, WatchSubscriptionRequest,
    },
    error::AtmError,
};

use crate::direct_boundaries;
use crate::notification_runtime::NotificationRuntime;
use crate::reconcile_runtime::ReconcileRuntime;
use crate::watch_runtime::WatchRuntime;

#[derive(Clone)]
pub(crate) struct DaemonNotificationSink {
    runtime: NotificationRuntime,
}

impl std::fmt::Debug for DaemonNotificationSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DaemonNotificationSink(..)")
    }
}

impl DaemonNotificationSink {
    pub(crate) fn new() -> Self {
        Self {
            runtime: NotificationRuntime::new(),
        }
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        self.runtime.start()
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        self.runtime.shutdown()
    }
}

impl boundary::sealed::Sealed for DaemonNotificationSink {}

impl boundary::NotificationSink for DaemonNotificationSink {
    fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError> {
        self.runtime.deliver(event)
    }
}

#[derive(Clone)]
pub(crate) struct FileWatchEventSource {
    runtime: WatchRuntime,
}

impl std::fmt::Debug for FileWatchEventSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FileWatchEventSource(..)")
    }
}

impl FileWatchEventSource {
    pub(crate) fn new() -> Self {
        Self {
            runtime: WatchRuntime::new(),
        }
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        self.runtime.start()
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        self.runtime.shutdown()
    }
}

impl boundary::sealed::Sealed for FileWatchEventSource {}

impl boundary::WatchEventSource for FileWatchEventSource {
    fn poll(&self, request: WatchSubscriptionRequest) -> Result<WatchEventBatch, AtmError> {
        self.runtime.poll(request)
    }
}

#[derive(Clone)]
pub(crate) struct DaemonReconcileCoordinator {
    runtime: ReconcileRuntime,
}

impl std::fmt::Debug for DaemonReconcileCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DaemonReconcileCoordinator(..)")
    }
}

impl DaemonReconcileCoordinator {
    pub(crate) fn new(
        watch_event_source: FileWatchEventSource,
        inbox_ingress: DaemonInboxIngress,
        notification_sink: DaemonNotificationSink,
    ) -> Self {
        Self {
            runtime: ReconcileRuntime::new(watch_event_source, inbox_ingress, notification_sink),
        }
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        self.runtime.start()
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        self.runtime.shutdown()
    }
}

impl boundary::sealed::Sealed for DaemonReconcileCoordinator {}

impl boundary::ReconcileCoordinator for DaemonReconcileCoordinator {
    fn reconcile(&self, request: ReconcileRequest) -> Result<ReconcileResult, AtmError> {
        self.runtime.reconcile(request)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DaemonConfigIngress;

impl DaemonConfigIngress {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonConfigIngress {}

impl ConfigIngress for DaemonConfigIngress {
    fn load_config(&self, request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError> {
        direct_boundaries::load_workspace_config(request)
    }

    fn load_team_config(
        &self,
        request: ConfigTeamLoadRequest,
    ) -> Result<ConfigTeamLoadResponse, AtmError> {
        direct_boundaries::load_team_config(request)
    }
}

#[derive(Clone, Debug, Default)]
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
        direct_boundaries::import_inbox_source(request)
    }

    fn compute_identity_fingerprint(
        &self,
        request: InboxIngressIdentityFingerprintRequest,
    ) -> Result<InboxIngressIdentityFingerprintResponse, AtmError> {
        direct_boundaries::compute_identity_fingerprint(request)
    }

    fn report_diagnostics(
        &self,
        request: InboxIngressDiagnosticsRequest,
    ) -> Result<InboxIngressDiagnosticsResponse, AtmError> {
        direct_boundaries::report_inbox_diagnostics(request)
    }
}

#[derive(Clone, Debug, Default)]
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
        direct_boundaries::export_source_files(request)
    }

    fn reexport_message(
        &self,
        request: InboxExportReexportMessageRequest,
    ) -> Result<InboxExportReexportMessageResponse, AtmError> {
        direct_boundaries::reexport_messages(request)
    }
}
