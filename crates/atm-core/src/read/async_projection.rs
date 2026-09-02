//! Pure command preparation and response assembly for the Tokio mailbox path.
//!
//! This module owns target resolution, owner-only authorization, selection
//! shape, and protocol outcome assembly.  HTTP handlers pass typed commands
//! to `atm-runtime`; they neither reconstruct mailbox policy nor call the
//! retained synchronous service runtime.

use atm_storage::MailboxScope;
use std::time::Duration;

use crate::error::AtmError;
use crate::mailbox::source::resolve_target;
use crate::types::{CommandAction, IsoTimestamp, ReadSelection};

use super::selection::{MailboxSelectionRequest, MailboxSelectionResult, SelectedMailboxMessage};
use super::{ClassifiedMessage, PeekQuery, ReadOutcome, ReadQuery, SourceIndex};

/// Fully prepared, storage-neutral mailbox command for the async reader lane.
#[derive(Debug, Clone)]
pub struct AsyncReadCommand {
    scope: MailboxScope,
    selection: MailboxSelectionRequest,
    explicit_target: bool,
    selection_mode: ReadSelection,
    action: CommandAction,
    loads_seen_watermark: bool,
    updates_seen_state: bool,
    wait_timeout: Option<Duration>,
}

impl AsyncReadCommand {
    #[must_use]
    pub fn scope(&self) -> &MailboxScope {
        &self.scope
    }

    #[must_use]
    pub fn explicit_target(&self) -> bool {
        self.explicit_target
    }

    #[must_use]
    pub fn requires_seen_watermark(&self) -> bool {
        self.loads_seen_watermark
    }

    #[must_use]
    pub fn with_seen_watermark(mut self, seen_watermark: Option<IsoTimestamp>) -> Self {
        self.selection.seen_watermark = seen_watermark;
        self
    }

    #[must_use]
    pub fn selection(&self) -> &MailboxSelectionRequest {
        &self.selection
    }

    #[must_use]
    pub fn updates_seen_state(&self) -> bool {
        self.updates_seen_state
    }

    #[must_use]
    pub fn wait_timeout(&self) -> Option<Duration> {
        self.wait_timeout
    }
}

/// Prepare a peek command without touching configuration, filesystem state,
/// or storage. Alias configuration is intentionally unavailable to the
/// system daemon, matching the existing daemon-owned path.
pub fn prepare_async_peek(query: &PeekQuery) -> Result<AsyncReadCommand, AtmError> {
    let synthesized = ReadQuery {
        mailbox: query.mailbox.clone(),
        caller_identity: query.caller_identity.clone(),
        caller_chat_id: query.caller_chat_id.clone(),
        caller_team: query.caller_team.clone(),
        seen_state_update: false,
        activity_observation: None,
    };
    prepare(&synthesized, CommandAction::Peek, false, false)
}

/// Prepare a mutating-read command. The follow-up mutation is represented by
/// the runtime's handoff protocol, not executed during preparation.
pub fn prepare_async_read(query: &ReadQuery) -> Result<AsyncReadCommand, AtmError> {
    prepare(query, CommandAction::Read, true, query.seen_state_update)
}

/// Assemble the stable protocol outcome from a reader-lane selection. The
/// handoff outcome tells clients whether the display state was accepted for
/// later writer-lane application; it never waits for that application.
#[must_use]
pub fn complete_async_read(
    command: &AsyncReadCommand,
    selection: MailboxSelectionResult,
    match_count: usize,
    mutation_handoff_accepted: bool,
) -> ReadOutcome {
    let selected_message_id = selection
        .selected
        .first()
        .and_then(|message| message.envelope.message_id);
    let message = selection.selected.first().map(classified_message);
    ReadOutcome {
        action: command.action,
        team: command.scope.team.clone(),
        agent: command.scope.agent.clone(),
        selection_mode: command.selection_mode,
        mutation_applied: command.action == CommandAction::Read && mutation_handoff_accepted,
        count: usize::from(message.is_some()),
        message,
        selected_message_id,
        match_count,
        additional_match_count: match_count.saturating_sub(1),
        bucket_counts: selection.bucket_counts,
    }
}

fn prepare(
    query: &ReadQuery,
    action: CommandAction,
    owner_only: bool,
    updates_seen_state: bool,
) -> Result<AsyncReadCommand, AtmError> {
    let actor = query.caller_identity.clone();
    let target = resolve_target(
        query.mailbox.target_address.as_ref(),
        &actor,
        &query.caller_team,
        None,
    )?;
    if owner_only {
        super::ensure_owner_only_read_target(&actor, &query.caller_team, &target)?;
    }
    let seen_requested =
        query.mailbox.seen_state_filter && query.mailbox.selection_mode != ReadSelection::All;
    Ok(AsyncReadCommand {
        scope: MailboxScope::new(target.team, target.agent),
        selection: MailboxSelectionRequest {
            selection_mode: query.mailbox.selection_mode,
            seen_watermark: None,
            message_id_filter: query.mailbox.message_id_filter,
            sender_filter: query.mailbox.sender_filter.clone(),
            participant_filter: query.mailbox.participant_filter.clone(),
            timestamp_filter: query.mailbox.timestamp_filter,
            task_filter: query.mailbox.task_filter.clone(),
            contains_filter: query.mailbox.contains_filter.clone(),
        },
        explicit_target: target.explicit,
        selection_mode: query.mailbox.selection_mode,
        action,
        loads_seen_watermark: seen_requested,
        updates_seen_state,
        wait_timeout: query.timeout_secs().map(Duration::from_secs),
    })
}

fn classified_message(message: &SelectedMailboxMessage) -> ClassifiedMessage {
    ClassifiedMessage {
        source_index: SourceIndex::from(0usize),
        source_path: message.message_key.clone().into(),
        bucket: message.bucket,
        class: message.class,
        envelope: message.envelope.clone(),
    }
}
