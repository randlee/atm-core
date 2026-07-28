use super::stmt_cache::WriterStatementCache;
use crate::shared_db::{SharedDbTarget, serialize_json, sqlite_thread_mode};
use atm_storage::contract::Message;
use atm_storage::error::AtmError;
use atm_storage::schema::MessageEnvelope;
use atm_storage::types::IsoTimestamp;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

pub(crate) const MAX_ENVELOPE_JSON_BYTES: usize = 1_048_576;

#[derive(Debug, Clone)]
pub(crate) enum WriteOp {
    UpsertMessage(Box<Message>),
    /// A related group of immutable records that must either all become
    /// visible or none do.  AI.31 uses this for the ACK reply and the
    /// acknowledged source record.
    UpsertMessages(Vec<Message>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WriteOpResult {
    UpsertMessage { inserted: bool },
    UpsertMessages,
}

pub(crate) fn execute(
    op: &WriteOp,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    match op {
        WriteOp::UpsertMessage(request) => {
            execute_upsert_message(request, connection, cache, target)
        }
        WriteOp::UpsertMessages(records) => {
            for record in records {
                let _ = execute_upsert_message(record, connection, cache, target)?;
            }
            Ok(WriteOpResult::UpsertMessages)
        }
    }
}

pub(crate) fn validate_upsert_message_request(record: &Message) -> Result<(), AtmError> {
    let envelope_json = serialize_json(
        &StorageEnvelope::new(&record.envelope),
        "mail-store envelope",
    )?;
    if envelope_json.len() > MAX_ENVELOPE_JSON_BYTES {
        return Err(AtmError::validation(format!(
            "mail-store envelope JSON exceeded the writer lane limit of {MAX_ENVELOPE_JSON_BYTES} bytes"
        )));
    }
    Ok(())
}

fn execute_upsert_message(
    record: &Message,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    let envelope_json = serialize_json(
        &StorageEnvelope::new(&record.envelope),
        "mail-store envelope",
    )?;
    validate_message_record(record, envelope_json.len(), connection, cache, target)?;
    let parent_message_id = record
        .envelope
        .parent_message_id
        .as_ref()
        .map(ToString::to_string);
    let thread_mode = sqlite_thread_mode(record.envelope.thread_mode);
    // Accepted risk: `IsoTimestamp` is ATM's validated UTC timestamp newtype,
    // so column writes can reuse its canonical RFC3339 rendering directly.
    let expires_at = record.envelope.expires_at.map(rfc3339);
    let pending_ack_at = record
        .envelope
        .pending_ack_at
        .map(|value: IsoTimestamp| value.into_inner().to_rfc3339());
    let acknowledged_at = record
        .envelope
        .acknowledged_at
        .map(|value: IsoTimestamp| value.into_inner().to_rfc3339());
    let from_agent = record.envelope.from.to_string();
    let source_chat_id = record
        .envelope
        .source_chat_id
        .as_ref()
        .map(ToString::to_string);
    let destination_chat_id = record
        .envelope
        .destination_chat_id
        .as_ref()
        .map(ToString::to_string);
    let message_text = record.envelope.text.clone();
    let summary = record.envelope.summary.clone();
    let message_at = record.envelope.timestamp.into_inner().to_rfc3339();
    let message_id = record.envelope.message_id.as_ref().map(ToString::to_string);
    // Ingest timing is owned by the durable store, not by callers (ADR-005).
    let recorded_at = IsoTimestamp::now().into_inner().to_rfc3339();

    let inserted = cache
        .insert_message_row(
            connection,
            params![
                record.team.as_str(),
                record.agent.as_str(),
                record.message_key.as_ref(),
                envelope_json,
                from_agent,
                source_chat_id,
                destination_chat_id,
                message_text,
                summary,
                message_at,
                message_id,
                parent_message_id,
                thread_mode,
                recorded_at.clone(),
            ],
        )
        .map_err(|error| {
            crate::shared_db::sqlite_error(target, "failed to upsert mail-store message", error)
        })?
        == 1;
    let timestamps =
        initial_state_timestamps(pending_ack_at, acknowledged_at, expires_at, recorded_at);
    insert_initial_message_state(connection, cache, target, record, timestamps)?;

    Ok(WriteOpResult::UpsertMessage { inserted })
}

struct InitialStateTimestamps {
    pending_ack_at: Option<String>,
    acknowledged_at: Option<String>,
    expires_at: Option<String>,
    recorded_at: String,
}

fn initial_state_timestamps(
    pending_ack_at: Option<String>,
    acknowledged_at: Option<String>,
    expires_at: Option<String>,
    recorded_at: String,
) -> InitialStateTimestamps {
    InitialStateTimestamps {
        pending_ack_at,
        acknowledged_at,
        expires_at,
        recorded_at,
    }
}

fn insert_initial_message_state(
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
    record: &Message,
    timestamps: InitialStateTimestamps,
) -> Result<(), AtmError> {
    cache
        .upsert_message_state(
            connection,
            params![
                record.team.as_str(),
                record.agent.as_str(),
                record.message_key.as_ref(),
                i64::from(record.envelope.read),
                timestamps.pending_ack_at,
                timestamps.acknowledged_at,
                timestamps.expires_at,
                Option::<String>::None,
                timestamps.recorded_at,
            ],
        )
        .map_err(|error| {
            crate::shared_db::sqlite_error(target, "failed to upsert mail message state row", error)
        })?;
    Ok(())
}

fn validate_message_record(
    record: &Message,
    envelope_json_len: usize,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    if envelope_json_len > MAX_ENVELOPE_JSON_BYTES {
        return Err(AtmError::validation(format!(
            "mail-store envelope JSON exceeded the writer lane limit of {MAX_ENVELOPE_JSON_BYTES} bytes"
        )));
    }

    let message_key = record.message_key.as_ref();
    if !message_key.starts_with("atm:") && !message_key.starts_with("ext:") {
        return Err(AtmError::validation(format!(
            "mail-store message_key must start with `atm:` or `ext:`; got `{message_key}`"
        )));
    }

    validate_single_successor_invariant(record, connection, cache, target)?;
    validate_message_id_uniqueness(record, connection, cache, target)?;

    Ok(())
}

fn validate_single_successor_invariant(
    record: &Message,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let message_key = record.message_key.as_ref();
    if let Some(parent_message_id) = record.envelope.parent_message_id {
        let owner = cache
            .load_successor_owner(
                connection,
                params![
                    record.team.as_str(),
                    record.agent.as_str(),
                    parent_message_id.to_string()
                ],
            )
            .optional()
            .map_err(|error| {
                crate::shared_db::sqlite_error(
                    target,
                    "failed to validate single-successor mail-store invariant",
                    error,
                )
            })?;
        if let Some(owner) = owner
            && owner != message_key
        {
            return Err(AtmError::validation(format!(
                "mail-store parent message `{parent_message_id}` already has successor `{owner}`; `{message_key}` would violate the single-successor invariant"
            )));
        }
    }
    Ok(())
}

fn validate_message_id_uniqueness(
    record: &Message,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let message_key = record.message_key.as_ref();
    if let Some(message_id) = record.envelope.message_id {
        let owner = cache
            .load_message_id_owner(
                connection,
                params![
                    record.team.as_str(),
                    record.agent.as_str(),
                    message_id.to_string()
                ],
            )
            .optional()
            .map_err(|error| {
                crate::shared_db::sqlite_error(
                    target,
                    "failed to validate message identity uniqueness",
                    error,
                )
            })?;
        if let Some(owner) = owner
            && owner != message_key
        {
            return Err(AtmError::validation(format!(
                "message_id `{message_id}` is already owned by `{owner}` and cannot be reassigned to `{message_key}`"
            )));
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct StorageEnvelope<'a> {
    from: &'a atm_storage::types::AgentName,
    #[serde(
        rename = "sourceChatId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    source_chat_id: &'a Option<atm_storage::types::ChatId>,
    text: &'a str,
    timestamp: IsoTimestamp,
    read: bool,
    #[serde(default)]
    source_team: &'a Option<atm_storage::types::TeamName>,
    #[serde(
        rename = "destinationChatId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    destination_chat_id: &'a Option<atm_storage::types::ChatId>,
    #[serde(default)]
    summary: &'a Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message_id: Option<String>,
    #[serde(rename = "pendingAckAt", skip_serializing_if = "Option::is_none")]
    pending_ack_at: Option<IsoTimestamp>,
    #[serde(rename = "acknowledgedAt", skip_serializing_if = "Option::is_none")]
    acknowledged_at: Option<IsoTimestamp>,
    #[serde(
        rename = "acknowledgesMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    acknowledges_message_id: Option<String>,
    #[serde(
        rename = "parentMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    parent_message_id: Option<String>,
    #[serde(rename = "threadMode", skip_serializing_if = "Option::is_none")]
    thread_mode: &'a Option<atm_storage::schema::ThreadMode>,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    expires_at: Option<IsoTimestamp>,
    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    task_id: &'a Option<atm_storage::types::TaskId>,
    #[serde(flatten)]
    extra: &'a serde_json::Map<String, serde_json::Value>,
}

impl<'a> StorageEnvelope<'a> {
    fn new(envelope: &'a MessageEnvelope) -> Self {
        Self {
            from: &envelope.from,
            source_chat_id: &envelope.source_chat_id,
            text: envelope.text.as_str(),
            timestamp: envelope.timestamp,
            read: envelope.read,
            source_team: &envelope.source_team,
            destination_chat_id: &envelope.destination_chat_id,
            summary: &envelope.summary,
            message_id: envelope.message_id.as_ref().map(ToString::to_string),
            pending_ack_at: envelope.pending_ack_at,
            acknowledged_at: envelope.acknowledged_at,
            acknowledges_message_id: envelope
                .acknowledges_message_id
                .as_ref()
                .map(ToString::to_string),
            parent_message_id: envelope.parent_message_id.as_ref().map(ToString::to_string),
            thread_mode: &envelope.thread_mode,
            expires_at: envelope.expires_at,
            task_id: &envelope.task_id,
            extra: &envelope.extra,
        }
    }
}

fn rfc3339(value: IsoTimestamp) -> String {
    value.into_inner().to_rfc3339()
}
