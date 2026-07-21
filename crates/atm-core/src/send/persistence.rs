use std::path::Path;

use serde_json::Map;

use crate::boundary;
use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::error::AtmError;
use crate::schema::{AckIntentFields, AtmMessageId, InboxMessage};
use crate::service_runtime::RetainedServiceRuntime;
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::types::{AgentName, IsoTimestamp, TeamName};

use super::{DeliveryPersistenceResult, WarningEntry, prepare_threaded_message};

pub(crate) fn persist_message(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    home_dir: &Path,
    recipient: &DeliveryRecipientSnapshot,
    inbox_path: &Path,
    envelope: &InboxMessage,
    require_existing_inbox: bool,
) -> Result<DeliveryPersistenceResult, AtmError> {
    if require_existing_inbox && !inbox_path.exists() {
        return Ok(DeliveryPersistenceResult::persisted(envelope.clone()));
    }

    let mut prepared = envelope.clone();
    let inbox_messages =
        load_store_backed_mailbox_projection(runtime, home_dir, &recipient.team, &recipient.agent)?;
    prepare_threaded_message(&mut prepared, &inbox_messages)?;

    match mirror_message_to_store(runtime, &recipient.team, &recipient.agent, &prepared) {
        Ok(()) => Ok(DeliveryPersistenceResult::persisted(prepared)),
        Err(error) if error.code() == crate::error_codes::AtmErrorCode::MailboxWriteFailed => {
            recover_after_sqlite_failure(runtime, recipient, inbox_path, &prepared, &error)
        }
        Err(error) => Err(error),
    }
}

fn recover_after_sqlite_failure(
    _runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    recipient: &DeliveryRecipientSnapshot,
    _inbox_path: &Path,
    original_message: &InboxMessage,
    sqlite_error: &AtmError,
) -> Result<DeliveryPersistenceResult, AtmError> {
    let companion = build_sqlite_failure_companion_message(
        &recipient.team,
        &recipient.agent,
        original_message,
        sqlite_error,
    );
    let warning = WarningEntry::with_code(
        sqlite_error.code(),
        format!(
            "error: SQLite persistence failed for delivery to {}@{}: {}.",
            recipient.agent, recipient.team, sqlite_error
        ),
        Some(
            "ATM emitted a degraded fallback delivery plus an atm-system companion error. Investigate and repair the SQLite runtime immediately.",
        ),
    );
    Ok(DeliveryPersistenceResult::sqlite_failed_recovered(
        original_message.clone(),
        companion,
        warning,
    ))
}

fn build_sqlite_failure_companion_message(
    team: &TeamName,
    agent: &AgentName,
    original_message: &InboxMessage,
    sqlite_error: &AtmError,
) -> InboxMessage {
    let original_message_id = original_message
        .message_id
        .map(|message_id| message_id.to_string())
        .unwrap_or_else(|| "unknown-message-id".to_string());
    let ack_intent = AckIntentFields::not_required();
    InboxMessage {
        from: AgentName::from_validated("atm-system"),
        source_chat_id: None,
        text: format!(
            "ATM error: SQLite persistence failed while delivering message {} to {}@{}: {}. The original message was emitted through the degraded outward path only and the retained SQLite state must be repaired immediately.",
            original_message_id, agent, team, sqlite_error
        ),
        timestamp: IsoTimestamp::now(),
        read: false,
        source_team: Some(team.clone()),
        destination_chat_id: None,
        summary: Some(format!(
            "ATM error: SQLite persistence failed for {}@{}",
            agent, team
        )),
        message_id: Some(AtmMessageId::new()),
        requires_ack: ack_intent.requires_ack,
        pending_ack_at: ack_intent.pending_ack_at,
        acknowledged_at: ack_intent.acknowledged_at,
        acknowledges_message_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: original_message.task_id.clone(),
        extra: Map::new(),
    }
}

fn load_store_backed_mailbox_projection(
    runtime: &(impl RetainedMailboxRuntime + ?Sized),
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<InboxMessage>, AtmError> {
    let mut metadata_rows = runtime.query_mailbox_metadata_rows(home_dir, team, agent, None)?;
    metadata_rows.sort_by(|left, right| {
        left.message_at
            .cmp(&right.message_at)
            .then_with(|| left.message_key.as_ref().cmp(right.message_key.as_ref()))
    });

    let mut messages = Vec::with_capacity(metadata_rows.len());
    for row in metadata_rows {
        // Concurrent clear/ack paths can legally delete a row after metadata
        // enumeration but before the compatibility export reloads it. Skip the
        // vanished row and export the current mailbox contents instead of
        // failing the whole send after the write already committed.
        let Some(record) = runtime.load_message_record(home_dir, team, agent, &row.message_key)?
        else {
            continue;
        };
        messages.push(record.envelope);
    }
    Ok(messages)
}

fn mirror_message_to_store(
    runtime: &(impl RetainedMailboxRuntime + ?Sized),
    team: &TeamName,
    agent: &AgentName,
    envelope: &InboxMessage,
) -> Result<(), AtmError> {
    let Some(message_id) = envelope.message_id else {
        return Ok(());
    };
    let message_key = boundary::MessageKey::from(message_id);
    runtime.persist_message_record(boundary::Message {
        team: team.clone(),
        agent: agent.clone(),
        message_key: message_key.clone(),
        envelope: envelope.clone(),
    })?;
    runtime.persist_message_state(boundary::MailMessageState {
        team: team.clone(),
        agent: agent.clone(),
        actor: agent.clone(),
        message_key,
        read: envelope.read,
        pending_ack_at: envelope.pending_ack_at,
        acknowledged_at: envelope.acknowledged_at,
        expires_at: envelope.expires_at,
        deleted_at: None,
        updated_at: Some(IsoTimestamp::now()),
    })
}
