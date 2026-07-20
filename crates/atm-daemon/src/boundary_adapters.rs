#[cfg(test)]
use atm_core::{boundary, error::AtmError};
#[cfg(test)]
use atm_storage::MessageEnvelope;

#[cfg(test)]
use crate::claude_compat::SourceFileRecord;
#[cfg(test)]
use crate::direct_boundaries;

#[cfg(test)]
#[derive(Clone, Debug, Default)]
#[deprecated(
    note = "Phase AD obsolete: retained only as daemon-local historical Claude compatibility scaffolding after reconcile/watch runtime removal."
)]
pub(crate) struct DaemonInboxIngress;

#[cfg(test)]
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

#[cfg(test)]
impl boundary::sealed::Sealed for DaemonInboxIngress {}

#[cfg(test)]
mod tests {
    use super::DaemonInboxIngress;
    use crate::claude_compat;
    use atm_core::schema::{AtmMessageId, InboxMessage};
    use atm_core::test_support::{TEST_LEAD, TEST_SENDER, TEST_TEAM};
    use atm_core::types::{AgentName, IsoTimestamp};
    use tempfile::TempDir;

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
            requires_ack: false,
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
