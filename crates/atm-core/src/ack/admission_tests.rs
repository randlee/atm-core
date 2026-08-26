#![cfg(test)]

//! Entry tests pinning `admit_acknowledgement_write` and
//! `admit_acknowledgement_write_async` before the AU.3 relocation into
//! `atm_core::write`. These are the refactor's regression net: admit,
//! reject-reacknowledge, and error paths for both variants.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Map;

use super::{admit_acknowledgement_write, admit_acknowledgement_write_async};
use crate::boundary::{Message, MessageKey, RosterEntry, RosterHarness, RosterMemberKind};
use crate::delivery_policy::DeliveryHarnessPath;
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::schema::AtmMessageId;
use crate::send::tests::{TestRuntime, message};
use crate::send::{SendMessageSource, SendRequest};
use crate::service_runtime::LocalServiceRuntime;
use crate::test_support::{TEST_SENDER, TEST_TEAM};
use crate::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::contract::{
    AcknowledgementCommit, AcknowledgementReplyBuilder, AcknowledgementSource,
};

/// The acknowledging agent: the pending source sits in this agent's mailbox
/// and was sent by `TEST_SENDER`, so the reply target is never self-addressed.
const CALLER: &str = "recipient";

fn ack_write_request(message_id: AtmMessageId) -> SendRequest {
    let home = PathBuf::from("unused-home");
    SendRequest::new(
        home.clone(),
        home,
        AgentName::from_validated(CALLER),
        &format!("{TEST_SENDER}@{TEST_TEAM}"),
        TeamName::from_validated(TEST_TEAM),
        SendMessageSource::Inline("ack reply".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("ack write request")
    .with_acknowledges_message_id(message_id)
}

/// `expect_err` needs `T: Debug`, which the admission plan type does not
/// derive; unwrap the rejection by hand instead.
fn admission_err<T>(result: Result<T, AtmError>, rejection: &str) -> AtmError {
    match result {
        Ok(_) => panic!("{rejection}"),
        Err(error) => error,
    }
}

fn pending_source(message_id: AtmMessageId) -> Message {
    let mut envelope = message(TEST_SENDER, message_id, None, None);
    envelope.requires_ack = true;
    envelope.pending_ack_at = Some(IsoTimestamp::now());
    Message {
        team: TeamName::from_validated(TEST_TEAM),
        agent: AgentName::from_validated(CALLER),
        message_key: MessageKey::from(message_id),
        envelope,
    }
}

fn runtime_with_pending_source(message_id: AtmMessageId) -> TestRuntime {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::ClaudeCode);
    runtime
        .persisted_records
        .lock()
        .expect("persisted records lock")
        .push(pending_source(message_id));
    runtime
}

#[test]
fn local_ack_admission_transitions_source_and_shapes_reply() {
    let message_id = AtmMessageId::new();
    let runtime = runtime_with_pending_source(message_id);

    let write = admit_acknowledgement_write(ack_write_request(message_id), &runtime)
        .expect("local acknowledgement admits");

    assert_eq!(write.reply.team.as_str(), TEST_TEAM);
    assert_eq!(write.reply.agent.as_str(), TEST_SENDER);
    assert_eq!(write.reply.envelope.from.as_str(), CALLER);
    assert_eq!(
        write.reply.envelope.acknowledges_message_id,
        Some(message_id)
    );
    assert!(!write.reply.envelope.requires_ack);
    let destination = write
        .canonical_request
        .to
        .as_ref()
        .expect("canonical destination resolved from the source");
    assert_eq!(destination.agent().as_str(), TEST_SENDER);
    assert_eq!(
        write.canonical_request.acknowledges_message_id,
        Some(message_id)
    );

    let records = runtime
        .persisted_records
        .lock()
        .expect("persisted records lock");
    assert_eq!(records.len(), 2, "acknowledged source plus reply");
    let source = records
        .iter()
        .find(|record| record.envelope.message_id == Some(message_id))
        .expect("acknowledged source");
    assert!(source.envelope.read);
    assert!(source.envelope.pending_ack_at.is_none());
    assert!(source.envelope.acknowledged_at.is_some());
}

#[test]
fn client_supplied_destination_is_rejected_without_peer_provenance() {
    let message_id = AtmMessageId::new();
    let runtime = runtime_with_pending_source(message_id);
    let mut request = ack_write_request(message_id);
    request.to = Some(
        format!("{TEST_SENDER}@{TEST_TEAM}")
            .parse()
            .expect("destination address"),
    );

    let error = admission_err(
        admit_acknowledgement_write(request, &runtime),
        "a client-supplied destination must be rejected",
    );

    assert!(
        error
            .message()
            .contains("peer write requests require authenticated source provenance"),
        "{error:#?}"
    );
    let records = runtime
        .persisted_records
        .lock()
        .expect("persisted records lock");
    assert_eq!(records.len(), 1, "no reply persisted");
    assert!(
        records[0].envelope.pending_ack_at.is_some(),
        "source must remain pending"
    );
}

#[test]
fn missing_acknowledges_message_id_is_rejected() {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::ClaudeCode);
    let mut request = ack_write_request(AtmMessageId::new());
    request.acknowledges_message_id = None;

    let error = admission_err(
        admit_acknowledgement_write(request, &runtime),
        "a write without an acknowledged id is not an acknowledgement",
    );

    assert!(
        error
            .message()
            .contains("acknowledgement write is missing acknowledges_message_id")
    );
}

#[test]
fn non_inline_reply_body_is_rejected() {
    let message_id = AtmMessageId::new();
    let runtime = runtime_with_pending_source(message_id);
    let mut request = ack_write_request(message_id);
    request.message_source = SendMessageSource::File {
        path: PathBuf::from("reply.txt"),
        message: None,
    };

    let error = admission_err(
        admit_acknowledgement_write(request, &runtime),
        "a file-backed reply body must be rejected",
    );

    assert!(
        error
            .message()
            .contains("acknowledgement reply body must be inline")
    );
}

#[test]
fn missing_roster_member_fails_before_storage() {
    let message_id = AtmMessageId::new();
    let mut runtime = runtime_with_pending_source(message_id);
    runtime.roster_member_missing = true;

    let error = admission_err(
        admit_acknowledgement_write(ack_write_request(message_id), &runtime),
        "an unknown caller must be rejected",
    );

    assert_eq!(error.code(), AtmErrorCode::AgentNotFound);
    assert!(
        error
            .message()
            .contains("Repair or reload the ATM roster before retrying `atm ack`.")
    );
    let records = runtime
        .persisted_records
        .lock()
        .expect("persisted records lock");
    assert_eq!(records.len(), 1, "no reply persisted");
    assert!(
        records[0].envelope.pending_ack_at.is_some(),
        "source must remain pending"
    );
}

#[test]
fn already_acknowledged_source_is_rejected() {
    let message_id = AtmMessageId::new();
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::ClaudeCode);
    let mut source = pending_source(message_id);
    source.envelope.pending_ack_at = None;
    source.envelope.acknowledged_at = Some(IsoTimestamp::now());
    runtime
        .persisted_records
        .lock()
        .expect("persisted records lock")
        .push(source);

    let error = admission_err(
        admit_acknowledgement_write(ack_write_request(message_id), &runtime),
        "re-acknowledgement must be rejected",
    );

    assert!(error.message().contains("not pending acknowledgement"));
    assert_eq!(
        runtime
            .persisted_records
            .lock()
            .expect("persisted records lock")
            .len(),
        1,
        "no reply persisted"
    );
}

// --- async variant -------------------------------------------------------

struct InMemoryAsyncStore {
    records: Mutex<Vec<Message>>,
}

impl atm_storage::contract::sealed::Sealed for InMemoryAsyncStore {}

#[allow(
    deprecated,
    reason = "admission entry tests intentionally exercise the transitional shared storage traits"
)]
impl atm_storage::MessageStore for InMemoryAsyncStore {
    fn save_message(&self, message: &Message) -> Result<(), AtmError> {
        self.records
            .lock()
            .expect("records lock")
            .push(message.clone());
        Ok(())
    }

    fn save_messages_atomically(&self, messages: &[Message]) -> Result<(), AtmError> {
        self.records
            .lock()
            .expect("records lock")
            .extend_from_slice(messages);
        Ok(())
    }

    fn acknowledge_message_atomically(
        &self,
        source: &AcknowledgementSource,
        builder: Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<AcknowledgementCommit, AtmError> {
        let mut records = self.records.lock().expect("records lock");
        let source_index = records
            .iter()
            .position(|record| {
                record.team == source.team
                    && record.agent == source.agent
                    && record.envelope.message_id == Some(source.message_id)
            })
            .ok_or_else(|| AtmError::validation("acknowledgement source was not found"))?;
        let mut acknowledged_source = records[source_index].clone();
        if acknowledged_source.envelope.pending_ack_at.is_none()
            || acknowledged_source.envelope.acknowledged_at.is_some()
        {
            return Err(AtmError::validation(
                "acknowledgement source is not pending acknowledgement",
            ));
        }
        let reply = builder.build_reply(&acknowledged_source)?;
        acknowledged_source.envelope.read = true;
        acknowledged_source.envelope.pending_ack_at = None;
        acknowledged_source.envelope.acknowledged_at = Some(IsoTimestamp::now());
        records[source_index] = acknowledged_source.clone();
        records.push(reply.clone());
        Ok(AcknowledgementCommit {
            reply,
            source: acknowledged_source,
        })
    }

    fn load_message(&self, key: &MessageKey) -> Result<Option<Message>, AtmError> {
        Ok(self
            .records
            .lock()
            .expect("records lock")
            .iter()
            .find(|record| &record.message_key == key)
            .cloned())
    }

    fn list_messages(&self, _query: &atm_storage::MessageQuery) -> Result<Vec<Message>, AtmError> {
        Ok(Vec::new())
    }

    fn delete_message(&self, _key: &MessageKey) -> Result<(), AtmError> {
        unreachable!("admission entry tests never delete a record")
    }
}

// The default async methods delegate to the synchronous implementations,
// matching the transitional composition-root wiring under test.
#[async_trait::async_trait]
impl atm_storage::AsyncMessageStore for InMemoryAsyncStore {}

struct SingleMemberRoster;

impl atm_storage::contract::sealed::Sealed for SingleMemberRoster {}

#[allow(
    deprecated,
    reason = "admission entry tests intentionally exercise the transitional shared storage traits"
)]
impl atm_storage::RosterStore for SingleMemberRoster {
    fn load_roster(&self, team: &TeamName) -> Result<atm_storage::RosterSnapshot, AtmError> {
        Ok(atm_storage::RosterSnapshot {
            team_name: team.clone(),
            members: vec![RosterEntry {
                team_name: team.clone(),
                agent_name: AgentName::from_validated(CALLER),
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: crate::schema::AgentType::default(),
                model: crate::types::ModelName::default(),
                recipient_pane_id: None,
                metadata_json: Map::new(),
            }],
            refreshed_at: None,
        })
    }

    fn save_roster(&self, _roster: &atm_storage::RosterSnapshot) -> Result<(), AtmError> {
        unreachable!("admission entry tests never mutate the roster")
    }

    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
        unreachable!("admission entry tests never enumerate teams")
    }
}

struct NoopNudgeTemplateOverrideStore;

impl atm_storage::contract::sealed::Sealed for NoopNudgeTemplateOverrideStore {}

impl crate::boundary::NudgeTemplateOverrideStore for NoopNudgeTemplateOverrideStore {
    fn load_template_override(
        &self,
        _team: &TeamName,
        _kind: crate::boundary::BuiltInNudgeTemplateKind,
    ) -> Result<Option<crate::boundary::TeamNudgeTemplateOverrideRow>, AtmError> {
        Ok(None)
    }

    fn save_template_override(
        &self,
        _team: &TeamName,
        _kind: crate::boundary::BuiltInNudgeTemplateKind,
        _template_body: &str,
    ) -> Result<crate::boundary::TeamNudgeTemplateOverrideRow, AtmError> {
        unreachable!("admission entry tests never touch the override-store boundary")
    }

    fn disable_template_override(
        &self,
        _team: &TeamName,
        _kind: crate::boundary::BuiltInNudgeTemplateKind,
    ) -> Result<crate::boundary::TeamNudgeTemplateOverrideRow, AtmError> {
        unreachable!("admission entry tests never touch the override-store boundary")
    }

    fn clear_template_override(
        &self,
        _team: &TeamName,
        _kind: crate::boundary::BuiltInNudgeTemplateKind,
    ) -> Result<bool, AtmError> {
        unreachable!("admission entry tests never touch the override-store boundary")
    }
}

fn local_runtime(store: Arc<InMemoryAsyncStore>, attach_async_store: bool) -> LocalServiceRuntime {
    let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        store.clone(),
        Arc::new(SingleMemberRoster),
        Arc::new(NoopNudgeTemplateOverrideStore),
        Arc::new(crate::LocalFileNonClaudeOutbound::new()),
    );
    if attach_async_store {
        runtime.with_async_message_store(store)
    } else {
        runtime
    }
}

/// Minimal executor: with in-memory stores the admission future never pends,
/// so a noop-waker poll loop is sufficient and atm-core keeps its zero
/// tokio dev-dependency footprint.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(output) => return output,
            std::task::Poll::Pending => std::thread::yield_now(),
        }
    }
}

#[test]
fn async_admission_matches_sync_behavior_for_local_ack() {
    let message_id = AtmMessageId::new();
    let store = Arc::new(InMemoryAsyncStore {
        records: Mutex::new(vec![pending_source(message_id)]),
    });
    let runtime = local_runtime(store.clone(), true);

    let write = block_on(admit_acknowledgement_write_async(
        ack_write_request(message_id),
        &runtime,
    ))
    .expect("async local acknowledgement admits");

    assert_eq!(write.reply.agent.as_str(), TEST_SENDER);
    assert_eq!(write.reply.envelope.from.as_str(), CALLER);
    assert_eq!(
        write.reply.envelope.acknowledges_message_id,
        Some(message_id)
    );
    let records = store.records.lock().expect("records lock");
    assert_eq!(records.len(), 2, "acknowledged source plus reply");
    let source = records
        .iter()
        .find(|record| record.envelope.message_id == Some(message_id))
        .expect("acknowledged source");
    assert!(source.envelope.pending_ack_at.is_none());
    assert!(source.envelope.acknowledged_at.is_some());
}

#[test]
fn async_admission_requires_installed_async_store() {
    let message_id = AtmMessageId::new();
    let store = Arc::new(InMemoryAsyncStore {
        records: Mutex::new(vec![pending_source(message_id)]),
    });
    let runtime = local_runtime(store.clone(), false);

    let error = admission_err(
        block_on(admit_acknowledgement_write_async(
            ack_write_request(message_id),
            &runtime,
        )),
        "a runtime without the async store must refuse admission",
    );

    assert!(
        error
            .message()
            .contains("Tokio acknowledgement admission was not installed in this runtime")
    );
    let records = store.records.lock().expect("records lock");
    assert_eq!(records.len(), 1, "no reply persisted");
    assert!(
        records[0].envelope.pending_ack_at.is_some(),
        "source must remain pending"
    );
}
