use std::path::PathBuf;

use atm_storage::{
    AtmError, Message, MessageKey, MessageQuery, MessageStore, RosterSnapshot, RosterStore,
    TeamName,
};

#[derive(Debug, Clone)]
struct ClaudeStorageBackend {
    home_dir: PathBuf,
}

impl ClaudeStorageBackend {
    #[allow(
        dead_code,
        reason = "Called from tests while still appearing dead in the production compile."
    )]
    fn new(home_dir: PathBuf) -> Self {
        Self { home_dir }
    }
}

impl MessageStore for ClaudeStorageBackend {
    fn save_message(&self, message: &Message) -> Result<(), AtmError> {
        crate::mailbox::save_message(&self.home_dir, message)
    }

    fn load_message(&self, key: &MessageKey) -> Result<Option<Message>, AtmError> {
        crate::mailbox::load_message(&self.home_dir, key)
    }

    fn list_messages(&self, query: &MessageQuery) -> Result<Vec<Message>, AtmError> {
        crate::mailbox::list_messages(&self.home_dir, query)
    }

    fn delete_message(&self, key: &MessageKey) -> Result<(), AtmError> {
        crate::mailbox::delete_message(&self.home_dir, key)
    }
}

impl RosterStore for ClaudeStorageBackend {
    fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError> {
        crate::roster::load_roster(&self.home_dir, team)
    }

    fn save_roster(&self, roster: &RosterSnapshot) -> Result<(), AtmError> {
        crate::roster::save_roster(&self.home_dir, roster)
    }

    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
        crate::roster::list_teams(&self.home_dir)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::str::FromStr;

    use atm_storage::{
        AgentName, IsoTimestamp, Message, MessageEnvelope, MessageKey, MessageQuery, MessageStore,
        ROLE_TEAM_LEAD, RosterHarness, RosterMember, RosterMemberKind, RosterSnapshot, RosterStore,
        TeamName,
    };
    use tempfile::tempdir;

    use super::ClaudeStorageBackend;

    const TEST_TEAM: &str = "test-team";
    const TEST_SENDER: &str = "test-sender";
    const TEST_SENDER_2: &str = "test-sender-2";

    fn sample_message(team: &str, agent: &str, sender: &str, text: &str) -> Message {
        let team = TeamName::from_str(team).expect("team");
        let agent = AgentName::from_str(agent).expect("agent");
        let message_id = atm_storage::AtmMessageId::new();
        Message {
            team,
            agent,
            message_key: MessageKey::from(message_id),
            envelope: MessageEnvelope {
                from: AgentName::from_str(sender).expect("sender"),
                text: text.to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                summary: None,
                message_id: Some(message_id),
                task_id: None,
                parent_message_id: None,
                thread_mode: None,
                extra: serde_json::Map::new(),
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                source_team: None,
                expires_at: None,
            },
        }
    }

    #[test]
    fn message_store_round_trips_primary_array_inbox() {
        let tempdir = tempdir().expect("tempdir");
        let backend = ClaudeStorageBackend::new(tempdir.path().to_path_buf());
        let first = sample_message(TEST_TEAM, ROLE_TEAM_LEAD, TEST_SENDER, "one");
        let second = sample_message(TEST_TEAM, ROLE_TEAM_LEAD, TEST_SENDER_2, "two");

        backend.save_message(&first).expect("save first");
        backend.save_message(&second).expect("save second");

        let listed = backend
            .list_messages(&MessageQuery {
                team: TeamName::from_str(TEST_TEAM).expect("team"),
                agent: AgentName::from_str(ROLE_TEAM_LEAD).expect("agent"),
                sender: None,
                task_id: None,
                limit: None,
            })
            .expect("list");

        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].envelope.text, "one");
        assert_eq!(listed[1].envelope.text, "two");

        let loaded = backend
            .load_message(&second.message_key)
            .expect("load")
            .expect("present");
        assert_eq!(loaded.envelope.text, "two");

        backend.delete_message(&first.message_key).expect("delete");
        let listed = backend
            .list_messages(&MessageQuery {
                team: TeamName::from_str(TEST_TEAM).expect("team"),
                agent: AgentName::from_str(ROLE_TEAM_LEAD).expect("agent"),
                sender: None,
                task_id: None,
                limit: None,
            })
            .expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].envelope.text, "two");
    }

    #[test]
    fn roster_store_round_trips_config_json() {
        let tempdir = tempdir().expect("tempdir");
        let backend = ClaudeStorageBackend::new(tempdir.path().to_path_buf());
        let roster = RosterSnapshot {
            team_name: TeamName::from_str(TEST_TEAM).expect("team"),
            members: vec![RosterMember {
                team_name: TeamName::from_str(TEST_TEAM).expect("team"),
                agent_name: AgentName::from_str(ROLE_TEAM_LEAD).expect("agent"),
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: atm_storage::contract::AgentType::Lead,
                model: atm_storage::ModelName::new("claude-sonnet-4-5").expect("model"),
                recipient_pane_id: Some(atm_storage::PaneId::new("%1").expect("pane")),
                metadata_json: serde_json::Map::new(),
            }],
            refreshed_at: None,
        };

        backend.save_roster(&roster).expect("save roster");
        let loaded = backend
            .load_roster(&TeamName::from_str(TEST_TEAM).expect("team"))
            .expect("load roster");
        assert_eq!(loaded.members.len(), 1);
        assert_eq!(loaded.members[0].agent_name.as_str(), ROLE_TEAM_LEAD);

        let teams = backend.list_teams().expect("teams");
        assert_eq!(teams, vec![TeamName::from_str(TEST_TEAM).expect("team")]);
    }

    #[test]
    fn message_store_reports_io_errors() {
        let tempdir = tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        fs::create_dir_all(&team_dir).expect("team dir");
        fs::write(team_dir.join("inboxes"), "not-a-directory").expect("blocking file");

        let backend = ClaudeStorageBackend::new(tempdir.path().to_path_buf());
        let error = backend
            .save_message(&sample_message(
                TEST_TEAM,
                ROLE_TEAM_LEAD,
                TEST_SENDER,
                "one",
            ))
            .expect_err("save should fail when inboxes path is a file");
        assert!(error.message.contains("failed to create mailbox directory"));
    }

    #[test]
    fn roster_store_returns_empty_snapshot_for_missing_team() {
        let tempdir = tempdir().expect("tempdir");
        let backend = ClaudeStorageBackend::new(tempdir.path().to_path_buf());

        let loaded = backend
            .load_roster(&TeamName::from_str(TEST_TEAM).expect("team"))
            .expect("missing team should degrade to empty roster");
        assert_eq!(loaded.team_name.as_str(), TEST_TEAM);
        assert!(loaded.members.is_empty());
    }

    #[test]
    fn message_store_propagates_malformed_json_errors() {
        let tempdir = tempdir().expect("tempdir");
        let inbox_dir = tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(TEST_TEAM)
            .join("inboxes");
        fs::create_dir_all(&inbox_dir).expect("inboxes");
        fs::write(
            inbox_dir.join(format!("{ROLE_TEAM_LEAD}.json")),
            "{not-json",
        )
        .expect("inbox");

        let backend = ClaudeStorageBackend::new(tempdir.path().to_path_buf());
        let error = backend
            .list_messages(&MessageQuery {
                team: TeamName::from_str(TEST_TEAM).expect("team"),
                agent: AgentName::from_str(ROLE_TEAM_LEAD).expect("agent"),
                sender: None,
                task_id: None,
                limit: None,
            })
            .expect_err("malformed inbox must propagate as an error");
        assert!(error.message.contains("failed to parse mailbox"));
    }
}
