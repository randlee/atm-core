use super::stmt_cache::WriterStatementCache;
use crate::search_schema::{
    sync_message_projection, sync_message_projection_by_key, sync_template_projection,
};
use crate::shared_db::{SharedDbTarget, serialize_json, sqlite_error, sqlite_thread_mode};
use atm_storage::contract::{
    AcknowledgementCommit, AcknowledgementReplyBuilder, AcknowledgementSource, Message, MessageKey,
    MessageQuery,
};
use atm_storage::error::AtmError;
use atm_storage::schema::MessageEnvelope;
use atm_storage::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::{
    DecomposedMessageAdmission, DecomposedMessageAdmissionOutcome, TemplateMessageAdmission,
    TemplateRegistration, TemplateRegistrationOutcome,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub(crate) const MAX_ENVELOPE_JSON_BYTES: usize = 1_048_576;

type DecomposedWorkflowColumns<'a> = (
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<String>,
    Option<String>,
);

#[derive(Clone)]
pub(crate) enum WriteOp {
    ListMessages(MessageQuery),
    UpsertMessage(Box<Message>),
    /// A related group of immutable records that must either all become
    /// visible or none do.  AI.31 uses this for the ACK reply and the
    /// acknowledged source record.
    UpsertMessages(Vec<Message>),
    Acknowledge {
        source: AcknowledgementSource,
        builder: Arc<dyn AcknowledgementReplyBuilder>,
    },
    RegisterTemplate(Box<TemplateRegistration>),
    AdmitDecomposedMessage(Box<DecomposedMessageAdmission>),
    AdmitTemplateMessage(Box<TemplateMessageAdmission>),
}

impl std::fmt::Debug for WriteOp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ListMessages(_) => formatter.write_str("ListMessages(..)"),
            Self::UpsertMessage(_) => formatter.write_str("UpsertMessage(..)"),
            Self::UpsertMessages(_) => formatter.write_str("UpsertMessages(..)"),
            Self::Acknowledge { source, .. } => formatter
                .debug_struct("Acknowledge")
                .field("source", source)
                .finish_non_exhaustive(),
            Self::RegisterTemplate(request) => formatter
                .debug_tuple("RegisterTemplate")
                .field(&request.sha)
                .finish(),
            Self::AdmitDecomposedMessage(admission) => formatter
                .debug_tuple("AdmitDecomposedMessage")
                .field(&admission.message.key)
                .finish(),
            Self::AdmitTemplateMessage(admission) => formatter
                .debug_tuple("AdmitTemplateMessage")
                .field(&admission.record.message_key)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WriteOpResult {
    Messages(Vec<Message>),
    UpsertMessage {
        inserted: bool,
        /// Populated only when an immutable-key duplicate won the admission
        /// race. Loading it on the writer connection keeps async callers from
        /// opening a synchronous reader connection after awaiting the queue.
        existing: Option<Box<Message>>,
    },
    UpsertMessages,
    Acknowledged(Box<AcknowledgementCommit>),
    TemplateRegistration(TemplateRegistrationOutcome),
    DecomposedMessageAdmission(DecomposedMessageAdmissionOutcome),
    TemplateMessageAdmission {
        inserted: bool,
        existing: Option<Box<Message>>,
    },
}

pub(crate) fn execute(
    op: &WriteOp,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    match op {
        WriteOp::ListMessages(query) => execute_list_messages(query, connection, target),
        WriteOp::UpsertMessage(request) => {
            execute_upsert_message(request, connection, cache, target)
        }
        WriteOp::UpsertMessages(records) => {
            for record in records {
                let _ = execute_upsert_message(record, connection, cache, target)?;
            }
            Ok(WriteOpResult::UpsertMessages)
        }
        WriteOp::Acknowledge { source, builder } => {
            execute_acknowledgement(source, builder, connection, cache, target)
        }
        WriteOp::RegisterTemplate(request) => {
            execute_template_registration(request, connection, target)
        }
        WriteOp::AdmitDecomposedMessage(admission) => {
            execute_decomposed_message_admission(admission, connection, target)
        }
        WriteOp::AdmitTemplateMessage(admission) => {
            admission.validate()?;
            match execute_upsert_message(&admission.record, connection, cache, target)? {
                WriteOpResult::UpsertMessage {
                    inserted: false,
                    existing,
                } => Ok(WriteOpResult::TemplateMessageAdmission {
                    inserted: false,
                    existing,
                }),
                WriteOpResult::UpsertMessage { inserted: true, .. } => {
                    let _ = execute_decomposed_message_admission(
                        &admission.decomposition,
                        connection,
                        target,
                    )?;
                    Ok(WriteOpResult::TemplateMessageAdmission {
                        inserted: true,
                        existing: None,
                    })
                }
                other => Err(AtmError::daemon_unavailable(format!(
                    "sqlite writer returned the wrong result while admitting a template message: {other:?}"
                ))),
            }
        }
    }
}

fn execute_template_registration(
    request: &TemplateRegistration,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    request.validate()?;
    let inserted = insert_template_if_absent(request, connection, target)?;
    Ok(WriteOpResult::TemplateRegistration(if inserted {
        TemplateRegistrationOutcome::Inserted
    } else {
        TemplateRegistrationOutcome::AlreadyRegistered
    }))
}

fn execute_decomposed_message_admission(
    admission: &DecomposedMessageAdmission,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    admission.validate()?;
    let template_inserted = insert_template_if_absent(&admission.template, connection, target)?;
    let existing_template_sha = connection
        .query_row(
            "SELECT template_sha FROM mail_messages WHERE message_key = ?1 LIMIT 1",
            params![admission.message.key.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| {
            sqlite_error(target, "failed to inspect decomposed message target", error)
        })?
        .ok_or_else(|| {
            AtmError::mailbox_write("decomposed admission requires an existing canonical message")
        })?;

    if let Some(existing) = existing_template_sha {
        if existing == admission.message.template_sha.as_str() {
            return Ok(WriteOpResult::DecomposedMessageAdmission(
                DecomposedMessageAdmissionOutcome::MessageAlreadyPresent,
            ));
        }
        return Err(AtmError::validation(
            "an existing decomposed message cannot be rebound to a different template SHA",
        ));
    }

    let vars_json = serialize_json(admission.message.vars.as_map(), "decomposed message vars")?;
    let tags_json = serialize_json(&admission.message.tags, "decomposed message tags")?;
    persist_decomposed_message_columns(admission, connection, target, vars_json, tags_json)?;
    Ok(WriteOpResult::DecomposedMessageAdmission(
        DecomposedMessageAdmissionOutcome::Inserted {
            template: if template_inserted {
                TemplateRegistrationOutcome::Inserted
            } else {
                TemplateRegistrationOutcome::AlreadyRegistered
            },
        },
    ))
}

fn persist_decomposed_message_columns(
    admission: &DecomposedMessageAdmission,
    connection: &Connection,
    target: &SharedDbTarget,
    vars_json: String,
    tags_json: String,
) -> Result<(), AtmError> {
    let (
        workflow_scope_kind,
        workflow_scope_id,
        workflow_state,
        workflow_stage,
        workflow_transition,
        workflow_iteration,
        applied_template_tags_json,
        effective_tags_json,
    ) = decomposed_workflow_columns(admission)?;
    let changed = connection
        .execute(
            "UPDATE mail_messages
             SET template_sha = ?1, vars_json = ?2, category = ?3,
                 tags_json = ?4, content_format = ?5, message_text = NULL,
                 workflow_scope_kind = ?6, workflow_scope_id = ?7,
                 workflow_state = ?8, workflow_stage = ?9,
                 workflow_transition = ?10, workflow_iteration = ?11,
                 applied_template_tags_json = ?12, effective_tags_json = ?13
             WHERE message_key = ?14 AND template_sha IS NULL",
            params![
                admission.message.template_sha.as_str(),
                vars_json,
                admission.message.category.as_deref(),
                tags_json,
                admission.message.content_format.as_deref(),
                workflow_scope_kind,
                workflow_scope_id,
                workflow_state,
                workflow_stage,
                workflow_transition,
                workflow_iteration,
                applied_template_tags_json,
                effective_tags_json,
                admission.message.key.as_str(),
            ],
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to persist decomposed message columns",
                error,
            )
        })?;
    if changed != 1 {
        return Err(AtmError::mailbox_write(
            "decomposed admission did not update exactly one canonical message",
        ));
    }
    sync_message_projection_by_key(connection, target, admission.message.key.as_str())?;
    Ok(())
}

fn decomposed_workflow_columns(
    admission: &DecomposedMessageAdmission,
) -> Result<DecomposedWorkflowColumns<'_>, AtmError> {
    match admission.message.workflow.as_ref() {
        Some(workflow) => Ok((
            Some(workflow.snapshot.scope_kind.as_str()),
            Some(workflow.snapshot.scope_id.as_str()),
            Some(workflow.snapshot.state.as_str()),
            Some(workflow.snapshot.stage.as_str()),
            Some(workflow.snapshot.transition.as_str()),
            workflow
                .snapshot
                .iteration
                .as_ref()
                .map(|iteration| iteration.as_str()),
            Some(serialize_json(
                &workflow.tag_provenance.applied_template_tags,
                "applied template tags",
            )?),
            Some(serialize_json(
                &workflow.tag_provenance.effective_tags,
                "effective tags",
            )?),
        )),
        None => Ok((None, None, None, None, None, None, None, None)),
    }
}

fn insert_template_if_absent(
    request: &TemplateRegistration,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<bool, AtmError> {
    let schema_json = serialize_json(&request.frontmatter, "template frontmatter")?;
    let inserted = connection
        .execute(
            "INSERT INTO message_templates(
                template_sha, template_type, template_name, content_bytes,
                content_text, schema_json, first_seen_at, first_seen_by
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(template_sha) DO NOTHING",
            params![
                request.sha.as_str(),
                request.template_type.as_deref(),
                request.template_name.as_deref(),
                request.content_bytes.as_slice(),
                request.content_text.as_str(),
                schema_json,
                request.first_seen.at.to_string(),
                request.first_seen.by.as_str(),
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to register immutable template", error))?
        == 1;
    sync_template_projection(
        connection,
        target,
        request.sha.as_str(),
        request.content_text.as_str(),
    )?;
    Ok(inserted)
}

fn execute_list_messages(
    query: &MessageQuery,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    let limit = query
        .limit
        .map(|value| i64::try_from(value).unwrap_or(i64::MAX))
        .unwrap_or(-1);
    let mut statement = connection
        .prepare(
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
               AND (?3 IS NULL OR mail_messages.from_agent = ?3)
               AND (?4 IS NULL OR json_extract(mail_messages.envelope_json, '$.taskId') = ?4)
               AND mail_message_states.deleted_at IS NULL
               AND (
                    mail_message_states.expires_at IS NULL
                    OR mail_message_states.expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
               )
             ORDER BY mail_messages.message_at DESC, mail_messages.message_key DESC
             LIMIT ?5",
        )
        .map_err(|error| {
            sqlite_error(target, "failed to prepare writer mailbox projection", error)
        })?;
    let rows = statement
        .query_map(
            params![
                query.team.as_str(),
                query.agent.as_str(),
                query.sender.as_ref().map(|value| value.as_str()),
                query.task_id.as_ref().map(|value| value.as_str()),
                limit,
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
        .map_err(|error| {
            sqlite_error(target, "failed to execute writer mailbox projection", error)
        })?;

    rows.map(|row| decode_mailbox_projection_row(row, query, target))
        .collect::<Result<Vec<_>, _>>()
        .map(WriteOpResult::Messages)
}

type MailboxProjectionRow = (
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn decode_mailbox_projection_row(
    row: rusqlite::Result<MailboxProjectionRow>,
    query: &MessageQuery,
    target: &SharedDbTarget,
) -> Result<Message, AtmError> {
    let (message_key, envelope_json, read, pending_ack_at, acknowledged_at, expires_at) = row
        .map_err(|error| {
            sqlite_error(target, "failed to decode writer mailbox projection", error)
        })?;
    let mut envelope = serde_json::from_str::<MessageEnvelope>(&envelope_json).map_err(|_| {
        AtmError::mailbox_read("failed to decode writer mailbox projection envelope")
    })?;
    envelope.read = read != 0;
    envelope.pending_ack_at = parse_timestamp(pending_ack_at, "pending_ack_at")?;
    envelope.acknowledged_at = parse_timestamp(acknowledged_at, "acknowledged_at")?;
    envelope.expires_at = parse_timestamp(expires_at, "expires_at")?;
    Ok(Message {
        team: query.team.clone(),
        agent: query.agent.clone(),
        message_key: MessageKey::new(message_key)?,
        envelope,
    })
}

fn execute_acknowledgement(
    source: &AcknowledgementSource,
    builder: &Arc<dyn AcknowledgementReplyBuilder>,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    let source = load_pending_ack_source(source, connection, target)?;
    let reply = builder.build_reply(&source)?;
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

fn load_pending_ack_source(
    source: &AcknowledgementSource,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<Message, AtmError> {
    let row = load_acknowledgement_source_row(source, connection, target)?;
    reject_acknowledgement_source_with_successor(source, connection, target)?;
    decode_pending_acknowledgement_source(source, row)
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

fn decode_pending_acknowledgement_source(
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
    if envelope.pending_ack_at.is_none() {
        let state = if envelope.acknowledged_at.is_some() {
            "already acknowledged"
        } else {
            "not pending acknowledgement"
        };
        return Err(AtmError::validation(format!(
            "message {} is {state}",
            source.message_id
        )));
    }
    Ok(Message {
        team: source.team.clone(),
        agent: source.agent.clone(),
        message_key: MessageKey::new(message_key)?,
        envelope,
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
    let values = prepare_message_insert_values(record)?;
    let inserted = cache
        .insert_message_row(
            connection,
            params![
                record.team.as_str(),
                record.agent.as_str(),
                record.message_key.as_ref(),
                values.envelope_json,
                values.from_agent,
                values.source_chat_id,
                values.destination_chat_id,
                values.message_text,
                values.classification.category,
                values.classification.content_format,
                values.classification.tags_json,
                values.summary,
                values.message_at,
                values.message_id,
                values.parent_message_id,
                values.thread_mode,
                values.recorded_at.clone(),
            ],
        )
        .map_err(|error| map_message_insert_error(target, error))?
        == 1;
    let timestamps = initial_state_timestamps(
        values.pending_ack_at,
        values.acknowledged_at,
        values.expires_at,
        values.recorded_at,
    );
    insert_initial_message_state(connection, cache, target, record, timestamps)?;
    if inserted {
        sync_message_projection(
            connection,
            target,
            record.team.as_str(),
            record.agent.as_str(),
            record.message_key.as_str(),
        )?;
    }

    let existing = if inserted {
        None
    } else {
        Some(Box::new(load_existing_message(record, connection, target)?))
    };
    Ok(WriteOpResult::UpsertMessage { inserted, existing })
}

struct MessageInsertValues {
    envelope_json: String,
    from_agent: String,
    source_chat_id: Option<String>,
    destination_chat_id: Option<String>,
    message_text: String,
    classification: PlainTemplateClassification,
    summary: Option<String>,
    message_at: String,
    message_id: Option<String>,
    parent_message_id: Option<String>,
    thread_mode: Option<String>,
    recorded_at: String,
    pending_ack_at: Option<String>,
    acknowledged_at: Option<String>,
    expires_at: Option<String>,
}

fn prepare_message_insert_values(record: &Message) -> Result<MessageInsertValues, AtmError> {
    let envelope_json = serialize_json(
        &StorageEnvelope::new(&record.envelope),
        "mail-store envelope",
    )?;
    validate_message_record(record, envelope_json.len())?;
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
    Ok(MessageInsertValues {
        envelope_json,
        from_agent: record.envelope.from.to_string(),
        source_chat_id: record
            .envelope
            .source_chat_id
            .as_ref()
            .map(ToString::to_string),
        destination_chat_id: record
            .envelope
            .destination_chat_id
            .as_ref()
            .map(ToString::to_string),
        message_text: record.envelope.text.clone(),
        classification: message_classification(&record.envelope.extra)?,
        summary: record.envelope.summary.clone(),
        message_at: record.envelope.timestamp.into_inner().to_rfc3339(),
        message_id: record.envelope.message_id.as_ref().map(ToString::to_string),
        parent_message_id: record
            .envelope
            .parent_message_id
            .as_ref()
            .map(ToString::to_string),
        thread_mode: sqlite_thread_mode(record.envelope.thread_mode).map(str::to_owned),
        // Ingest timing is owned by the durable store, not by callers (ADR-005).
        recorded_at: IsoTimestamp::now().into_inner().to_rfc3339(),
        pending_ack_at,
        acknowledged_at,
        expires_at,
    })
}

/// The canonical envelope remains the single ordinary-message DTO.
/// Classification is carried in explicitly named metadata fields and projected
/// here into normal searchable columns; no core caller reaches SQLite or
/// constructs a second storage path.
struct PlainTemplateClassification {
    category: Option<String>,
    content_format: Option<String>,
    tags_json: String,
}

fn message_classification(
    extra: &serde_json::Map<String, Value>,
) -> Result<PlainTemplateClassification, AtmError> {
    let optional_string = |key: &str| -> Result<Option<String>, AtmError> {
        match extra.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => Ok(Some(value.clone())),
            Some(_) => Err(AtmError::mailbox_write(format!(
                "message classification metadata '{key}' must be a string or null"
            ))),
        }
    };
    let tags = match extra.get("tags") {
        None => Vec::new(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| {
                    AtmError::mailbox_write(
                        "message classification metadata 'tags' must be an array of strings",
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err(AtmError::mailbox_write(
                "message classification metadata 'tags' must be an array of strings",
            ));
        }
    };
    Ok(PlainTemplateClassification {
        category: optional_string("category")?,
        content_format: optional_string("content_format")?,
        tags_json: serialize_json(&tags, "message classification tags")?,
    })
}

fn load_existing_message(
    requested: &Message,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<Message, AtmError> {
    let row = connection
        .query_row(
            "SELECT mail_messages.team, mail_messages.agent, mail_messages.envelope_json,
                    mail_message_states.read, mail_message_states.pending_ack_at,
                    mail_message_states.acknowledged_at, mail_message_states.expires_at
             FROM mail_messages
             LEFT JOIN mail_message_states
               ON mail_message_states.team = mail_messages.team
              AND mail_message_states.agent = mail_messages.agent
              AND mail_message_states.message_key = mail_messages.message_key
             WHERE mail_messages.team = ?1
               AND mail_messages.agent = ?2
               AND mail_messages.message_key = ?3",
            params![
                requested.team.as_str(),
                requested.agent.as_str(),
                requested.message_key.as_ref(),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to load duplicate message", error))?
        .ok_or_else(|| {
            AtmError::daemon_unavailable(
                "sqlite writer reported an existing message key but the retained record could not be loaded",
            )
        })?;
    let (team, agent, envelope_json, read, pending_ack_at, acknowledged_at, expires_at) = row;
    let team = team.parse::<TeamName>().map_err(|error: AtmError| {
        AtmError::validation(format!(
            "failed to parse sqlite team for duplicate message {}: {error}",
            requested.message_key
        ))
    })?;
    let agent = agent.parse::<AgentName>().map_err(|error: AtmError| {
        AtmError::validation(format!(
            "failed to parse sqlite agent for duplicate message {}: {error}",
            requested.message_key
        ))
    })?;
    let mut envelope = serde_json::from_str::<MessageEnvelope>(&envelope_json)
        .map_err(|_| AtmError::mailbox_read("failed to decode duplicate message envelope"))?;
    envelope.read = read != 0;
    envelope.pending_ack_at = parse_optional_timestamp(pending_ack_at, "pending_ack_at")?;
    envelope.acknowledged_at = parse_optional_timestamp(acknowledged_at, "acknowledged_at")?;
    envelope.expires_at = parse_optional_timestamp(expires_at, "expires_at")?;
    Ok(Message {
        team,
        agent,
        message_key: requested.message_key.clone(),
        envelope,
    })
}

fn parse_optional_timestamp(
    value: Option<String>,
    field: &str,
) -> Result<Option<IsoTimestamp>, AtmError> {
    value
        .map(|value| value.parse::<IsoTimestamp>())
        .transpose()
        .map_err(|_| AtmError::mailbox_read(format!("duplicate message {field} is invalid")))
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
