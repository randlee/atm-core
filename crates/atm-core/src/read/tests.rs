use std::path::PathBuf;

use serde_json::Map;
use tempfile::tempdir;

#[path = "../../tests/support.rs"]
mod support;

use super::{ReadQuery, selected_after_filters};
use crate::error_codes::AtmErrorCode;
use crate::mailbox::source::SourcedMessage;
use crate::read::projection::idle_notification_sender;
use crate::schema::{LegacyMessageId, MessageEnvelope};
use crate::types::{
    AckActivationMode, AgentName, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection,
    TeamName,
};
use crate::workflow;

use support::ROLE_TEAM_LEAD;

const TEST_TEAM: &str = "test-team";
const TEST_SOURCE_FILE: &str = "test-sender.json";

fn malformed_idle_notification() -> String {
    format!(
        r#"{{"type":"idle_notification","from":"{}""#,
        ROLE_TEAM_LEAD
    )
}

fn sourced_message(index: usize, text: &str) -> SourcedMessage {
    SourcedMessage {
        envelope: MessageEnvelope {
            from: ROLE_TEAM_LEAD.parse::<AgentName>().expect("agent"),
            text: text.to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: Some(LegacyMessageId::new()),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: None,
            extra: Map::new(),
        },
        source_path: PathBuf::from(TEST_SOURCE_FILE),
        source_index: index.into(),
    }
}

#[test]
fn idle_notification_sender_returns_none_for_malformed_json() {
    let message = sourced_message(0, &malformed_idle_notification());

    assert_eq!(idle_notification_sender(&message.envelope), None);
}

#[test]
fn malformed_idle_notification_adjacent_to_valid_records_remains_readable_and_classifiable() {
    let workflow_state = workflow::WorkflowStateFile::default();
    let malformed_idle = malformed_idle_notification();
    let messages = vec![
        sourced_message(0, &malformed_idle),
        sourced_message(1, "normal unread"),
    ];
    let query = ReadQuery {
        home_dir: PathBuf::new(),
        current_dir: PathBuf::new(),
        actor_override: None,
        target_address: None,
        team_override: None,
        selection_mode: ReadSelection::All,
        seen_state_filter: false,
        seen_state_update: false,
        ack_activation_mode: AckActivationMode::ReadOnly,
        limit: None,
        sender_filter: None,
        timestamp_filter: None,
        timeout_secs: None,
    };

    let selected = std::panic::catch_unwind(|| {
        selected_after_filters(&messages, &workflow_state, &query, None)
    })
    .expect("malformed idle notification should not panic");

    assert_eq!(selected.len(), 2);
    let valid = selected
        .iter()
        .find(|message| message.envelope.text == "normal unread")
        .expect("valid record");
    assert_eq!(valid.class, MessageClass::Unread);
    assert_eq!(valid.bucket, DisplayBucket::Unread);

    let malformed = selected
        .iter()
        .find(|message| message.envelope.text == malformed_idle)
        .expect("malformed record");
    assert_eq!(malformed.class, MessageClass::Unread);
    assert_eq!(malformed.bucket, DisplayBucket::Unread);
}

#[test]
fn read_query_new_rejects_invalid_target_before_command_execution() {
    let tempdir = tempdir().expect("tempdir");
    let error = ReadQuery::new(
        tempdir.path().to_path_buf(),
        tempdir.path().to_path_buf(),
        Some("test-sender"),
        Some("../evil"),
        Some(TEST_TEAM),
        ReadSelection::Actionable,
        false,
        false,
        AckActivationMode::ReadOnly,
        None,
        None,
        None,
        None,
    )
    .expect_err("invalid target");

    assert_eq!(error.code, AtmErrorCode::AddressParseFailed);
    assert!(error.message.contains("agent name"));
}

#[test]
fn read_query_new_rejects_invalid_actor_before_command_execution() {
    let tempdir = tempdir().expect("tempdir");
    let error = ReadQuery::new(
        tempdir.path().to_path_buf(),
        tempdir.path().to_path_buf(),
        Some("../evil"),
        None,
        Some(TEST_TEAM),
        ReadSelection::Actionable,
        false,
        false,
        AckActivationMode::ReadOnly,
        None,
        None,
        None,
        None,
    )
    .expect_err("invalid actor");

    assert_eq!(error.code, AtmErrorCode::AddressParseFailed);
    assert!(error.message.contains("agent name"));
}
