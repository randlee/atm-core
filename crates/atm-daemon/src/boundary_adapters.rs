use atm_core::{
    boundary::{
        self, ConfigIngress, ConfigLoadRequest, ConfigLoadResponse, InboxExport,
        InboxExportAppendMessageSetRequest, InboxExportAppendMessageSetResponse,
        InboxExportRecordRequest, InboxExportRecordResponse, InboxExportReexportMessageRequest,
        InboxExportReexportMessageResponse, InboxIngress, InboxIngressDiagnosticsRequest,
        InboxIngressDiagnosticsResponse, InboxIngressIdentityFingerprintRequest,
        InboxIngressIdentityFingerprintResponse, InboxIngressImportRequest,
        InboxIngressImportResponse, NotificationEvent, ReconcileRequest, ReconcileResult,
        WatchEventBatch, WatchSubscriptionRequest,
    },
    error::AtmError,
};
use std::sync::Arc;

use crate::SubsystemObservability;
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
    pub(crate) fn new(runtime: NotificationRuntime) -> Self {
        Self { runtime }
    }

    pub(crate) fn runtime(&self) -> NotificationRuntime {
        self.runtime.clone()
    }

    #[cfg(test)]
    pub(crate) fn new_for_test_with_path(path: std::path::PathBuf, queue_capacity: usize) -> Self {
        Self {
            runtime: NotificationRuntime::new_for_test_with_path(path, queue_capacity),
        }
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        self.runtime.start()
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        self.runtime.shutdown()
    }
}

impl Drop for DaemonNotificationSink {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            tracing::warn!(
                subsystem = "notification",
                action = "drop_shutdown",
                outcome = "failed",
                %error,
                "DaemonNotificationSink drop could not shut down runtime cleanly"
            );
        }
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
    pub(crate) fn new_with_observability(observability: SubsystemObservability) -> Self {
        Self {
            runtime: WatchRuntime::new_with_observability(observability),
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
    pub(crate) fn new_with_observability(
        watch_event_source: FileWatchEventSource,
        inbox_ingress: DaemonInboxIngress,
        roster_store: Arc<dyn boundary::RosterStore + Send + Sync>,
        notification_sink: DaemonNotificationSink,
        observability: SubsystemObservability,
    ) -> Self {
        Self {
            runtime: ReconcileRuntime::new_with_observability(
                Arc::new(watch_event_source),
                Arc::new(inbox_ingress),
                roster_store,
                Arc::new(notification_sink),
                observability,
            ),
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
    ) -> InboxIngressIdentityFingerprintResponse {
        direct_boundaries::compute_identity_fingerprint(request)
    }

    fn report_diagnostics(
        &self,
        request: InboxIngressDiagnosticsRequest,
    ) -> InboxIngressDiagnosticsResponse {
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

    fn append_message_set(
        &self,
        request: InboxExportAppendMessageSetRequest,
    ) -> Result<InboxExportAppendMessageSetResponse, AtmError> {
        direct_boundaries::append_message_set(request)
    }
}

#[cfg(test)]
mod tests {
    use super::{DaemonInboxExport, DaemonInboxIngress, DaemonNotificationSink};
    use atm_core::boundary::{
        InboxExport, InboxExportReexportMessageRequest, InboxIngress,
        InboxIngressIdentityFingerprintRequest, InboxIngressImportRequest, NotificationSink,
    };
    use atm_core::protocol::{NotificationEvent, NotificationKind};
    use atm_core::roles::ROLE_TEAM_LEAD;
    use atm_core::schema::{AtmMessageId, MessageEnvelope};
    use atm_core::test_support::{TEST_SENDER, TEST_TEAM};
    use atm_core::types::{AgentName, IsoTimestamp};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn notifier_delivery_stays_behind_boundary_trait() {
        let tempdir = TempDir::new().expect("tempdir");
        let output_path = tempdir.path().join("notifications.jsonl");
        let sink = DaemonNotificationSink::new_for_test_with_path(output_path.clone(), 8);
        let boundary: Arc<dyn NotificationSink + Send + Sync> = Arc::new(sink.clone());

        sink.start().expect("start");
        boundary
            .deliver(NotificationEvent {
                kind: NotificationKind::Delivery,
                detail: "boundary-only".to_string(),
                team: None,
                agent: None,
            })
            .expect("deliver");
        sink.shutdown().expect("shutdown");

        let output = std::fs::read_to_string(output_path).expect("output");
        assert!(output.contains("\"kind\":\"delivery\""));
        assert!(output.contains("\"detail\":\"boundary-only\""));
    }

    #[test]
    fn inbox_projection_stub_reexport_preserves_logical_identity() {
        let tempdir = TempDir::new().expect("tempdir");
        std::fs::write(
            tempdir.path().join(".atm.toml"),
            "[atm]\nclaude_jsonl_body_export_max_bytes = 0\n",
        )
        .expect("config");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        let inbox_dir = team_dir.join("inboxes");
        std::fs::create_dir_all(&inbox_dir).expect("inboxes");
        std::fs::write(
            team_dir.join("config.json"),
            serde_json::json!({
                "members": [{"name": TEST_SENDER}, {"name": ROLE_TEAM_LEAD}]
            })
            .to_string(),
        )
        .expect("team config");
        let inbox_path = inbox_dir.join(format!("{TEST_SENDER}.json"));

        let export = DaemonInboxExport::new();
        let ingress = DaemonInboxIngress::new();
        let message = sample_message(ROLE_TEAM_LEAD, "full body that should project to a stub");
        let original_fingerprint = ingress
            .compute_identity_fingerprint(InboxIngressIdentityFingerprintRequest {
                message: message.clone(),
            })
            .fingerprint;

        export
            .reexport_message(InboxExportReexportMessageRequest {
                path: inbox_path.clone(),
                messages: vec![message.clone()],
            })
            .expect("first reexport");
        export
            .reexport_message(InboxExportReexportMessageRequest {
                path: inbox_path.clone(),
                messages: vec![message.clone()],
            })
            .expect("second reexport");

        let import = ingress
            .import_inbox_source(InboxIngressImportRequest {
                home_dir: tempdir.path().to_path_buf(),
                team: TEST_TEAM.parse().expect("team"),
                agent: TEST_SENDER.parse().expect("agent"),
            })
            .expect("import source");
        assert_eq!(import.source_files.len(), 1);
        assert_eq!(import.source_files[0].messages.len(), 1);

        let imported = import.source_files[0].messages[0].clone();
        assert_eq!(
            imported.text,
            format!(
                "atm read --message-id {}",
                message.message_id.expect("message id")
            )
        );

        let imported_fingerprint = ingress
            .compute_identity_fingerprint(InboxIngressIdentityFingerprintRequest {
                message: imported,
            })
            .fingerprint;
        assert_eq!(imported_fingerprint, original_fingerprint);
    }

    #[test]
    fn inbox_projection_full_body_reexport_preserves_logical_identity() {
        let tempdir = TempDir::new().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        let inbox_dir = team_dir.join("inboxes");
        std::fs::create_dir_all(&inbox_dir).expect("inboxes");
        std::fs::write(
            team_dir.join("config.json"),
            serde_json::json!({
                "members": [{"name": TEST_SENDER}, {"name": ROLE_TEAM_LEAD}]
            })
            .to_string(),
        )
        .expect("team config");
        let inbox_path = inbox_dir.join(format!("{TEST_SENDER}.json"));

        let export = DaemonInboxExport::new();
        let ingress = DaemonInboxIngress::new();
        let message = sample_message(ROLE_TEAM_LEAD, "small body stays fully exported");
        let original_fingerprint = ingress
            .compute_identity_fingerprint(InboxIngressIdentityFingerprintRequest {
                message: message.clone(),
            })
            .fingerprint;

        export
            .reexport_message(InboxExportReexportMessageRequest {
                path: inbox_path.clone(),
                messages: vec![message.clone()],
            })
            .expect("first reexport");
        export
            .reexport_message(InboxExportReexportMessageRequest {
                path: inbox_path.clone(),
                messages: vec![message.clone()],
            })
            .expect("second reexport");

        let import = ingress
            .import_inbox_source(InboxIngressImportRequest {
                home_dir: tempdir.path().to_path_buf(),
                team: TEST_TEAM.parse().expect("team"),
                agent: TEST_SENDER.parse().expect("agent"),
            })
            .expect("import source");
        assert_eq!(import.source_files.len(), 1);
        assert_eq!(import.source_files[0].messages.len(), 1);

        let imported = import.source_files[0].messages[0].clone();
        assert_eq!(imported.text, message.text);

        let imported_fingerprint = ingress
            .compute_identity_fingerprint(InboxIngressIdentityFingerprintRequest {
                message: imported,
            })
            .fingerprint;
        assert_eq!(imported_fingerprint, original_fingerprint);
    }

    fn sample_message(from: &str, text: &str) -> MessageEnvelope {
        let message_id = AtmMessageId::new();

        MessageEnvelope {
            from: from.parse::<AgentName>().expect("agent"),
            text: text.to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            summary: Some("summary".to_string()),
            message_id: Some(message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }
}
