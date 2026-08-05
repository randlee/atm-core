use super::stmt_cache::WriterStatementCache;
use crate::shared_db::{SharedDbTarget, serialize_json, sqlite_thread_mode};
use atm_storage::contract::{
    AcknowledgementCommit, AcknowledgementReplyBuilder, AcknowledgementSource, Message, MessageKey,
};
use atm_storage::error::AtmError;
use atm_storage::schema::MessageEnvelope;
use atm_storage::types::IsoTimestamp;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::sync::Arc;

pub(crate) const MAX_ENVELOPE_JSON_BYTES: usize = 1_048_576;

#[derive(Clone)]
pub(crate) enum WriteOp {
    UpsertMessage(Box<Message>),
    /// A related group of immutable records that must either all become
    /// visible or none do.  AI.31 uses this for the ACK reply and the
    /// acknowledged source record.
    UpsertMessages(Vec<Message>),
    /// New immutable records that must all be absent at commit time. This is
    /// intentionally distinct from an upsert batch so a competing admission
    /// cannot be silently treated as a successful array replay.
    AdmitMessages(Vec<Message>),
    Acknowledge {
        source: AcknowledgementSource,
        builder: Arc<dyn AcknowledgementReplyBuilder>,
    },
}

impl std::fmt::Debug for WriteOp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UpsertMessage(_) => formatter.write_str("UpsertMessage(..)"),
            Self::UpsertMessages(_) => formatter.write_str("UpsertMessages(..)"),
            Self::AdmitMessages(_) => formatter.write_str("AdmitMessages(..)"),
            Self::Acknowledge { source, .. } => formatter
                .debug_struct("Acknowledge")
                .field("source", source)
                .finish_non_exhaustive(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WriteOpResult {
    UpsertMessage { inserted: bool },
    UpsertMessages,
    AdmitMessages,
    Acknowledged(Box<AcknowledgementCommit>),
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
        WriteOp::AdmitMessages(records) => {
            for record in records {
                let WriteOpResult::UpsertMessage { inserted } =
                    execute_upsert_message(record, connection, cache, target)?
                else {
                    return Err(AtmError::daemon_unavailable(
                        "sqlite writer returned the wrong result while admitting an immutable batch",
                    ));
                };
                if !inserted {
                    let message_id = record
                        .envelope
                        .message_id
                        .map_or_else(|| record.message_key.to_string(), |id| id.to_string());
                    return Err(AtmError::message_id_conflict(format!(
                        "message {message_id} was admitted concurrently; retry the complete peer message array"
                    )));
                }
            }
            Ok(WriteOpResult::AdmitMessages)
        }
        WriteOp::Acknowledge { source, builder } => {
            execute_acknowledgement(source, builder, connection, cache, target)
        }
    }
}

fn execute_acknowledgement(
    acknowledgement_source: &AcknowledgementSource,
    builder: &Arc<dyn AcknowledgementReplyBuilder>,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    let source = load_acknowledgement_source(acknowledgement_source, connection, target)?;
    let reply = builder.build_reply(&source)?;
    if source.envelope.pending_ack_at.is_none() {
        if source.envelope.acknowledged_at.is_some()
            && matching_acknowledgement_reply_exists(
                &reply,
                source.envelope.message_id,
                connection,
                target,
            )?
        {
            // A peer can retransmit an immutable acknowledgement after its
            // first delivery was committed but before it observed the HTTP
            // response. Treat only that exact already-persisted reply as an
            // idempotent success. This also keeps same-host HTTPS loopback on
            // the identical inbound path as a remote peer.
            return Ok(WriteOpResult::Acknowledged(Box::new(
                AcknowledgementCommit { reply, source },
            )));
        }
        return Err(acknowledgement_source_state_error(&source));
    }
    reject_acknowledgement_source_with_successor(acknowledgement_source, connection, target)?;
    let mut acknowledged_source = source.clone();
    acknowledged_source.envelope.read = true;
    acknowledged_source.envelope.pending_ack_at = None;
    acknowledged_source.envelope.acknowledged_at = Some(IsoTimestamp::now());
    let _ = execute_upsert_message(&reply, connection, cache, target)?;
    let _ = execute_upsert_message(&acknowledged_source, connection, cache, target)?;
    Ok(WriteOpResult::Acknowledged(Box::new(
        AcknowledgementCommit {
            reply,
            source: acknowledged_source,
        },
    )))
}

fn load_acknowledgement_source(
    source: &AcknowledgementSource,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<Message, AtmError> {
    let row = load_acknowledgement_source_row(source, connection, target)?;
    decode_acknowledgement_source(source, row)
}

type AcknowledgementSourceRow = (
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn load_acknowledgement_source_row(
    source: &AcknowledgementSource,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<AcknowledgementSourceRow, AtmError> {
    connection
        .query_row(
            "SELECT mail_messages.message_key, mail_messages.envelope_json,
                    mail_message_states.read, mail_message_states.pending_ack_at,
                    mail_message_states.acknowledged_at, mail_message_states.expires_at
             FROM mail_messages
             JOIN mail_message_states
               ON mail_message_states.team = mail_messages.team
              AND mail_message_states.agent = mail_messages.agent
              AND mail_message_states.message_key = mail_messages.message_key
             WHERE mail_messages.team = ?1
               AND mail_messages.agent = ?2
               AND mail_messages.message_id = ?3
               AND mail_message_states.deleted_at IS NULL",
            params![
                source.team.as_str(),
                source.agent.as_str(),
                source.message_id.to_string()
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(|error| {
            crate::shared_db::sqlite_error(target, "failed to load acknowledgement source", error)
        })?
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {} was not found in {}@{}",
                source.message_id, source.agent, source.team
            ))
        })
}

fn reject_acknowledgement_source_with_successor(
    source: &AcknowledgementSource,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let has_successor = connection
        .query_row(
            "SELECT 1 FROM mail_messages
             WHERE team = ?1 AND agent = ?2 AND parent_message_id = ?3
             LIMIT 1",
            params![
                source.team.as_str(),
                source.agent.as_str(),
                source.message_id.to_string()
            ],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| {
            crate::shared_db::sqlite_error(
                target,
                "failed to validate acknowledgement terminal source",
                error,
            )
        })?
        .is_some();
    if has_successor {
        return Err(AtmError::validation(format!(
            "message {} has been updated; acknowledge the current terminal message instead",
            source.message_id
        )));
    }
    Ok(())
}

fn decode_acknowledgement_source(
    source: &AcknowledgementSource,
    row: AcknowledgementSourceRow,
) -> Result<Message, AtmError> {
    let (message_key, envelope_json, read, pending_ack_at, acknowledged_at, expires_at) = row;
    let mut envelope = serde_json::from_str::<atm_storage::schema::MessageEnvelope>(&envelope_json)
        .map_err(|_| AtmError::mailbox_read("failed to decode acknowledgement source envelope"))?;
    envelope.read = read != 0;
    envelope.pending_ack_at = parse_timestamp(pending_ack_at, "pending_ack_at")?;
    envelope.acknowledged_at = parse_timestamp(acknowledged_at, "acknowledged_at")?;
    envelope.expires_at = parse_timestamp(expires_at, "expires_at")?;
    Ok(Message {
        team: source.team.clone(),
        agent: source.agent.clone(),
        message_key: MessageKey::new(message_key)?,
        envelope,
    })
}

fn acknowledgement_source_state_error(source: &Message) -> AtmError {
    let state = if source.envelope.acknowledged_at.is_some() {
        "already acknowledged"
    } else {
        "not pending acknowledgement"
    };
    AtmError::validation(format!(
        "message {} is {state}",
        source
            .envelope
            .message_id
            .as_ref()
            .map_or_else(String::new, ToString::to_string)
    ))
}

fn matching_acknowledgement_reply_exists(
    reply: &Message,
    source_message_id: Option<atm_storage::AtmMessageId>,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<bool, AtmError> {
    let Some(source_message_id) = source_message_id else {
        return Err(AtmError::mailbox_read(
            "acknowledged source is missing its immutable message ID",
        ));
    };
    connection
        .query_row(
            "SELECT 1 FROM mail_messages
             WHERE team = ?1
               AND agent = ?2
               AND message_key = ?3
               AND json_extract(envelope_json, '$.acknowledgesMessageId') = ?4
             LIMIT 1",
            params![
                reply.team.as_str(),
                reply.agent.as_str(),
                reply.message_key.as_ref(),
                source_message_id.to_string(),
            ],
            |_| Ok(()),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(|error| {
            crate::shared_db::sqlite_error(
                target,
                "failed to validate idempotent acknowledgement reply",
                error,
            )
        })
}

fn parse_timestamp(value: Option<String>, field: &str) -> Result<Option<IsoTimestamp>, AtmError> {
    value
        .map(|value| value.parse::<IsoTimestamp>())
        .transpose()
        .map_err(|_| AtmError::mailbox_read(format!("acknowledgement source {field} is invalid")))
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
    validate_message_record(record, envelope_json.len())?;
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
        .map_err(|error| map_message_insert_error(target, error))?
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

fn validate_message_record(record: &Message, envelope_json_len: usize) -> Result<(), AtmError> {
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

    Ok(())
}

fn map_message_insert_error(target: &SharedDbTarget, error: rusqlite::Error) -> AtmError {
    if matches!(
        error,
        rusqlite::Error::SqliteFailure(ref failure, _)
            if failure.code == rusqlite::ErrorCode::ConstraintViolation
    ) {
        return AtmError::validation(
            "mail-store message violates a durable message-id or successor uniqueness invariant",
        );
    }
    crate::shared_db::sqlite_error(target, "failed to upsert mail-store message", error)
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
