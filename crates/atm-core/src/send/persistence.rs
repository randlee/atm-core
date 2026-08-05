use std::path::Path;

use serde_json::Map;
use tracing::{error, info};

use crate::boundary;
use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::error::AtmError;
use crate::schema::{
    AckIntentFields, AtmMessageId, InboxMessage, clear_transport_delivery_metadata,
    peer_outbound_host,
};
use crate::service_runtime::RetainedServiceRuntime;
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::types::{AgentName, HostName, IsoTimestamp, TeamName};

use super::{
    DeliveryPersistenceResult, DuplicateWriteDisposition, WarningEntry, prepare_threaded_message,
};

#[cfg(test)]
pub(crate) fn persist_message(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    home_dir: &Path,
    recipient: &DeliveryRecipientSnapshot,
    inbox_path: &Path,
    envelope: &InboxMessage,
    require_existing_inbox: bool,
    same_store_peer_receipt: Option<(&HostName, &HostName)>,
) -> Result<DeliveryPersistenceResult, AtmError> {
    persist_message_with_ack_update(
        runtime,
        home_dir,
        recipient,
        inbox_path,
        envelope,
        require_existing_inbox,
        same_store_peer_receipt,
        None,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the acknowledgement source replacement is part of the one durable admission commit"
)]
pub(crate) fn persist_message_with_ack_update(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    home_dir: &Path,
    recipient: &DeliveryRecipientSnapshot,
    inbox_path: &Path,
    envelope: &InboxMessage,
    require_existing_inbox: bool,
    same_store_peer_receipt: Option<(&HostName, &HostName)>,
    acknowledgement_source_update: Option<boundary::Message>,
) -> Result<DeliveryPersistenceResult, AtmError> {
    if require_existing_inbox && !inbox_path.exists() {
        return Ok(DeliveryPersistenceResult::persisted(envelope.clone()));
    }

    let mut prepared = envelope.clone();
    // Ordinary immutable messages have no thread relation to validate. Do not
    // enumerate the recipient mailbox on their admission path; that makes a
    // simple send grow with mailbox size and opens avoidable SQLite readers.
    let inbox_messages = if prepared.parent_message_id.is_some() && prepared.thread_mode.is_some() {
        load_store_backed_mailbox_projection(runtime, home_dir, &recipient.team, &recipient.agent)?
    } else {
        Vec::new()
    };
    prepare_threaded_message(&mut prepared, &inbox_messages)?;

    match mirror_message_to_store(
        runtime,
        home_dir,
        &recipient.team,
        &recipient.agent,
        &prepared,
        same_store_peer_receipt,
        acknowledgement_source_update,
    ) {
        Ok(DuplicateWriteDisposition::NotDuplicate) => {
            Ok(DeliveryPersistenceResult::persisted(prepared))
        }
        Ok(DuplicateWriteDisposition::AlreadyDeliveredRemote) => {
            Ok(DeliveryPersistenceResult::already_persisted(prepared))
        }
        Ok(DuplicateWriteDisposition::SameStorePeerReceipt) => {
            Ok(DeliveryPersistenceResult::same_store_peer_receipt(prepared))
        }
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

pub(super) fn load_store_backed_mailbox_projection(
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
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    envelope: &InboxMessage,
    same_store_peer_receipt: Option<(&HostName, &HostName)>,
    acknowledgement_source_update: Option<boundary::Message>,
) -> Result<DuplicateWriteDisposition, AtmError> {
    let Some(message_id) = envelope.message_id else {
        return Ok(DuplicateWriteDisposition::NotDuplicate);
    };
    let message_key = boundary::MessageKey::from(message_id);
    let record = boundary::Message {
        team: team.clone(),
        agent: agent.clone(),
        message_key: message_key.clone(),
        envelope: envelope.clone(),
    };
    if let Some(source_update) = acknowledgement_source_update {
        if let Some(existing) = runtime.load_message_record(home_dir, team, agent, &message_key)? {
            let disposition = classify_existing_message(
                existing,
                envelope,
                message_id,
                team,
                agent,
                same_store_peer_receipt,
            );
            return persist_same_store_peer_receipt(runtime, record, disposition);
        }
        runtime.persist_message_records_atomically(vec![record, source_update])?;
    } else {
        if let Some(existing) = runtime.admit_message_record(home_dir, record.clone())? {
            let disposition = classify_existing_message(
                existing,
                envelope,
                message_id,
                team,
                agent,
                same_store_peer_receipt,
            );
            return persist_same_store_peer_receipt(runtime, record, disposition);
        }
    }
    Ok(DuplicateWriteDisposition::NotDuplicate)
}

fn persist_same_store_peer_receipt(
    runtime: &(impl RetainedMailboxRuntime + ?Sized),
    received_record: boundary::Message,
    disposition: Result<DuplicateWriteDisposition, AtmError>,
) -> Result<DuplicateWriteDisposition, AtmError> {
    let disposition = disposition?;
    if disposition == DuplicateWriteDisposition::SameStorePeerReceipt {
        // Replace only the prior local transport wrapper with the exact envelope
        // received through the ordinary peer ingress. The immutable payload is
        // untouched; this also makes every later same-ID receipt a normal
        // idempotent duplicate instead of another initial receipt.
        runtime.persist_message_record(received_record)?;
    }
    Ok(disposition)
}

pub(super) fn classify_existing_message(
    existing: boundary::Message,
    envelope: &InboxMessage,
    message_id: crate::schema::AtmMessageId,
    team: &TeamName,
    agent: &AgentName,
    same_store_peer_receipt: Option<(&HostName, &HostName)>,
) -> Result<DuplicateWriteDisposition, AtmError> {
    if immutable_envelopes_match(&existing.envelope, envelope) {
        if let Some((source_host, destination_host)) = same_store_peer_receipt
            && is_local_same_store_receipt(source_host, destination_host)
            && peer_outbound_host(&existing.envelope)?.as_ref() == Some(destination_host)
        {
            info!(
                event = "peer_duplicate_write_skipped",
                message_id = %message_id,
                source_host = %source_host,
                destination_host = %destination_host,
                same_store_peer_receipt = true,
                database_write = "skipped",
                delivery = "continued",
                "peer_duplicate_write_skipped"
            );
            return Ok(DuplicateWriteDisposition::SameStorePeerReceipt);
        }
        info!(
            message_id = %message_id,
            team = %team,
            agent = %agent,
            "duplicate message ULID write matched immutable data; retaining existing record"
        );
        return Ok(DuplicateWriteDisposition::AlreadyDeliveredRemote);
    }
    error!(
        code = %crate::error_codes::AtmErrorCode::MessageIdConflict,
        message_id = %message_id,
        team = %team,
        agent = %agent,
        "duplicate message ULID carried different immutable data; retaining original record"
    );
    Err(AtmError::message_id_conflict(format!(
        "message {message_id} already exists for {agent}@{team} with different immutable data"
    )))
}

/// A host-qualified message can return to the originating store only when the
/// receiver endpoint is loopback or the authenticated peer has the identical
/// advertised IP/host.  This is deliberately an admission-only classification:
/// it never rewrites the received envelope or changes its payload.
fn is_local_same_store_receipt(source_host: &HostName, destination_host: &HostName) -> bool {
    matches!(destination_host.as_str(), "localhost" | "127.0.0.1" | "::1")
        || source_host == destination_host
}

fn immutable_envelopes_match(left: &InboxMessage, right: &InboxMessage) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.read = false;
    left.pending_ack_at = None;
    left.acknowledged_at = None;
    right.read = false;
    right.pending_ack_at = None;
    right.acknowledged_at = None;
    clear_transport_delivery_metadata(&mut left);
    clear_transport_delivery_metadata(&mut right);
    left == right
}
