use std::collections::HashMap;
use std::path::Path;

#[cfg(test)]
use crate::address::MessageParticipantFilter;
use crate::boundary;
use crate::error::AtmError;
use crate::schema::InboxMessage;
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::threading::ThreadIndex;
use crate::types::{IsoTimestamp, TeamName};

use super::selection::{
    MailboxSelectionCandidate, MailboxSelectionRequest, classify_mailbox_candidates,
    select_classified_mailbox_candidates,
};
use super::{BucketCounts, ClassifiedMessage, ReadQuery, filters, state};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataBackedReadSelection {
    pub summary_text: Option<String>,
    pub message_text: Option<String>,
}

pub(crate) fn selection_state_for_mailbox_metadata_rows(
    rows: &[boundary::MailStoreMailboxMetadataRow],
    query: &ReadQuery,
    seen_watermark: Option<IsoTimestamp>,
) -> (BucketCounts, Vec<ClassifiedMessage>) {
    let candidates = rows.iter().map(metadata_candidate).collect::<Vec<_>>();
    let request =
        MailboxSelectionRequest::from_read_query(query, seen_watermark).without_contains();
    select_classified_mailbox_candidates(candidates, &request)
}

fn metadata_candidate(row: &boundary::MailStoreMailboxMetadataRow) -> MailboxSelectionCandidate {
    MailboxSelectionCandidate {
        message_key: row.message_key.to_string(),
        envelope: InboxMessage {
            from: row.from_agent.clone(),
            source_chat_id: row.source_chat_id.clone(),
            // Metadata rows deliberately carry no durable body. The retained
            // sync path reloads it only for the existing contains behavior.
            text: String::new(),
            timestamp: row.message_at,
            read: row.read,
            source_team: None,
            destination_chat_id: row.destination_chat_id.clone(),
            summary: row.summary.clone(),
            message_id: row.message_id,
            requires_ack: row.requires_ack,
            pending_ack_at: row.pending_ack.then_some(row.message_at),
            acknowledged_at: row.acknowledged_at,
            acknowledges_message_id: None,
            parent_message_id: row.parent_message_id,
            thread_mode: row.thread_mode,
            expires_at: row.expires_at,
            task_id: row.task_id.clone(),
            extra: serde_json::Map::new(),
        },
    }
}

pub(crate) fn classify_mailbox_metadata_rows(
    rows: &[boundary::MailStoreMailboxMetadataRow],
) -> Vec<ClassifiedMessage> {
    classify_mailbox_candidates(rows.iter().map(metadata_candidate).collect())
}

#[cfg(test)]
pub(crate) fn apply_metadata_only_filters(
    messages: Vec<ClassifiedMessage>,
    sender_filter: Option<&crate::types::AgentName>,
    participant_filter: Option<&MessageParticipantFilter>,
    timestamp_filter: Option<IsoTimestamp>,
    task_filter: Option<&crate::types::TaskId>,
) -> Vec<ClassifiedMessage> {
    filters::apply_task_filter(
        filters::apply_timestamp_filter(
            filters::apply_participant_filter(
                filters::apply_sender_filter(messages, sender_filter),
                participant_filter,
            ),
            timestamp_filter,
        ),
        task_filter,
    )
}

pub(crate) fn filter_metadata_backed_contains_candidates<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &Path,
    team: &TeamName,
    agent: &crate::types::AgentName,
    metadata_rows: &[boundary::MailStoreMailboxMetadataRow],
    messages: Vec<ClassifiedMessage>,
    contains_needle: Option<&str>,
) -> Result<Vec<ClassifiedMessage>, AtmError> {
    let Some(needle) = contains_needle else {
        return Ok(messages);
    };
    let row_by_id = metadata_rows
        .iter()
        .filter_map(|row| row.message_id.map(|message_id| (message_id, row)))
        .collect::<HashMap<_, _>>();

    messages
        .into_iter()
        .map(|message| -> Result<Option<ClassifiedMessage>, AtmError> {
            let row = metadata_rows.get(message.source_index.get()).ok_or_else(|| {
                AtmError::validation(format!(
                    "sqlite mailbox metadata source index {} is out of range during contains filtering",
                    message.source_index.get()
                ))

            })?;
            let mut selection = MetadataBackedReadSelection {
                summary_text: row.summary.clone(),
                message_text: None,
            };
            if filters::text_contains_needle(selection.summary_text.as_deref(), needle) {
                return Ok(Some(message));
            }
            selection.message_text = Some(load_durable_message_text(
                runtime,
                home_dir,
                team,
                agent,
                &row_by_id,
                &message,
                row,
            )?);
            Ok(filters::text_contains_needle(
                selection.message_text.as_deref(),
                needle,
            )
            .then_some(message))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|selected| selected.into_iter().flatten().collect())
}

pub(crate) fn load_durable_metadata_message<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &Path,
    team: &TeamName,
    agent: &crate::types::AgentName,
    metadata_rows: &[boundary::MailStoreMailboxMetadataRow],
    selected_message: &ClassifiedMessage,
    exact_message_id: Option<crate::schema::AtmMessageId>,
) -> Result<ClassifiedMessage, AtmError> {
    let row_by_id = metadata_rows
        .iter()
        .filter_map(|row| row.message_id.map(|message_id| (message_id, row)))
        .collect::<HashMap<_, _>>();
    let metadata_row = metadata_rows
        .get(selected_message.source_index.get())
        .ok_or_else(|| {
            AtmError::validation(format!(
                "sqlite mailbox metadata source index {} is out of range during durable reload",
                selected_message.source_index.get()
            ))
        })?;
    let Some(record) =
        runtime.load_message_record(home_dir, team, agent, &metadata_row.message_key)?
    else {
        // A concurrent clear can delete the row after selection but before the
        // durable body reload. Preserve command completion with the selected
        // snapshot; actual reload errors still propagate above.
        return Ok(selected_message.clone());
    };
    let envelope = if exact_message_id == record.envelope.message_id {
        record.envelope
    } else if record.envelope.thread_mode == Some(crate::schema::ThreadMode::AddDetails) {
        load_logical_current_record(
            runtime,
            home_dir,
            team,
            agent,
            &row_by_id,
            selected_message,
            record.envelope,
        )?
    } else {
        record.envelope
    };
    let envelope =
        runtime.render_message_body(&metadata_row.message_key, envelope.message_id, &envelope)?;
    Ok(ClassifiedMessage {
        source_index: selected_message.source_index,
        source_path: selected_message.source_path.clone(),
        bucket: state::display_bucket_for_class(state::classify_message(&envelope)),
        class: state::classify_message(&envelope),
        envelope,
    })
}

fn load_durable_message_text<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &Path,
    team: &TeamName,
    agent: &crate::types::AgentName,
    row_by_id: &HashMap<crate::schema::AtmMessageId, &boundary::MailStoreMailboxMetadataRow>,
    selected_message: &ClassifiedMessage,
    metadata_row: &boundary::MailStoreMailboxMetadataRow,
) -> Result<String, AtmError> {
    let Some(record) =
        runtime.load_message_record(home_dir, team, agent, &metadata_row.message_key)?
    else {
        return Err(AtmError::validation(format!(
            "sqlite mailbox metadata row {} could not be reloaded for contains filtering",
            metadata_row.message_key
        )));
    };
    let envelope = if record.envelope.thread_mode == Some(crate::schema::ThreadMode::AddDetails) {
        load_logical_current_record(
            runtime,
            home_dir,
            team,
            agent,
            row_by_id,
            selected_message,
            record.envelope,
        )?
    } else {
        record.envelope
    };
    Ok(runtime
        .render_message_body(&metadata_row.message_key, envelope.message_id, &envelope)?
        .text)
}

fn load_logical_current_record<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &Path,
    team: &TeamName,
    agent: &crate::types::AgentName,
    row_by_id: &HashMap<crate::schema::AtmMessageId, &boundary::MailStoreMailboxMetadataRow>,
    selected_message: &ClassifiedMessage,
    terminal_envelope: InboxMessage,
) -> Result<InboxMessage, AtmError> {
    let Some(mut current_id) = terminal_envelope.message_id else {
        return Ok(terminal_envelope);
    };

    let mut chain_ids = Vec::new();
    while let Some(row) = row_by_id.get(&current_id) {
        chain_ids.push(current_id);
        let Some(parent_id) = row.parent_message_id else {
            break;
        };
        current_id = parent_id;
    }
    chain_ids.reverse();

    let mut chain = Vec::new();
    for message_id in chain_ids {
        let Some(row) = row_by_id.get(&message_id) else {
            return Err(AtmError::validation(format!(
                "sqlite mailbox thread row for {} disappeared during logical-current reconstruction",
                message_id
            )));
        };
        let Some(record) = runtime.load_message_record(home_dir, team, agent, &row.message_key)?
        else {
            return Err(AtmError::validation(format!(
                "sqlite mailbox thread row {} could not be reloaded for logical-current reconstruction",
                row.message_key
            )));
        };
        chain.push(record.envelope);
    }
    let thread_index = ThreadIndex::new(&chain);
    thread_index
        .logical_current_envelope(
            terminal_envelope
                .message_id
                .or(selected_message.envelope.message_id)
                .unwrap_or(current_id),
        )
        .ok_or_else(|| {
            AtmError::validation("failed to reconstruct logical current thread envelope")
        })
}

pub(crate) fn sort_and_limit_selected(selected: &mut Vec<ClassifiedMessage>, limit: Option<usize>) {
    selected.sort_by(|left, right| {
        right
            .envelope
            .timestamp
            .cmp(&left.envelope.timestamp)
            .then_with(|| right.envelope.message_id.cmp(&left.envelope.message_id))
            .then_with(|| right.source_index.cmp(&left.source_index))
    });

    if let Some(limit) = limit {
        selected.truncate(limit);
    }
}
