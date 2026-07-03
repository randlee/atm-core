use atm_core::{
    boundary::{
        self, ConfigIngress, ConfigLoadRequest, ConfigLoadResponse, NotificationEvent,
        ReconcileRequest, ReconcileResult, WatchEventBatch, WatchSubscriptionRequest,
    },
    error::AtmError,
};
use atm_storage::{MessageEnvelope, RosterStore};
use std::sync::Arc;

use crate::SubsystemObservability;
use crate::claude_compat::SourceFileRecord;
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
        roster_store: Arc<dyn RosterStore + Send + Sync>,
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

    pub(crate) fn import_inbox_source(
        &self,
        home_dir: &std::path::Path,
        team: &atm_storage::TeamName,
        agent: &atm_storage::AgentName,
    ) -> Result<Vec<SourceFileRecord>, AtmError> {
        direct_boundaries::import_inbox_source(home_dir, team, agent)
    }

    pub(crate) fn compute_identity_fingerprint(
        &self,
        message: &MessageEnvelope,
    ) -> Option<atm_core::boundary::MessageFingerprint> {
        direct_boundaries::compute_identity_fingerprint(message)
    }
}

impl boundary::sealed::Sealed for DaemonInboxIngress {}

impl crate::reconcile_runtime::InboxIngressPort for DaemonInboxIngress {
    fn import_inbox_source(
        &self,
        home_dir: &std::path::Path,
        team: &atm_storage::TeamName,
        agent: &atm_storage::AgentName,
    ) -> Result<Vec<SourceFileRecord>, AtmError> {
        self.import_inbox_source(home_dir, team, agent)
    }

    fn compute_identity_fingerprint(
        &self,
        message: &MessageEnvelope,
    ) -> Option<atm_core::boundary::MessageFingerprint> {
        self.compute_identity_fingerprint(message)
    }
}

#[cfg(test)]
mod tests {
    use super::{DaemonInboxIngress, DaemonNotificationSink};
    use crate::claude_compat;
    use atm_core::boundary::NotificationSink;
    use atm_core::protocol::{NotificationEvent, NotificationKind};
    use atm_core::schema::{AtmMessageId, InboxMessage};
    use atm_core::test_support::{TEST_LEAD, TEST_SENDER, TEST_TEAM};
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
                "members": [{"name": TEST_SENDER}, {"name": TEST_LEAD}]
            })
            .to_string(),
        )
        .expect("team config");
        let inbox_path = inbox_dir.join(format!("{TEST_SENDER}.json"));

        let ingress = DaemonInboxIngress::new();
        let message = sample_message(TEST_LEAD, "full body that should project to a stub");
        let original_fingerprint = ingress.compute_identity_fingerprint(&message);

        claude_compat::reexport_messages(&inbox_path, std::slice::from_ref(&message))
            .expect("first reexport");
        claude_compat::reexport_messages(&inbox_path, std::slice::from_ref(&message))
            .expect("second reexport");

        let import = ingress
            .import_inbox_source(
                tempdir.path(),
                &TEST_TEAM.parse().expect("team"),
                &TEST_SENDER.parse().expect("agent"),
            )
            .expect("import source");
        assert_eq!(import.len(), 1);
        assert_eq!(import[0].messages.len(), 1);

        let imported = import[0].messages[0].clone();
        assert_eq!(
            imported.text,
            format!(
                "atm read --message-id {}",
                message.message_id.expect("message id")
            )
        );

        let imported_fingerprint = ingress.compute_identity_fingerprint(&imported);
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
                "members": [{"name": TEST_SENDER}, {"name": TEST_LEAD}]
            })
            .to_string(),
        )
        .expect("team config");
        let inbox_path = inbox_dir.join(format!("{TEST_SENDER}.json"));

        let ingress = DaemonInboxIngress::new();
        let message = sample_message(TEST_LEAD, "small body stays fully exported");
        let original_fingerprint = ingress.compute_identity_fingerprint(&message);

        claude_compat::reexport_messages(&inbox_path, std::slice::from_ref(&message))
            .expect("first reexport");
        claude_compat::reexport_messages(&inbox_path, std::slice::from_ref(&message))
            .expect("second reexport");

        let import = ingress
            .import_inbox_source(
                tempdir.path(),
                &TEST_TEAM.parse().expect("team"),
                &TEST_SENDER.parse().expect("agent"),
            )
            .expect("import source");
        assert_eq!(import.len(), 1);
        assert_eq!(import[0].messages.len(), 1);

        let imported = import[0].messages[0].clone();
        assert_eq!(imported.text, message.text);

        let imported_fingerprint = ingress.compute_identity_fingerprint(&imported);
        assert_eq!(imported_fingerprint, original_fingerprint);
    }

    fn sample_message(from: &str, text: &str) -> InboxMessage {
        let message_id = AtmMessageId::new();

        InboxMessage {
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
