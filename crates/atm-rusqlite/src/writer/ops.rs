use super::stmt_cache::WriterStatementCache;
use crate::shared_db::{SharedDbTarget, serialize_json, sqlite_thread_mode};
use atm_core::boundary;
use atm_core::error::AtmError;
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) const MAX_ENVELOPE_JSON_BYTES: usize = 1_048_576;

#[derive(Debug, Clone)]
pub(crate) enum WriteOp {
    UpsertMessage(boundary::MailStoreUpsertMessageRequest),
    UpsertVisibilityState(boundary::MailStoreUpsertVisibilityStateRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WriteOpResult {
    UpsertMessage { inserted: bool },
    Unit,
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
        WriteOp::UpsertVisibilityState(request) => {
            execute_upsert_visibility_state(request, connection, cache, target)
        }
    }
}

pub(crate) fn validate_upsert_message_request(
    request: &boundary::MailStoreUpsertMessageRequest,
) -> Result<(), AtmError> {
    let envelope_json = serialize_json(&request.record.envelope, "mail-store envelope")?;
    if envelope_json.len() > MAX_ENVELOPE_JSON_BYTES {
        return Err(AtmError::validation(format!(
            "mail-store envelope JSON exceeded the writer lane limit of {MAX_ENVELOPE_JSON_BYTES} bytes"
        ))
        .with_recovery(
            "Reduce the message envelope payload before retrying or raise the documented writer-lane size ceiling intentionally.",
        ));
    }
    Ok(())
}

fn execute_upsert_message(
    request: &boundary::MailStoreUpsertMessageRequest,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    let record = &request.record;
    let envelope_json = serialize_json(&record.envelope, "mail-store envelope")?;
    validate_message_record(record, envelope_json.len(), connection, cache, target)?;
    let parent_message_id = record
        .envelope
        .parent_message_id
        .as_ref()
        .map(ToString::to_string);
    let thread_mode = sqlite_thread_mode(record.envelope.thread_mode);
    let stale_at = record
        .envelope
        .stale_at
        .map(|value| value.into_inner().to_rfc3339());
    let pending_ack_at = record
        .envelope
        .pending_ack_at
        .map(|value| value.into_inner().to_rfc3339());
    let acknowledged_at = record
        .envelope
        .acknowledged_at
        .map(|value| value.into_inner().to_rfc3339());
    let from_agent = record.envelope.from.to_string();
    let message_text = record.envelope.text.clone();
    let summary = record.envelope.summary.clone();
    let message_at = record.envelope.timestamp.into_inner().to_rfc3339();
    let legacy_message_id = record.envelope.message_id.as_ref().map(ToString::to_string);
    let recorded_at = record
        .recorded_at
        .map(|value| value.into_inner().to_rfc3339());

    let inserted = cache
        .insert_message_row(
            connection,
            params![
                record.team.as_str(),
                record.agent.as_str(),
                record.message_key.as_ref(),
                envelope_json,
                from_agent,
                message_text,
                summary,
                message_at,
                legacy_message_id,
                parent_message_id,
                thread_mode,
                stale_at,
                record.imported_from,
                recorded_at.clone(),
            ],
        )
        .map_err(|error| {
            crate::shared_db::sqlite_error(target, "failed to upsert mail-store message", error)
        })?
        == 1;
    cache
        .upsert_ack_state(
            connection,
            params![
                record.team.as_str(),
                record.agent.as_str(),
                record.message_key.as_ref(),
                pending_ack_at,
                acknowledged_at,
                recorded_at,
            ],
        )
        .map_err(|error| {
            crate::shared_db::sqlite_error(target, "failed to upsert ack-state row", error)
        })?;

    Ok(WriteOpResult::UpsertMessage { inserted })
}

fn validate_message_record(
    record: &boundary::MailStoreMessageRecord,
    envelope_json_len: usize,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    if envelope_json_len > MAX_ENVELOPE_JSON_BYTES {
        return Err(AtmError::validation(format!(
            "mail-store envelope JSON exceeded the writer lane limit of {MAX_ENVELOPE_JSON_BYTES} bytes"
        ))
        .with_recovery(
            "Reduce the message envelope payload before retrying or raise the documented writer-lane size ceiling intentionally.",
        ));
    }

    let message_key = record.message_key.as_ref();
    if !message_key.starts_with("atm:") && !message_key.starts_with("ext:") {
        return Err(AtmError::validation(format!(
            "mail-store message_key must start with `atm:` or `ext:`; got `{message_key}`"
        ))
        .with_recovery(
            "Rewrite the message record with an ATM-owned or external message_key prefix before retrying the sqlite write.",
        ));
    }

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
            ))
            .with_recovery(
                "Reuse the existing successor message_key or choose a different parent_message_id before retrying the sqlite write.",
            ));
        }
    }

    if let Some(legacy_message_id) = record.envelope.message_id {
        let owner = cache
            .load_legacy_identity_owner(
                connection,
                params![
                    record.team.as_str(),
                    record.agent.as_str(),
                    legacy_message_id.to_string()
                ],
            )
            .optional()
            .map_err(|error| {
                crate::shared_db::sqlite_error(
                    target,
                    "failed to validate legacy message identity uniqueness",
                    error,
                )
            })?;
        if let Some(owner) = owner
            && owner != message_key
        {
            return Err(AtmError::validation(format!(
                "legacy message_id `{legacy_message_id}` is already owned by `{owner}` and cannot be reassigned to `{message_key}`"
            ))
            .with_recovery(
                "Reuse the existing message_key for that legacy message identity or generate a new legacy identity before retrying the sqlite write.",
            ));
        }
    }

    Ok(())
}

fn execute_upsert_visibility_state(
    request: &boundary::MailStoreUpsertVisibilityStateRequest,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    let state_json = serialize_json(&request.state, "mail-store visibility state")?;
    cache
        .upsert_visibility_state(
            connection,
            params![
                request.team.as_str(),
                request.agent.as_str(),
                request.state.message_key.as_ref(),
                state_json,
            ],
        )
        .map_err(|error| {
            crate::shared_db::sqlite_error(
                target,
                "failed to upsert mail-store visibility state",
                error,
            )
        })?;
    cache
        .upsert_ack_state(
            connection,
            params![
                request.team.as_str(),
                request.agent.as_str(),
                request.state.message_key.as_ref(),
                request
                    .state
                    .pending_ack_at
                    .map(|value| value.into_inner().to_rfc3339()),
                request
                    .state
                    .acknowledged_at
                    .map(|value| value.into_inner().to_rfc3339()),
                request
                    .state
                    .updated_at
                    .map(|value| value.into_inner().to_rfc3339()),
            ],
        )
        .map_err(|error| {
            crate::shared_db::sqlite_error(
                target,
                "failed to upsert ack-state visibility projection",
                error,
            )
        })?;
    Ok(WriteOpResult::Unit)
}
