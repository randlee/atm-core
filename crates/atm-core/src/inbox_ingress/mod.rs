use std::path::Path;

use crate::error::{AtmError, AtmErrorKind};
use crate::mail_store::{
    AckStateRecord, ImportedMessageState, IngestRecord, MailStore, MessageSourceKind,
    StoredMessageRecord, VisibilityStateRecord,
};
use crate::mailbox::{self, MailboxReadReport};
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::schema::MessageEnvelope;
use crate::store::{InsertOutcome, MessageKey, SourceFingerprint, StoreError, StoreErrorKind};
use crate::task_store::{TaskRecord, TaskStatus, TaskStore};
use crate::types::{AgentName, IsoTimestamp, TeamName};
use crate::workflow::{self, WorkflowStateFile};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngestOutcome {
    pub imported_messages: usize,
    pub duplicate_messages: usize,
    pub degraded_records: usize,
}

pub trait InboxIngestStore: MailStore + TaskStore {}

impl<T> InboxIngestStore for T where T: MailStore + TaskStore + ?Sized {}

pub trait InboxIngress {
    fn ingest_mailbox_state(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        store: &dyn InboxIngestStore,
        observability: &dyn ObservabilityPort,
    ) -> Result<InboxIngestOutcome, AtmError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct JsonInboxIngress;

pub fn default_inbox_ingress() -> JsonInboxIngress {
    JsonInboxIngress
}

impl InboxIngress for JsonInboxIngress {
    fn ingest_mailbox_state(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        store: &dyn InboxIngestStore,
        observability: &dyn ObservabilityPort,
    ) -> Result<InboxIngestOutcome, AtmError> {
        let workflow_state =
            workflow::load_workflow_state(home_dir, team.as_str(), agent.as_str())?;
        // INVARIANT: discover_source_paths() is bounded to the validated
        // home/team/agent scope provided to this ingress call.
        let source_paths =
            mailbox::source::discover_source_paths(home_dir, team.as_str(), agent.as_str())?;
        let mut outcome = InboxIngestOutcome::default();

        for source_path in source_paths {
            let report = mailbox::read_messages_report(&source_path)?;
            outcome.degraded_records +=
                report.stats.skipped_records + report.stats.malformed_metadata_records;
            for _ in 0..report.stats.skipped_records {
                let _ = observability.emit(CommandEvent {
                    command: "inbox_ingress",
                    action: "import_record",
                    outcome: "skipped_record",
                    team: team.clone(),
                    agent: agent.clone(),
                    sender: AgentName::system(),
                    message_id: None,
                    requires_ack: false,
                    dry_run: false,
                    task_id: None,
                    error_code: Some(crate::error::AtmErrorCode::WarningMailboxRecordSkipped),
                    error_message: Some(format!(
                        "skipped malformed inbox record while importing {}",
                        source_path.display()
                    )),
                });
            }
            for _ in 0..report.stats.malformed_metadata_records {
                let _ = observability.emit(CommandEvent {
                    command: "inbox_ingress",
                    action: "import_record",
                    outcome: "malformed_metadata",
                    team: team.clone(),
                    agent: agent.clone(),
                    sender: AgentName::system(),
                    message_id: None,
                    requires_ack: false,
                    dry_run: false,
                    task_id: None,
                    error_code: Some(crate::error::AtmErrorCode::WarningMalformedAtmFieldIgnored),
                    error_message: Some(format!(
                        "ignored malformed metadata while importing {}",
                        source_path.display()
                    )),
                });
            }
            ingest_source_report(
                &source_path,
                report,
                team,
                agent,
                &workflow_state,
                store,
                &mut outcome,
            )?;
        }

        let _ = observability.emit(CommandEvent {
            command: "inbox_ingress",
            action: "import",
            outcome: if outcome.degraded_records > 0 {
                "degraded"
            } else {
                "ok"
            },
            team: team.clone(),
            agent: agent.clone(),
            sender: AgentName::system(),
            message_id: None,
            requires_ack: false,
            dry_run: false,
            task_id: None,
            error_code: None,
            error_message: None,
        });

        Ok(outcome)
    }
}
fn ingest_source_report(
    source_path: &Path,
    report: MailboxReadReport,
    team: &TeamName,
    agent: &AgentName,
    workflow_state: &WorkflowStateFile,
    store: &dyn InboxIngestStore,
    outcome: &mut InboxIngestOutcome,
) -> Result<(), AtmError> {
    for envelope in report.messages {
        let message_key = canonical_message_key(source_path, &envelope);
        let stored = stored_message_record(message_key.clone(), team, agent, &envelope)?;
        let ingest_record = IngestRecord {
            team_name: team.clone(),
            recipient_agent: agent.clone(),
            source_path: source_path.to_path_buf(),
            source_fingerprint: source_fingerprint(source_path, &envelope),
            message_key: message_key.clone(),
            imported_at: IsoTimestamp::now(),
        };
        let imported_state = imported_message_state(&message_key, &envelope, workflow_state);
        match store
            .insert_message_with_ingest_state(&stored, &ingest_record, &imported_state)
            .map_err(|error| map_store_error("failed to insert imported mailbox row", error))?
        {
            InsertOutcome::Inserted(_) => outcome.imported_messages += 1,
            InsertOutcome::Duplicate(_) => outcome.duplicate_messages += 1,
        }
    }
    Ok(())
}

fn imported_message_state(
    message_key: &MessageKey,
    envelope: &MessageEnvelope,
    workflow_state: &WorkflowStateFile,
) -> ImportedMessageState {
    let projected = workflow::workflow_key(envelope)
        .and_then(|key| workflow_state.messages.get(&key).cloned())
        .unwrap_or_else(|| workflow::initial_state_for_envelope(envelope));

    ImportedMessageState {
        ack_state: (projected.pending_ack_at.is_some() || projected.acknowledged_at.is_some())
            .then(|| AckStateRecord {
                message_key: message_key.clone(),
                pending_ack_at: projected.pending_ack_at,
                acknowledged_at: projected.acknowledged_at,
                ack_reply_message_key: None,
                ack_reply_team: None,
                ack_reply_agent: None,
            }),
        visibility: projected.read.then(|| VisibilityStateRecord {
            message_key: message_key.clone(),
            read_at: Some(envelope.timestamp),
            cleared_at: None,
        }),
        task: envelope.task_id.clone().map(|task_id| TaskRecord {
            task_id,
            message_key: message_key.clone(),
            status: if projected.acknowledged_at.is_some() {
                TaskStatus::Acknowledged
            } else {
                TaskStatus::PendingAck
            },
            created_at: envelope.timestamp,
            acknowledged_at: projected.acknowledged_at,
            metadata_json: None,
        }),
    }
}

fn canonical_message_key(source_path: &Path, envelope: &MessageEnvelope) -> MessageKey {
    if let Some(atm_message_id) = envelope.atm_message_id() {
        return MessageKey::from_atm_message_id(atm_message_id);
    }
    if let Some(message_id) = envelope.message_id {
        return MessageKey::from_legacy_message_id(message_id);
    }
    MessageKey::from_source_fingerprint(&source_fingerprint(source_path, envelope))
}

fn source_fingerprint(source_path: &Path, envelope: &MessageEnvelope) -> SourceFingerprint {
    if let Some(atm_message_id) = envelope.atm_message_id() {
        // INVARIANT: AtmMessageId is always a valid ULID-format fingerprint source.
        return atm_message_id.into();
    }
    if let Some(message_id) = envelope.message_id {
        // INVARIANT: LegacyMessageId always produces a valid fingerprint.
        return message_id.into();
    }

    let mut hash = 0xcbf29ce484222325_u64;
    for segment in [
        source_path.to_string_lossy().replace('\\', "/"),
        envelope.from.to_string(),
        envelope.timestamp.to_string(),
        envelope.summary.clone().unwrap_or_default(),
        envelope.text.clone(),
    ] {
        for byte in segment.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= u64::from(b'|');
        hash = hash.wrapping_mul(0x100000001b3);
    }

    // INVARIANT: FNV hash of canonical path+message fields always produces a
    // valid hex fingerprint.
    SourceFingerprint::from_external_hex(hash)
}

fn stored_message_record(
    message_key: MessageKey,
    team: &TeamName,
    agent: &AgentName,
    envelope: &MessageEnvelope,
) -> Result<StoredMessageRecord, AtmError> {
    let raw_metadata_json = envelope
        .extra
        .get("metadata")
        .map(serde_json::to_string)
        .transpose()
        .map_err(|source| {
            AtmError::new(
                AtmErrorKind::Serialization,
                format!(
                    "failed to encode metadata for imported inbox message from {}",
                    envelope.from
                ),
            )
            .with_recovery(
                "Repair the imported metadata payload or re-read the source inbox after the sender/export path emits valid JSON metadata.",
            )
            .with_source(source)
        })?;

    Ok(StoredMessageRecord {
        message_key: message_key.clone(),
        team_name: team.clone(),
        recipient_agent: agent.clone(),
        sender_display: envelope.from.to_string(),
        sender_canonical: sender_canonical(envelope),
        sender_team: envelope.source_team.clone(),
        body: envelope.text.clone(),
        summary: envelope.summary.clone(),
        created_at: envelope.timestamp,
        source_kind: match message_key.source_kind() {
            crate::store::MessageKeySource::Atm => MessageSourceKind::Atm,
            crate::store::MessageKeySource::Legacy => MessageSourceKind::Legacy,
            crate::store::MessageKeySource::External => MessageSourceKind::External,
        },
        legacy_message_id: envelope.message_id,
        atm_message_id: envelope.atm_message_id(),
        raw_metadata_json,
    })
}

fn sender_canonical(envelope: &MessageEnvelope) -> Option<AgentName> {
    envelope
        .extra
        .get("metadata")
        .and_then(serde_json::Value::as_object)
        .and_then(|metadata| metadata.get("atm"))
        .and_then(serde_json::Value::as_object)
        .and_then(|atm| atm.get("fromIdentity"))
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.parse().ok())
}

fn map_store_error(context: &str, error: StoreError) -> AtmError {
    let kind = match error.kind {
        StoreErrorKind::Busy => AtmErrorKind::Timeout,
        StoreErrorKind::Constraint => AtmErrorKind::Validation,
        _ => AtmErrorKind::MailboxWrite,
    };
    let mut atm_error =
        AtmError::new_with_code(error.code, kind, format!("{context}: {}", error.message));
    if let Some(recovery) = error.recovery.as_ref() {
        atm_error = atm_error.with_recovery(recovery.clone());
    }
    atm_error.with_source(error)
}
