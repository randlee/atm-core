//! ATM-owned mailbox workflow sidecar helpers.
//!
//! This module owns the workflow source-of-truth file family under
//! `.claude/teams/<team>/.atm-state/workflow/<agent>.json`. Read/ack/clear may
//! project these fields onto the Claude-owned inbox surface, but command-layer
//! code must not shape or persist workflow JSON directly.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{AtmError, AtmErrorKind};
use crate::home;
use crate::mailbox::lock;
use crate::persistence;
use crate::schema::MessageEnvelope;
use crate::types::{AgentName, IsoTimestamp, TeamName};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct WorkflowStateFile {
    #[serde(default)]
    pub messages: BTreeMap<String, WorkflowMessageState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct WorkflowMessageState {
    #[serde(default, skip_serializing_if = "is_false")]
    pub read: bool,

    #[serde(rename = "pendingAckAt", skip_serializing_if = "Option::is_none")]
    pub pending_ack_at: Option<IsoTimestamp>,

    #[serde(rename = "acknowledgedAt", skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

pub(crate) fn load_workflow_state(
    home_dir: &Path,
    team: &str,
    agent: &str,
) -> Result<WorkflowStateFile, AtmError> {
    let team: TeamName = team.parse()?;
    let agent: AgentName = agent.parse()?;
    let path = home::workflow_state_path_from_home(home_dir, &team, &agent)?;
    if !path.exists() {
        return Ok(WorkflowStateFile::default());
    }

    let raw = fs::read_to_string(&path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::MailboxRead,
            format!("failed to read workflow state {}: {error}", path.display()),
        )
        .with_recovery(
            "Check workflow-state file permissions or remove the malformed workflow state file before retrying the ATM command.",
        )
        .with_source(error)
    })?;

    serde_json::from_str(&raw).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Serialization,
            format!("invalid workflow state {}: {error}", path.display()),
        )
        .with_recovery(
            "Remove or repair the malformed workflow state file so ATM can rebuild it on the next successful command.",
        )
        .with_source(error)
    })
}

pub(crate) fn save_workflow_state(
    home_dir: &Path,
    team: &str,
    agent: &str,
    state: &WorkflowStateFile,
) -> Result<(), AtmError> {
    let team: TeamName = team.parse()?;
    let agent: AgentName = agent.parse()?;
    let path = home::workflow_state_path_from_home(home_dir, &team, &agent)?;
    let encoded = serde_json::to_string_pretty(state).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Serialization,
            format!("failed to encode workflow state {}: {error}", path.display()),
        )
        .with_recovery(
            "Retry after removing unsupported workflow-state values or repairing the local ATM state.",
        )
        .with_source(error)
    })?;
    persistence::atomic_write_string(
        &path,
        &encoded,
        AtmErrorKind::MailboxWrite,
        "workflow state",
        "Check workflow-state directory permissions and retry the ATM command.",
    )
}

pub(crate) fn commit_workflow_state<T, I, F>(
    home_dir: &Path,
    team: &str,
    agent: &str,
    extra_write_paths: I,
    timeout: Duration,
    body: F,
) -> Result<T, AtmError>
where
    I: IntoIterator<Item = PathBuf>,
    F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), AtmError>,
{
    let team_name: TeamName = team.parse()?;
    let agent_name: AgentName = agent.parse()?;
    let workflow_path = home::workflow_state_path_from_home(home_dir, &team_name, &agent_name)?;
    let mut write_paths = vec![workflow_path];
    write_paths.extend(extra_write_paths);
    let _locks = lock::acquire_many_sorted(write_paths, timeout)?;
    let mut workflow_state = load_workflow_state(home_dir, team, agent)?;
    let (result, changed) = body(&mut workflow_state)?;
    if changed {
        save_workflow_state(home_dir, team, agent, &workflow_state)?;
    }
    Ok(result)
}

pub(crate) fn project_envelope(
    envelope: &MessageEnvelope,
    workflow_state: &WorkflowStateFile,
) -> MessageEnvelope {
    // Projection is the guardrail: higher-level services classify mailbox
    // state from this joined view instead of re-deriving workflow durability
    // from the Claude-owned inbox record.
    let Some(key) = workflow_key(envelope) else {
        return envelope.clone();
    };
    let Some(projected) = workflow_state.messages.get(&key) else {
        return envelope.clone();
    };

    let mut projected_envelope = envelope.clone();
    projected_envelope.read = projected.read;
    projected_envelope.pending_ack_at = projected.pending_ack_at;
    projected_envelope.acknowledged_at = projected.acknowledged_at;
    projected_envelope
}

pub(crate) fn apply_projected_state(
    workflow_state: &mut WorkflowStateFile,
    original: &MessageEnvelope,
    projected: &MessageEnvelope,
) -> bool {
    // Persist only the projected workflow axes here. Callers keep any inbox
    // compatibility rewrite separate so the workflow sidecar stays the single
    // owner-layer write boundary for ATM-local durability.
    let Some(key) = workflow_key(original) else {
        return false;
    };

    let next_state = WorkflowMessageState {
        read: projected.read,
        pending_ack_at: projected.pending_ack_at,
        acknowledged_at: projected.acknowledged_at,
    };
    if workflow_state.messages.get(&key) == Some(&next_state) {
        return false;
    }
    workflow_state.messages.insert(key, next_state);
    true
}

pub(crate) fn remove_message_state(
    workflow_state: &mut WorkflowStateFile,
    envelope: &MessageEnvelope,
) -> bool {
    workflow_key(envelope)
        .and_then(|key| workflow_state.messages.remove(&key))
        .is_some()
}

pub(crate) fn workflow_key(envelope: &MessageEnvelope) -> Option<String> {
    envelope
        .message_id
        .map(|message_id| format!("legacy:{message_id}"))
}

pub(crate) fn initial_state_for_envelope(envelope: &MessageEnvelope) -> WorkflowMessageState {
    WorkflowMessageState {
        read: envelope.read,
        pending_ack_at: envelope.pending_ack_at,
        acknowledged_at: envelope.acknowledged_at,
    }
}

pub(crate) fn remember_initial_state(
    workflow_state: &mut WorkflowStateFile,
    envelope: &MessageEnvelope,
) -> bool {
    let Some(key) = workflow_key(envelope) else {
        return false;
    };
    let next_state = initial_state_for_envelope(envelope);
    if workflow_state.messages.get(&key) == Some(&next_state) {
        return false;
    }
    workflow_state.messages.insert(key, next_state);
    true
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{
        WorkflowMessageState, apply_projected_state, load_workflow_state, project_envelope,
        remember_initial_state, remove_message_state, save_workflow_state, workflow_key,
    };
    use crate::schema::{AtmMessageId, MessageEnvelope};
    use crate::test_support::{TEST_LEAD, TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    fn sample_message() -> MessageEnvelope {
        MessageEnvelope {
            from: TEST_LEAD.parse::<AgentName>().expect("agent"),
            text: "hello".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: Some(AtmMessageId::new()),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            stale_at: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn load_missing_workflow_state_returns_default() {
        let tempdir = TempDir::new().expect("tempdir");
        let state =
            load_workflow_state(tempdir.path(), TEST_TEAM, TEST_SENDER).expect("load state");

        assert!(state.messages.is_empty());
    }

    #[test]
    fn save_and_load_workflow_state_round_trips() {
        let tempdir = TempDir::new().expect("tempdir");
        let mut state = super::WorkflowStateFile::default();
        state.messages.insert(
            "legacy:test".to_string(),
            WorkflowMessageState {
                read: true,
                pending_ack_at: None,
                acknowledged_at: None,
            },
        );

        save_workflow_state(tempdir.path(), TEST_TEAM, TEST_SENDER, &state).expect("save state");
        let loaded =
            load_workflow_state(tempdir.path(), TEST_TEAM, TEST_SENDER).expect("load state");

        assert_eq!(loaded, state);
    }

    #[test]
    fn workflow_key_uses_message_id() {
        let message = sample_message();

        assert_eq!(
            workflow_key(&message),
            message
                .message_id
                .map(|message_id| format!("legacy:{message_id}"))
        );
    }

    #[test]
    fn project_envelope_prefers_sidecar_state() {
        let message = sample_message();
        let message_id = message.message_id.expect("legacy message id");
        let mut state = super::WorkflowStateFile::default();
        state.messages.insert(
            format!("legacy:{message_id}"),
            WorkflowMessageState {
                read: true,
                pending_ack_at: Some(IsoTimestamp::now()),
                acknowledged_at: None,
            },
        );

        let projected = project_envelope(&message, &state);

        assert!(projected.read);
        assert!(projected.pending_ack_at.is_some());
    }

    #[test]
    fn apply_and_remove_projected_state_updates_sidecar() {
        let message = sample_message();
        let mut projected = message.clone();
        projected.read = true;
        let mut state = super::WorkflowStateFile::default();

        assert!(apply_projected_state(&mut state, &message, &projected));
        assert!(
            state
                .messages
                .get(&workflow_key(&message).expect("workflow key"))
                .expect("entry")
                .read
        );
        assert!(remove_message_state(&mut state, &message));
        assert!(state.messages.is_empty());
    }

    #[test]
    fn remember_initial_state_creates_entry_for_identified_message() {
        let message = sample_message();
        let mut state = super::WorkflowStateFile::default();

        assert!(remember_initial_state(&mut state, &message));
        assert_eq!(state.messages.len(), 1);
    }
}
