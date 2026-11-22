pub mod async_projection;
pub(crate) mod filters;
pub(crate) mod metadata_selection;
pub(crate) mod render;
mod request;
pub(crate) mod seen_state;
pub mod selection;
pub(crate) mod state;
pub(crate) mod wait;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::address::AgentAddress;
use crate::boundary;
use crate::error::AtmError;
use crate::mailbox::source::resolve_target;
use crate::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use crate::schema::{AtmMessageId, InboxMessage};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{
    AgentName, CommandAction, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection,
    SourceIndex, TeamName,
};
use metadata_selection::{
    filter_metadata_backed_contains_candidates, load_durable_metadata_message,
    selection_state_for_mailbox_metadata_rows, sort_and_limit_selected,
};

pub(crate) use request::normalize_contains_filter;
pub use request::{
    MAX_CONTAINS_FILTER_LEN, MAX_TIMEOUT_SECS, MailboxQueryFields, MailboxQueryFilters, PeekQuery,
    ReadQuery,
};

/// Canonicalizes every roster alias carried by a mailbox request before the
/// request reaches either reader implementation. This covers the caller,
/// owner target, and sender filter, leaving durable mailbox keys and audit
/// records in canonical ATM identity form.
pub fn canonicalize_roster_aliases<F>(query: &mut ReadQuery, mut resolve_member: F)
where
    F: FnMut(&TeamName, &AgentName, bool) -> Option<(TeamName, AgentName)>,
{
    let original_caller = query.caller_identity.clone();
    if let Some((team, member)) = resolve_member(&query.caller_team, &query.caller_identity, true) {
        if member != query.caller_identity {
            query.activity_observation = None;
        }
        query.caller_team = team;
        query.caller_identity = member;
    }
    if let Some(participant) = query.mailbox.participant_filter.as_mut()
        && participant.agent == original_caller
    {
        participant.agent = query.caller_identity.clone();
    }

    let target_team = query
        .mailbox
        .target_address
        .as_ref()
        .and_then(|address| address.team().cloned())
        .unwrap_or_else(|| query.caller_team.clone());
    let explicit_team = query
        .mailbox
        .target_address
        .as_ref()
        .is_some_and(|address| address.team().is_some());
    if let Some(target) = query.mailbox.target_address.as_ref()
        && let Some((team, member)) = resolve_member(&target_team, target.agent(), !explicit_team)
        && let Ok(canonical_address) = AgentAddress::new(
            member,
            target.chat_id().cloned(),
            Some(team),
            target.host().cloned(),
        )
    {
        query.mailbox.target_address = Some(canonical_address);
    }
    if let Some(sender) = query.mailbox.sender_filter.as_mut()
        && let Some((_, member)) = resolve_member(&target_team, sender, !explicit_team)
    {
        *sender = member;
    }
}

/// Canonicalizes a read-only peek request through the same ingress resolver.
pub fn canonicalize_peek_roster_aliases<F>(query: &mut PeekQuery, resolve_member: F)
where
    F: FnMut(&TeamName, &AgentName, bool) -> Option<(TeamName, AgentName)>,
{
    let mut read = ReadQuery {
        mailbox: query.mailbox.clone(),
        caller_identity: query.caller_identity.clone(),
        caller_chat_id: query.caller_chat_id.clone(),
        caller_team: query.caller_team.clone(),
        seen_state_update: false,
        activity_observation: None,
    };
    canonicalize_roster_aliases(&mut read, resolve_member);
    query.mailbox = read.mailbox;
    query.caller_identity = read.caller_identity;
}

/// Bucket counts for one classified mailbox surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketCounts {
    pub unread: usize,
    pub pending_ack: usize,
    pub history: usize,
}

/// One mailbox message classified for ATM display output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifiedMessage {
    #[serde(skip)]
    pub(crate) source_index: SourceIndex,
    #[serde(skip)]
    pub(crate) source_path: PathBuf,
    pub bucket: DisplayBucket,
    pub class: MessageClass,
    #[serde(flatten)]
    pub envelope: InboxMessage,
}

/// Result of one mailbox read/query command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadOutcome {
    pub action: CommandAction,
    pub team: TeamName,
    pub agent: AgentName,
    pub selection_mode: ReadSelection,
    pub mutation_applied: bool,
    pub count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<ClassifiedMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_message_id: Option<AtmMessageId>,
    pub match_count: usize,
    pub additional_match_count: usize,
    pub bucket_counts: BucketCounts,
}

/// Read one mailbox surface, optionally marking displayed messages as read.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::IdentityUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamNotFound`],
/// [`crate::error_codes::AtmErrorCode::AgentNotFound`],
/// [`crate::error_codes::AtmErrorCode::AddressParseFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxLockFailed`], or
/// [`crate::error_codes::AtmErrorCode::MailboxLockTimeout`] when actor or
/// target resolution fails, the team or agent cannot be validated, shared
/// mailbox locks cannot be acquired, or the selected mailbox state cannot be
/// reloaded or persisted safely.
pub fn read_mail(
    query: ReadQuery,
    observability: &dyn ObservabilityPort,
) -> Result<ReadOutcome, AtmError> {
    let runtime = default_runtime()?;
    read_mail_with_runtime_impl(query, observability, &runtime)
}

pub fn peek_mail(
    query: PeekQuery,
    observability: &dyn ObservabilityPort,
) -> Result<ReadOutcome, AtmError> {
    let runtime = default_runtime()?;
    peek_mail_with_runtime(query, observability, &runtime)
}

pub fn read_mail_with_runtime(
    query: ReadQuery,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<ReadOutcome, AtmError> {
    read_mail_with_runtime_impl(query, observability, runtime)
}

pub fn peek_mail_with_runtime(
    query: PeekQuery,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<ReadOutcome, AtmError> {
    peek_mail_with_runtime_impl(query, observability, runtime)
}

fn peek_mail_with_runtime_impl<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    mut query: PeekQuery,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<ReadOutcome, AtmError> {
    canonicalize_peek_roster_aliases(&mut query, |team, member, allow_database_wide_alias| {
        runtime.resolve_roster_member_at_ingress(team, member, allow_database_wide_alias)
    });
    let synthesized = ReadQuery {
        mailbox: query.mailbox,
        caller_identity: query.caller_identity,
        caller_chat_id: query.caller_chat_id,
        caller_team: query.caller_team,
        seen_state_update: false,
        activity_observation: None,
    };
    let ReadRuntimeContext {
        actor,
        actor_team,
        target,
        seen_watermark,
    } = resolve_read_context(&synthesized, runtime, false)?;
    let own_inbox = actor == target.agent && actor_team.as_deref() == Some(target.team.as_str());
    let selection = load_read_selection(runtime, &synthesized, &target, seen_watermark)?;
    let display = resolve_read_display(
        runtime,
        &synthesized,
        &target,
        seen_watermark,
        own_inbox,
        DisplayMutationMode::NonMutatingPeek,
        selection,
    )?;

    let outcome = ReadOutcome {
        action: CommandAction::Peek,
        team: target.team.clone(),
        agent: target.agent.clone(),
        selection_mode: synthesized.mailbox.selection_mode,
        mutation_applied: display.mutation_applied,
        count: usize::from(display.output_message.is_some()),
        message: display.output_message,
        selected_message_id: display.selected_message_id,
        match_count: display.match_count,
        additional_match_count: display.match_count.saturating_sub(1),
        bucket_counts: display.bucket_counts,
    };

    if let Err(error) = observability.emit(CommandEvent {
        command: "peek",
        action: action_name("peek"),
        outcome: outcome_label(if display.timed_out { "timeout" } else { "ok" }),
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: actor,
        message_id: None,
        requires_ack: false,
        dry_run: false,
        task_id: None,
        error_code: None,
        error_message: None,
    }) {
        tracing::warn!(%error, command = "peek", action = "peek", "failed to emit peek command event");
    }

    Ok(outcome)
}

fn read_mail_with_runtime_impl<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    mut query: ReadQuery,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<ReadOutcome, AtmError> {
    canonicalize_roster_aliases(&mut query, |team, member, allow_database_wide_alias| {
        runtime.resolve_roster_member_at_ingress(team, member, allow_database_wide_alias)
    });
    let ReadRuntimeContext {
        actor,
        actor_team,
        target,
        seen_watermark,
    } = resolve_read_context(&query, runtime, true)?;
    let own_inbox = actor == target.agent && actor_team.as_deref() == Some(target.team.as_str());
    let selection = load_read_selection(runtime, &query, &target, seen_watermark)?;
    let display = resolve_read_display(
        runtime,
        &query,
        &target,
        seen_watermark,
        own_inbox,
        DisplayMutationMode::MutatingRead,
        selection,
    )?;

    if query.seen_state_update
        && !display.selected.is_empty()
        && let Some(latest_timestamp) = display
            .selected
            .iter()
            .map(|message| message.envelope.timestamp)
            .max()
    {
        runtime.save_seen_watermark(
            &query.mailbox.home_dir,
            &target.team,
            &target.agent,
            latest_timestamp,
        )?;
    }

    let outcome = ReadOutcome {
        action: CommandAction::Read,
        team: target.team.clone(),
        agent: target.agent.clone(),
        selection_mode: query.mailbox.selection_mode,
        mutation_applied: display.mutation_applied,
        count: usize::from(display.output_message.is_some()),
        message: display.output_message,
        selected_message_id: display.selected_message_id,
        match_count: display.match_count,
        additional_match_count: display.match_count.saturating_sub(1),
        bucket_counts: display.bucket_counts,
    };

    if let Err(error) = observability.emit(CommandEvent {
        command: "read",
        action: action_name("read"),
        outcome: outcome_label(if display.timed_out { "timeout" } else { "ok" }),
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: actor,
        message_id: None,
        requires_ack: false,
        dry_run: false,
        task_id: None,
        error_code: None,
        error_message: None,
    }) {
        tracing::warn!(%error, command = "read", action = "read", "failed to emit read command event");
    }

    Ok(outcome)
}

struct ReadSelectionState {
    metadata_rows: Vec<boundary::MailboxMetadataRow>,
    bucket_counts: BucketCounts,
    selected: Vec<ClassifiedMessage>,
    timed_out: bool,
}

struct ReadDisplayState {
    mutation_applied: bool,
    output_message: Option<ClassifiedMessage>,
    bucket_counts: BucketCounts,
    selected_message_id: Option<AtmMessageId>,
    match_count: usize,
    timed_out: bool,
    selected: Vec<ClassifiedMessage>,
}

struct ReadSelectionSummary {
    selected_message_id: Option<AtmMessageId>,
    match_count: usize,
}

fn load_read_selection<R: RetainedMailboxRuntime>(
    runtime: &R,
    query: &ReadQuery,
    target: &crate::mailbox::source::ResolvedTarget,
    seen_watermark: Option<IsoTimestamp>,
) -> Result<ReadSelectionState, AtmError> {
    let contains_needle = query.mailbox.contains_filter.as_deref();
    let mut metadata_rows = load_checked_read_metadata(
        runtime,
        &query.mailbox.home_dir,
        &target.team,
        &target.agent,
    )?;
    let (mut bucket_counts, metadata_selected) =
        selection_state_for_mailbox_metadata_rows(&metadata_rows, query, seen_watermark);
    let mut selected = filter_metadata_backed_contains_candidates(
        runtime,
        &query.mailbox.home_dir,
        &target.team,
        &target.agent,
        &metadata_rows,
        metadata_selected,
        contains_needle,
    )?;
    let mut timed_out = false;

    if selected.is_empty()
        && let Some(timeout_secs) = query.mailbox.timeout_secs
    {
        let waited = wait_for_selection_candidates(
            runtime,
            query,
            target,
            seen_watermark,
            contains_needle,
            timeout_secs,
        )?;
        if let Some((updated_rows, updated_counts, updated_selected)) = waited {
            metadata_rows = updated_rows;
            bucket_counts = updated_counts;
            selected = updated_selected;
        } else {
            timed_out = true;
        }
    }

    Ok(ReadSelectionState {
        metadata_rows,
        bucket_counts,
        selected,
        timed_out,
    })
}

fn resolve_read_display<R: RetainedMailboxRuntime>(
    runtime: &R,
    query: &ReadQuery,
    target: &crate::mailbox::source::ResolvedTarget,
    seen_watermark: Option<IsoTimestamp>,
    _own_inbox: bool,
    mutation_mode: DisplayMutationMode,
    mut selection: ReadSelectionState,
) -> Result<ReadDisplayState, AtmError> {
    let summary = ReadSelectionSummary {
        match_count: selection.selected.len(),
        selected_message_id: selection
            .selected
            .first()
            .and_then(|message| message.envelope.message_id),
    };
    sort_and_limit_selected(&mut selection.selected, Some(1));
    let mutation_needed = displayed_messages_require_mutation(mutation_mode, &selection.selected);

    if selection.timed_out || selection.selected.is_empty() || !mutation_needed {
        return build_unmodified_read_display(runtime, query, target, selection, summary);
    }

    let mutation_applied = apply_display_mutations_to_store(
        runtime,
        &target.team,
        &target.agent,
        &selection.selected,
    )?;
    build_mutated_read_display(
        runtime,
        query,
        target,
        seen_watermark,
        selection,
        summary,
        mutation_applied,
    )
}

type WaitedSelection = Option<(
    Vec<boundary::MailboxMetadataRow>,
    BucketCounts,
    Vec<ClassifiedMessage>,
)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayMutationMode {
    MutatingRead,
    NonMutatingPeek,
}

fn wait_for_selection_candidates<R: RetainedMailboxRuntime>(
    runtime: &R,
    query: &ReadQuery,
    target: &crate::mailbox::source::ResolvedTarget,
    seen_watermark: Option<IsoTimestamp>,
    contains_needle: Option<&str>,
    timeout_secs: u64,
) -> Result<WaitedSelection, AtmError> {
    let wait_satisfied = wait::wait_for_eligible_message(
        timeout_secs,
        || {
            load_checked_read_metadata(
                runtime,
                &query.mailbox.home_dir,
                &target.team,
                &target.agent,
            )
        },
        |rows| {
            let selected = selection_state_for_mailbox_metadata_rows(rows, query, seen_watermark).1;
            filter_metadata_backed_contains_candidates(
                runtime,
                &query.mailbox.home_dir,
                &target.team,
                &target.agent,
                rows,
                selected,
                contains_needle,
            )
            .map(|filtered| !filtered.is_empty())
        },
    )?;
    if !wait_satisfied {
        return Ok(None);
    }
    let metadata_rows = load_checked_read_metadata(
        runtime,
        &query.mailbox.home_dir,
        &target.team,
        &target.agent,
    )?;
    let (updated_counts, metadata_selected) =
        selection_state_for_mailbox_metadata_rows(&metadata_rows, query, seen_watermark);
    let updated_selected = filter_metadata_backed_contains_candidates(
        runtime,
        &query.mailbox.home_dir,
        &target.team,
        &target.agent,
        &metadata_rows,
        metadata_selected,
        contains_needle,
    )?;
    Ok(Some((metadata_rows, updated_counts, updated_selected)))
}

fn build_unmodified_read_display<R: RetainedMailboxRuntime>(
    runtime: &R,
    query: &ReadQuery,
    target: &crate::mailbox::source::ResolvedTarget,
    selection: ReadSelectionState,
    summary: ReadSelectionSummary,
) -> Result<ReadDisplayState, AtmError> {
    let output_message = output_messages_from_metadata_selection(
        runtime,
        &query.mailbox.home_dir,
        &target.team,
        &target.agent,
        &selection.metadata_rows,
        &selection.selected,
        summary.selected_message_id,
    )?
    .into_iter()
    .next();
    Ok(ReadDisplayState {
        mutation_applied: false,
        output_message,
        bucket_counts: selection.bucket_counts,
        selected_message_id: summary.selected_message_id,
        match_count: summary.match_count,
        timed_out: selection.timed_out,
        selected: selection.selected,
    })
}

fn build_mutated_read_display<R: RetainedMailboxRuntime>(
    runtime: &R,
    query: &ReadQuery,
    target: &crate::mailbox::source::ResolvedTarget,
    seen_watermark: Option<IsoTimestamp>,
    selection: ReadSelectionState,
    summary: ReadSelectionSummary,
    mutation_applied: bool,
) -> Result<ReadDisplayState, AtmError> {
    let refreshed_rows = load_checked_read_metadata(
        runtime,
        &query.mailbox.home_dir,
        &target.team,
        &target.agent,
    )?;
    let (updated_counts, _updated_selected) =
        selection_state_for_mailbox_metadata_rows(&refreshed_rows, query, seen_watermark);
    // Reload against the snapshot the selection indices were computed from.
    // The refreshed rows only feed the bucket counts: a concurrent `clear`
    // can shrink them, which would make `source_index` point past the end
    // (or at a different row). The durable record itself is always loaded
    // fresh by key, and a vanished record falls back to the selected snapshot.
    let output_message = selection
        .selected
        .first()
        .cloned()
        .map(|selected_message| {
            output_messages_from_metadata_selection(
                runtime,
                &query.mailbox.home_dir,
                &target.team,
                &target.agent,
                &selection.metadata_rows,
                &[selected_message],
                summary.selected_message_id,
            )
            .map(|mut messages| messages.pop())
        })
        .transpose()?
        .flatten();
    Ok(ReadDisplayState {
        mutation_applied,
        output_message,
        bucket_counts: updated_counts,
        selected_message_id: summary.selected_message_id,
        match_count: summary.match_count,
        timed_out: selection.timed_out,
        selected: selection.selected,
    })
}

struct ReadRuntimeContext {
    actor: AgentName,
    actor_team: Option<TeamName>,
    target: crate::mailbox::source::ResolvedTarget,
    seen_watermark: Option<IsoTimestamp>,
}

fn resolve_read_context<R: RetainedServiceRuntime>(
    query: &ReadQuery,
    runtime: &R,
    owner_only: bool,
) -> Result<ReadRuntimeContext, AtmError> {
    let config = runtime.load_config(&query.mailbox.current_dir)?;
    let actor = query.caller_identity.clone();
    let actor_team = Some(query.caller_team.clone());
    let target = resolve_target(
        query.mailbox.target_address.as_ref(),
        &actor,
        &query.caller_team,
        config.as_ref(),
    )?;

    if owner_only {
        ensure_owner_only_read_target(&actor, &query.caller_team, &target)?;
    }

    validate_target_member_in_roster(runtime, &target)?;

    let seen_watermark =
        if query.mailbox.seen_state_filter && query.mailbox.selection_mode != ReadSelection::All {
            runtime.load_seen_watermark(&query.mailbox.home_dir, &target.team, &target.agent)?
        } else {
            None
        };

    Ok(ReadRuntimeContext {
        actor,
        actor_team,
        target,
        seen_watermark,
    })
}

fn validate_target_member_in_roster<R: RetainedServiceRuntime>(
    runtime: &R,
    target: &crate::mailbox::source::ResolvedTarget,
) -> Result<(), AtmError> {
    if !target.explicit {
        return Ok(());
    }

    if runtime
        .load_roster_member(&target.team, &target.agent)
        .is_none()
    {
        return Err(AtmError::agent_not_found(&target.agent, &target.team));
    }

    Ok(())
}

fn ensure_owner_only_read_target(
    actor: &AgentName,
    actor_team: &TeamName,
    target: &crate::mailbox::source::ResolvedTarget,
) -> Result<(), AtmError> {
    if target.explicit
        && (!actor.as_str().eq_ignore_ascii_case(target.agent.as_str())
            || !actor_team
                .as_str()
                .eq_ignore_ascii_case(target.team.as_str()))
    {
        return Err(AtmError::validation(format!(
            "owner-only `atm read` may not target '{}' in team '{}'; run the command as the mailbox owner or use `atm peek --as` for inspection",
            target.agent, target.team
        )));
    }

    Ok(())
}

fn message_key_for_classified(
    message: &ClassifiedMessage,
) -> Result<boundary::MessageKey, AtmError> {
    let message_id = message
        .envelope
        .message_id
        .ok_or_else(|| AtmError::validation("read message is missing a message_id"))?;
    Ok(boundary::MessageKey::from(message_id))
}

fn load_checked_read_metadata(
    runtime: &(impl RetainedMailboxRuntime + ?Sized),
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<boundary::MailboxMetadataRow>, AtmError> {
    let metadata_rows = runtime.query_mailbox_metadata_rows(home_dir, team, agent, None)?;
    Ok(metadata_rows)
}

fn output_messages_from_metadata_selection<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    metadata_rows: &[boundary::MailboxMetadataRow],
    selected: &[ClassifiedMessage],
    exact_message_id: Option<AtmMessageId>,
) -> Result<Vec<ClassifiedMessage>, AtmError> {
    selected
        .iter()
        .map(|selected_message| {
            load_durable_metadata_message(
                runtime,
                home_dir,
                team,
                agent,
                metadata_rows,
                selected_message,
                exact_message_id,
            )
        })
        .collect()
}

fn apply_display_mutations_to_store<R: RetainedMailboxRuntime>(
    runtime: &R,
    team: &TeamName,
    agent: &AgentName,
    displayed_messages: &[ClassifiedMessage],
) -> Result<bool, AtmError> {
    let mut changed = false;
    let now = IsoTimestamp::now();

    for message in displayed_messages {
        let updated = transition_displayed_message(message).into_envelope();
        if updated == message.envelope {
            continue;
        }
        runtime.persist_message_state(boundary::MailMessageState {
            team: team.clone(),
            agent: agent.clone(),
            actor: agent.clone(),
            message_key: message_key_for_classified(message)?,
            read: updated.read,
            pending_ack_at: updated.pending_ack_at,
            acknowledged_at: updated.acknowledged_at,
            expires_at: updated.expires_at,
            deleted_at: None,
            updated_at: Some(now),
        })?;
        changed = true;
    }

    Ok(changed)
}

fn displayed_messages_require_mutation(
    mutation_mode: DisplayMutationMode,
    displayed_messages: &[ClassifiedMessage],
) -> bool {
    if mutation_mode == DisplayMutationMode::NonMutatingPeek {
        return false;
    }
    displayed_messages
        .iter()
        .any(|message| !message.envelope.read)
}

fn transition_displayed_message(message: &ClassifiedMessage) -> state::TransitionedMessage {
    let read_state = state::derive_read_state(&message.envelope);
    let ack_state = state::derive_ack_state(&message.envelope);

    match (read_state, ack_state) {
        (crate::types::ReadState::Unread, crate::types::AckState::NoAckRequired) => {
            state::TransitionedMessage::ReadNoAck(
                state::StoredMessage::<crate::types::UnreadReadState, crate::types::NoAckState>::unread_no_ack(
                    message.envelope.clone(),
                )
                .display_without_ack(),
            )
        }
        (crate::types::ReadState::Unread, crate::types::AckState::PendingAck) => {
            state::TransitionedMessage::ReadPendingAck(
                state::StoredMessage::<
                    crate::types::UnreadReadState,
                    crate::types::PendingAckState,
                >::unread_pending_ack(message.envelope.clone())
                .mark_read_pending_ack(),
            )
        }
        (crate::types::ReadState::Unread, crate::types::AckState::Acknowledged)
        | (crate::types::ReadState::Read, crate::types::AckState::NoAckRequired)
        | (crate::types::ReadState::Read, crate::types::AckState::PendingAck)
        | (crate::types::ReadState::Read, crate::types::AckState::Acknowledged) => {
            let mut unchanged = message.envelope.clone();
            if !unchanged.read {
                unchanged.read = true;
            }
            state::TransitionedMessage::Unchanged(unchanged)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::Map;
    use serde_json::Value;
    use tempfile::tempdir;

    use super::{
        ClassifiedMessage, PeekQuery, ReadQuery, canonicalize_roster_aliases, metadata_selection,
        peek_mail_with_runtime_impl, read_mail_with_runtime_impl, state,
    };
    use crate::boundary::{self, MessageKey, RosterEntry, RosterHarness, RosterMemberKind};
    use crate::error::AtmError;
    use crate::mailbox::source::SourcedMessage;
    use crate::observability::NullObservability;
    use crate::read::selection;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::{AtmMessageId, InboxMessage, ThreadMode};
    use crate::service_runtime::RetainedServiceRuntime;
    use crate::service_runtime_store::RetainedMailboxRuntime;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::threading::ThreadIndex;
    use crate::types::{AgentName, ChatId, CommandAction, IsoTimestamp, ReadSelection, TeamName};

    #[test]
    fn read_ingress_canonicalizes_alias_caller_target_and_from_filter() {
        let root = tempdir().expect("root");
        let team = TeamName::from_validated(TEST_TEAM);
        let mut metadata_json = Map::new();
        metadata_json.insert("alias".to_owned(), Value::String("atm-lead".to_owned()));
        let roster = [RosterEntry {
            team_name: team.clone(),
            agent_name: AgentName::from_validated(ROLE_TEAM_LEAD),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::ClaudeCode,
            agent_type: crate::schema::AgentType::Lead,
            model: crate::types::ModelName::default(),
            recipient_pane_id: None,
            metadata_json,
        }];
        let mut query = ReadQuery::new(
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            AgentName::from_validated("atm-lead"),
            Some("atm-lead"),
            team,
            ReadSelection::All,
            false,
            false,
            None,
            Some("atm-lead"),
            None,
            None,
            None,
            None,
        )
        .expect("query");

        canonicalize_roster_aliases(&mut query, |_, candidate, _| {
            roster
                .iter()
                .find(|member| {
                    member.agent_name == *candidate
                        || member.metadata_json.get("alias").and_then(Value::as_str)
                            == Some(candidate.as_str())
                })
                .map(|member| (member.team_name.clone(), member.agent_name.clone()))
        });

        assert_eq!(query.caller_identity.as_str(), ROLE_TEAM_LEAD);
        assert_eq!(
            query
                .mailbox
                .target_address
                .as_ref()
                .map(|target| target.agent().as_str()),
            Some(ROLE_TEAM_LEAD)
        );
        assert_eq!(
            query.mailbox.sender_filter.as_ref().map(AgentName::as_str),
            Some(ROLE_TEAM_LEAD)
        );
    }

    #[test]
    fn unique_name_d14_qualified_alias_preserves_chat_id_at_ingress() {
        let root = tempdir().expect("root");
        let team = TeamName::from_validated(TEST_TEAM);
        let mut metadata_json = Map::new();
        metadata_json.insert("alias".to_owned(), Value::String("atm-lead".to_owned()));
        let roster = [RosterEntry {
            team_name: team.clone(),
            agent_name: AgentName::from_validated(ROLE_TEAM_LEAD),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::ClaudeCode,
            agent_type: crate::schema::AgentType::Lead,
            model: crate::types::ModelName::default(),
            recipient_pane_id: None,
            metadata_json,
        }];
        let chat_id: ChatId = "session-a".parse().expect("chat id");
        let mut query = ReadQuery::new(
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            AgentName::from_validated("atm-lead"),
            None,
            team,
            ReadSelection::All,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("query")
        .with_caller_chat_id(Some(chat_id.clone()));

        canonicalize_roster_aliases(&mut query, |_, candidate, _| {
            roster
                .iter()
                .find(|member| {
                    member.agent_name == *candidate
                        || member.metadata_json.get("alias").and_then(Value::as_str)
                            == Some(candidate.as_str())
                })
                .map(|member| (member.team_name.clone(), member.agent_name.clone()))
        });

        assert_eq!(query.caller_identity.as_str(), ROLE_TEAM_LEAD);
        assert_eq!(query.caller_chat_id(), Some(&chat_id));
        assert_eq!(
            query
                .mailbox
                .participant_filter
                .as_ref()
                .map(|filter| &filter.agent),
            Some(&AgentName::from_validated(ROLE_TEAM_LEAD))
        );
    }

    #[test]
    fn unique_name_d03_bare_alias_resolves_globally_but_explicit_team_stays_local() {
        let root = tempdir().expect("root");
        let local_team = TeamName::from_validated(TEST_TEAM);
        let remote_team = TeamName::from_validated("remote-team");
        let local = vec![RosterEntry {
            team_name: local_team.clone(),
            agent_name: AgentName::from_validated("bob"),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::ClaudeCode,
            agent_type: crate::schema::AgentType::Worker,
            model: crate::types::ModelName::default(),
            recipient_pane_id: None,
            metadata_json: Map::new(),
        }];
        let mut remote_metadata = Map::new();
        remote_metadata.insert("alias".to_owned(), Value::String("lead-alias".to_owned()));
        let remote = vec![RosterEntry {
            team_name: remote_team.clone(),
            agent_name: AgentName::from_validated(ROLE_TEAM_LEAD),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::ClaudeCode,
            agent_type: crate::schema::AgentType::Lead,
            model: crate::types::ModelName::default(),
            recipient_pane_id: None,
            metadata_json: remote_metadata,
        }];
        let all = [local.clone(), remote.clone()].concat();

        let mut bare = ReadQuery::new(
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            AgentName::from_validated("bob"),
            Some("lead-alias"),
            local_team.clone(),
            ReadSelection::All,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("bare query");
        canonicalize_roster_aliases(&mut bare, |team, candidate, allow_database_wide_alias| {
            let addressed = if *team == local_team { &local } else { &remote };
            let (resolved_team, canonical) = crate::caller_context::resolve_roster_alias_with_owner(
                candidate,
                team,
                addressed,
                &all,
                allow_database_wide_alias,
            );
            all.iter()
                .any(|member| member.team_name == resolved_team && member.agent_name == canonical)
                .then_some((resolved_team, canonical))
        });
        assert_eq!(
            bare.mailbox
                .target_address
                .as_ref()
                .and_then(|address| address.team()),
            Some(&remote_team)
        );
        assert_eq!(
            bare.mailbox
                .target_address
                .as_ref()
                .map(|address| address.agent().as_str()),
            Some(ROLE_TEAM_LEAD)
        );

        let mut explicit = bare.clone();
        explicit.mailbox.target_address = Some(
            format!("lead-alias@{local_team}")
                .parse()
                .expect("explicit address"),
        );
        canonicalize_roster_aliases(
            &mut explicit,
            |team, candidate, allow_database_wide_alias| {
                let addressed = if *team == local_team { &local } else { &remote };
                let (resolved_team, canonical) =
                    crate::caller_context::resolve_roster_alias_with_owner(
                        candidate,
                        team,
                        addressed,
                        &all,
                        allow_database_wide_alias,
                    );
                all.iter()
                    .any(|member| {
                        member.team_name == resolved_team && member.agent_name == canonical
                    })
                    .then_some((resolved_team, canonical))
            },
        );
        assert_eq!(
            explicit
                .mailbox
                .target_address
                .as_ref()
                .and_then(|address| address.team()),
            Some(&local_team)
        );
        assert_eq!(
            explicit
                .mailbox
                .target_address
                .as_ref()
                .map(|address| address.agent().as_str()),
            Some("lead-alias")
        );
    }

    fn selected_after_filters(
        messages: &[SourcedMessage],
        query: &ReadQuery,
        seen_watermark: Option<IsoTimestamp>,
    ) -> Vec<ClassifiedMessage> {
        let classified = classify_all(messages.to_vec());
        if let Some(message_id) = query.mailbox.message_id_filter {
            return classified
                .into_iter()
                .filter(|message| message.envelope.message_id == Some(message_id))
                .filter(|message| {
                    crate::read::filters::matches_participant_filter(
                        message,
                        query.mailbox.participant_filter.as_ref(),
                    )
                })
                .collect();
        }
        let filtered = crate::read::filters::apply_contains_filter(
            metadata_selection::apply_metadata_only_filters(
                selection::logical_current_messages(classified),
                query.mailbox.sender_filter.as_ref(),
                query.mailbox.participant_filter.as_ref(),
                query.mailbox.timestamp_filter,
                query.mailbox.task_filter.as_ref(),
            ),
            query.mailbox.contains_filter.as_deref(),
        );
        selection::select_classified_messages(
            &filtered,
            query.mailbox.selection_mode,
            seen_watermark,
        )
    }

    fn classify_all(messages: Vec<SourcedMessage>) -> Vec<ClassifiedMessage> {
        let projected = messages
            .iter()
            .map(|message| message.envelope.clone())
            .collect::<Vec<_>>();
        let thread_index = ThreadIndex::new(&projected);

        messages
            .into_iter()
            .zip(projected.iter().cloned())
            .map(|(message, projected)| {
                let effective = selection::effective_display_envelope(&projected, &thread_index);
                let class = state::classify_message(&effective);
                let bucket = state::display_bucket_for_class(class);

                ClassifiedMessage {
                    source_index: message.source_index,
                    source_path: message.source_path,
                    bucket,
                    class,
                    envelope: effective,
                }
            })
            .collect()
    }

    fn message_at(
        text: &str,
        message_id: AtmMessageId,
        parent_message_id: Option<AtmMessageId>,
        thread_mode: Option<ThreadMode>,
        read: bool,
        timestamp: IsoTimestamp,
    ) -> InboxMessage {
        InboxMessage {
            from: ROLE_TEAM_LEAD.parse::<AgentName>().expect("agent"),
            source_chat_id: None,
            text: text.to_string(),
            timestamp,
            read,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            destination_chat_id: None,
            summary: None,
            message_id: Some(message_id),
            requires_ack: false,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id,
            thread_mode,
            expires_at: None,
            task_id: None,
            placement: None,
            task_op: None,
            task_complete: None,
            extra: Map::new(),
        }
    }

    fn message(
        text: &str,
        message_id: AtmMessageId,
        parent_message_id: Option<AtmMessageId>,
        thread_mode: Option<ThreadMode>,
        read: bool,
    ) -> InboxMessage {
        message_at(
            text,
            message_id,
            parent_message_id,
            thread_mode,
            read,
            IsoTimestamp::now(),
        )
    }

    struct ReadRuntime {
        roster_present: bool,
        metadata_rows: Vec<boundary::MailboxMetadataRow>,
        metadata_row_batches: Option<Vec<Vec<boundary::MailboxMetadataRow>>>,
        message_records: HashMap<MessageKey, boundary::Message>,
        query_mailbox_metadata_rows_count: Arc<AtomicUsize>,
        load_message_record_count: Arc<AtomicUsize>,
        save_seen_watermark_count: Arc<AtomicUsize>,
        persist_message_state_count: Arc<AtomicUsize>,
        fail_load_message_record: bool,
    }

    impl crate::boundary::sealed::Sealed for ReadRuntime {}

    impl RetainedServiceRuntime for ReadRuntime {
        fn load_config(
            &self,
            _current_dir: &Path,
        ) -> Result<Option<crate::config::AtmConfig>, crate::error::AtmError> {
            Ok(None)
        }

        fn inbox_path(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<PathBuf, crate::error::AtmError> {
            unreachable!("read roster-truth tests do not resolve inbox paths")
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<IsoTimestamp>, crate::error::AtmError> {
            Ok(None)
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: IsoTimestamp,
        ) -> Result<(), crate::error::AtmError> {
            self.save_seen_watermark_count
                .fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn deliver_non_claude_payloads(
            &self,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            _messages: &[InboxMessage],
        ) -> Result<(), crate::error::AtmError> {
            unreachable!("read roster-truth tests do not route outbound payloads")
        }

        fn load_roster_member(
            &self,
            team: &TeamName,
            agent: &AgentName,
        ) -> Option<boundary::RosterEntry> {
            self.roster_present.then(|| boundary::RosterEntry {
                team_name: team.clone(),
                agent_name: agent.clone(),
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: crate::schema::AgentType::default(),
                model: crate::types::ModelName::default(),
                recipient_pane_id: None,
                metadata_json: Map::new(),
            })
        }

        fn load_team_roster(&self, _team: &TeamName) -> Vec<boundary::RosterEntry> {
            Vec::new()
        }
    }

    impl RetainedMailboxRuntime for ReadRuntime {
        fn acknowledge_message_atomically(
            &self,
            _source: &atm_storage::contract::AcknowledgementSource,
            _builder: std::sync::Arc<dyn atm_storage::contract::AcknowledgementReplyBuilder>,
        ) -> Result<atm_storage::contract::AcknowledgementCommit, crate::error::AtmError> {
            unreachable!("read roster-truth tests do not admit acknowledgements")
        }

        fn admit_message_record_with_outcome(
            &self,
            _home_dir: &Path,
            _record: boundary::Message,
            _provenance: atm_storage::MessageWriteOrigin,
        ) -> Result<
            atm_storage::CommittedTaskWrite<atm_storage::MessageAdmissionOutcome>,
            crate::error::AtmError,
        > {
            unreachable!("read roster-truth tests do not admit messages")
        }

        fn query_mailbox_metadata_rows(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _limit: Option<usize>,
        ) -> Result<Vec<boundary::MailboxMetadataRow>, crate::error::AtmError> {
            if let Some(batches) = &self.metadata_row_batches {
                let index = self
                    .query_mailbox_metadata_rows_count
                    .fetch_add(1, Ordering::SeqCst);
                return Ok(batches
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| batches.last().cloned().unwrap_or_default()));
            }
            Ok(self.metadata_rows.clone())
        }

        fn load_message_record(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            message_key: &boundary::MessageKey,
        ) -> Result<Option<boundary::Message>, crate::error::AtmError> {
            self.load_message_record_count
                .fetch_add(1, Ordering::SeqCst);
            if self.fail_load_message_record {
                return Err(AtmError::mailbox_read(
                    "simulated durable reload failure during contains filtering",
                ));
            }
            Ok(self.message_records.get(message_key).cloned())
        }

        fn persist_message_record(
            &self,
            _record: boundary::Message,
        ) -> Result<(), crate::error::AtmError> {
            unreachable!("read roster-truth tests do not persist message records")
        }

        fn persist_message_state(
            &self,
            _state: boundary::MailMessageState,
        ) -> Result<(), crate::error::AtmError> {
            self.persist_message_state_count
                .fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn explicit_read_query(home_dir: PathBuf, current_dir: PathBuf) -> ReadQuery {
        let target = format!("recipient@{TEST_TEAM}");
        ReadQuery::new(
            home_dir,
            current_dir,
            TEST_SENDER.parse().expect("caller"),
            Some(&target),
            TEST_TEAM.parse().expect("team"),
            ReadSelection::All,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query")
    }

    fn explicit_peek_query(home_dir: PathBuf, current_dir: PathBuf) -> PeekQuery {
        let target = format!("recipient@{TEST_TEAM}");
        PeekQuery::new(
            home_dir,
            current_dir,
            TEST_SENDER.parse().expect("caller"),
            Some(&target),
            TEST_TEAM.parse().expect("team"),
            ReadSelection::All,
            true,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("peek query")
    }

    fn base_read_query() -> ReadQuery {
        ReadQuery::new(
            PathBuf::new(),
            PathBuf::new(),
            TEST_SENDER.parse().expect("caller"),
            None,
            TEST_TEAM.parse().expect("team"),
            ReadSelection::All,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query")
    }

    #[test]
    fn chat_qualified_read_filters_source_and_metadata_surfaces_identically() {
        let chat_a = "chat-a".parse::<ChatId>().expect("chat id");
        let chat_b = "chat-b".parse::<ChatId>().expect("chat id");
        let first_id = AtmMessageId::new();
        let second_id = AtmMessageId::new();
        let mut first = message("for chat a", first_id, None, None, false);
        first.destination_chat_id = Some(chat_a.clone());
        let mut second = message("for chat b", second_id, None, None, false);
        second.destination_chat_id = Some(chat_b);
        let source_messages = vec![
            SourcedMessage {
                envelope: first.clone(),
                source_path: PathBuf::from("first.json"),
                source_index: 0.into(),
            },
            SourcedMessage {
                envelope: second.clone(),
                source_path: PathBuf::from("second.json"),
                source_index: 1.into(),
            },
        ];
        let query = ReadQuery::new(
            PathBuf::new(),
            PathBuf::new(),
            "recipient".parse().expect("caller"),
            None,
            TEST_TEAM.parse().expect("team"),
            ReadSelection::All,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query")
        .with_caller_chat_id(Some(chat_a.clone()));

        let source_selected = selected_after_filters(&source_messages, &query, None);
        assert_eq!(source_selected.len(), 1);
        assert_eq!(source_selected[0].envelope.message_id, Some(first_id));

        let (mut first_row, _) = metadata_row("for chat a", None, TEST_SENDER);
        first_row.message_id = Some(first_id);
        first_row.destination_chat_id = Some(chat_a);
        let (mut second_row, _) = metadata_row("for chat b", None, TEST_SENDER);
        second_row.message_id = Some(second_id);
        second_row.destination_chat_id = second.destination_chat_id;
        let (_counts, metadata_selected) =
            metadata_selection::selection_state_for_mailbox_metadata_rows(
                &[first_row, second_row],
                &query,
                None,
            );
        assert_eq!(metadata_selected.len(), 1);
        assert_eq!(metadata_selected[0].envelope.message_id, Some(first_id));

        let mut by_id = query.clone();
        by_id.mailbox.message_id_filter = Some(second_id);
        assert!(selected_after_filters(&source_messages, &by_id, None).is_empty());
    }

    #[test]
    fn chat_qualified_read_excludes_fresh_bare_agent_addressed_unread_mail() {
        let message_id = AtmMessageId::new();
        let caller_chat_id = "recipient-session".parse::<ChatId>().expect("chat id");
        let message = message("fresh unread", message_id, None, None, false);
        let query = base_read_query()
            .with_selection_mode(ReadSelection::Unread)
            .with_caller_chat_id(Some(caller_chat_id));

        let selected = selected_after_filters(
            &[SourcedMessage {
                envelope: message,
                source_path: PathBuf::from("recipient.json"),
                source_index: 0.into(),
            }],
            &query,
            None,
        );
        assert!(selected.is_empty());

        let (mut metadata_row, _) = metadata_row("fresh unread", None, TEST_SENDER);
        metadata_row.message_id = Some(message_id);
        let (_counts, metadata_selected) =
            metadata_selection::selection_state_for_mailbox_metadata_rows(
                &[metadata_row],
                &query,
                None,
            );
        assert!(metadata_selected.is_empty());
    }

    fn metadata_row(
        text: &str,
        summary: Option<&str>,
        from: &str,
    ) -> (boundary::MailboxMetadataRow, boundary::Message) {
        let message_id = AtmMessageId::new();
        let message_key = MessageKey::new(format!("atm:{message_id}")).expect("message key");
        let envelope = InboxMessage {
            from: from.parse::<AgentName>().expect("agent"),
            source_chat_id: None,
            text: text.to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            destination_chat_id: None,
            summary: summary.map(str::to_string),
            message_id: Some(message_id),
            requires_ack: false,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            placement: None,
            task_op: None,
            task_complete: None,
            extra: serde_json::Map::new(),
        };
        (
            boundary::MailboxMetadataRow {
                message_key: message_key.clone(),
                message_id: Some(message_id),
                parent_message_id: None,
                thread_mode: None,
                from_agent: from.parse::<AgentName>().expect("agent"),
                source_chat_id: None,
                destination_chat_id: None,
                summary: summary.map(str::to_string),
                message_at: envelope.timestamp,
                read: false,
                requires_ack: false,
                pending_ack: false,
                acknowledged_at: None,
                expires_at: None,
                task_id: None,
            },
            boundary::Message {
                team: TEST_TEAM.parse::<TeamName>().expect("team"),
                agent: "recipient".parse::<AgentName>().expect("agent"),
                message_key,
                envelope,
            },
        )
    }

    #[test]
    fn read_query_new_rejects_invalid_target_before_command_execution() {
        let tempdir = tempdir().expect("tempdir");
        let error = ReadQuery::new(
            tempdir.path().to_path_buf(),
            tempdir.path().to_path_buf(),
            TEST_SENDER.parse().expect("caller"),
            Some("../evil"),
            TEST_TEAM.parse().expect("team"),
            ReadSelection::Actionable,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect_err("invalid target");

        assert!(error.message().contains("agent name"));
    }

    #[test]
    fn read_ephemeral_message_is_hidden_outside_view_all() {
        let message_id = AtmMessageId::new();
        let expires_at =
            IsoTimestamp::from_datetime(chrono::Utc::now() + chrono::Duration::minutes(30));
        let messages = vec![SourcedMessage {
            envelope: InboxMessage {
                expires_at: Some(expires_at),
                ..message("ephemeral", message_id, None, None, true)
            },
            source_path: PathBuf::from("recipient.json"),
            source_index: 0.into(),
        }];
        let mut actionable = base_read_query();
        actionable.mailbox.selection_mode = ReadSelection::Actionable;
        let mut all = actionable.clone();
        all.mailbox.selection_mode = ReadSelection::All;

        assert!(selected_after_filters(&messages, &actionable, None).is_empty());
        assert_eq!(selected_after_filters(&messages, &all, None).len(), 1);
    }

    #[test]
    fn expired_ephemeral_message_is_cleaned_from_all_views() {
        let message_id = AtmMessageId::new();
        let expires_at =
            IsoTimestamp::from_datetime(chrono::Utc::now() - chrono::Duration::minutes(1));
        let messages = vec![SourcedMessage {
            envelope: InboxMessage {
                expires_at: Some(expires_at),
                ..message("expired ephemeral", message_id, None, None, false)
            },
            source_path: PathBuf::from("recipient.json"),
            source_index: 0.into(),
        }];
        let mut actionable = base_read_query();
        actionable.mailbox.selection_mode = ReadSelection::Actionable;
        let mut all = actionable.clone();
        all.mailbox.selection_mode = ReadSelection::All;

        assert!(selected_after_filters(&messages, &actionable, None).is_empty());
        assert!(selected_after_filters(&messages, &all, None).is_empty());
    }

    #[test]
    fn metadata_backed_read_contains_fetches_durable_body_when_summary_misses() {
        let tempdir = tempdir().expect("tempdir");
        let (mut metadata_row, mut message_record) = metadata_row(
            "durable body with needle",
            Some("summary miss"),
            TEST_SENDER,
        );
        metadata_row.read = true;
        message_record.envelope.read = true;
        let load_count = Arc::new(AtomicUsize::new(0));
        let runtime = ReadRuntime {
            roster_present: true,
            metadata_rows: vec![metadata_row],
            metadata_row_batches: None,
            message_records: HashMap::from([(message_record.message_key.clone(), message_record)]),
            query_mailbox_metadata_rows_count: Arc::new(AtomicUsize::new(0)),
            load_message_record_count: load_count.clone(),
            save_seen_watermark_count: Arc::new(AtomicUsize::new(0)),
            persist_message_state_count: Arc::new(AtomicUsize::new(0)),
            fail_load_message_record: false,
        };
        let query = ReadQuery::new(
            tempdir.path().to_path_buf(),
            tempdir.path().to_path_buf(),
            "recipient".parse().expect("caller"),
            None,
            TEST_TEAM.parse().expect("team"),
            ReadSelection::All,
            false,
            false,
            None,
            None,
            None,
            None,
            Some("needle"),
            None,
        )
        .expect("read query");

        let outcome =
            read_mail_with_runtime_impl(query, &NullObservability, &runtime).expect("read outcome");

        assert_eq!(outcome.count, 1);
        assert_eq!(
            outcome
                .message
                .as_ref()
                .map(|message| message.envelope.text.as_str()),
            Some("durable body with needle")
        );
        assert_eq!(load_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn metadata_contains_uses_selection_snapshot_when_durable_row_vanishes() {
        let tempdir = tempdir().expect("tempdir");
        let (metadata_row, message_record) =
            metadata_row("durable body", Some("summary miss"), TEST_SENDER);
        let rows = vec![metadata_row.clone()];
        let classified = metadata_selection::classify_mailbox_metadata_rows(&rows);
        let runtime = ReadRuntime {
            roster_present: true,
            metadata_rows: rows.clone(),
            metadata_row_batches: None,
            message_records: HashMap::new(),
            query_mailbox_metadata_rows_count: Arc::new(AtomicUsize::new(0)),
            load_message_record_count: Arc::new(AtomicUsize::new(0)),
            save_seen_watermark_count: Arc::new(AtomicUsize::new(0)),
            persist_message_state_count: Arc::new(AtomicUsize::new(0)),
            fail_load_message_record: false,
        };
        let filtered = metadata_selection::filter_metadata_backed_contains_candidates(
            &runtime,
            tempdir.path(),
            &TEST_TEAM.parse().expect("team"),
            &TEST_SENDER.parse().expect("agent"),
            &rows,
            classified,
            Some("needle"),
        )
        .expect("vanished durable row falls back to selected snapshot");
        assert!(filtered.is_empty());
        assert!(!message_record.envelope.text.contains("needle"));
    }

    #[test]
    fn peek_mail_with_runtime_does_not_persist_message_state_or_seen_watermark() {
        let tempdir = tempdir().expect("tempdir");
        let (metadata_row, message_record) =
            metadata_row("peek target", Some("peek summary"), TEST_SENDER);
        let persist_count = Arc::new(AtomicUsize::new(0));
        let seen_count = Arc::new(AtomicUsize::new(0));
        let runtime = ReadRuntime {
            roster_present: true,
            metadata_rows: vec![metadata_row],
            metadata_row_batches: None,
            message_records: HashMap::from([(message_record.message_key.clone(), message_record)]),
            query_mailbox_metadata_rows_count: Arc::new(AtomicUsize::new(0)),
            load_message_record_count: Arc::new(AtomicUsize::new(0)),
            save_seen_watermark_count: seen_count.clone(),
            persist_message_state_count: persist_count.clone(),
            fail_load_message_record: false,
        };

        let outcome = peek_mail_with_runtime_impl(
            explicit_peek_query(tempdir.path().to_path_buf(), tempdir.path().to_path_buf()),
            &NullObservability,
            &runtime,
        )
        .expect("peek outcome");

        assert_eq!(outcome.action, CommandAction::Peek);
        assert!(!outcome.mutation_applied);
        assert_eq!(outcome.count, 1);
        assert_eq!(
            outcome
                .message
                .as_ref()
                .map(|message| message.envelope.read),
            Some(false)
        );
        assert_eq!(persist_count.load(Ordering::SeqCst), 0);
        assert_eq!(seen_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn chat_qualified_peek_finds_only_the_callers_explicitly_addressed_mail() {
        let tempdir = tempdir().expect("tempdir");
        let caller_chat_id = "recipient-session".parse::<ChatId>().expect("chat id");
        let other_chat_id = "other-session".parse::<ChatId>().expect("chat id");
        let (mut caller_row, mut caller_record) =
            metadata_row("for caller session", Some("caller"), TEST_SENDER);
        caller_row.destination_chat_id = Some(caller_chat_id.clone());
        caller_record.envelope.destination_chat_id = Some(caller_chat_id.clone());
        let caller_key = caller_record.message_key.clone();
        let (mut other_row, mut other_record) =
            metadata_row("for other session", Some("other"), TEST_SENDER);
        other_row.destination_chat_id = Some(other_chat_id.clone());
        other_record.envelope.destination_chat_id = Some(other_chat_id);
        let other_key = other_record.message_key.clone();
        let runtime = ReadRuntime {
            roster_present: true,
            metadata_rows: vec![caller_row, other_row],
            metadata_row_batches: None,
            message_records: HashMap::from([
                (caller_key, caller_record),
                (other_key, other_record),
            ]),
            query_mailbox_metadata_rows_count: Arc::new(AtomicUsize::new(0)),
            load_message_record_count: Arc::new(AtomicUsize::new(0)),
            save_seen_watermark_count: Arc::new(AtomicUsize::new(0)),
            persist_message_state_count: Arc::new(AtomicUsize::new(0)),
            fail_load_message_record: false,
        };
        let query = PeekQuery::new(
            tempdir.path().to_path_buf(),
            tempdir.path().to_path_buf(),
            "recipient".parse().expect("caller"),
            None,
            TEST_TEAM.parse().expect("team"),
            ReadSelection::All,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("peek query")
        .with_caller_chat_id(Some(caller_chat_id));

        let outcome =
            peek_mail_with_runtime_impl(query, &NullObservability, &runtime).expect("peek outcome");

        assert_eq!(outcome.count, 1);
        assert_eq!(
            outcome
                .message
                .as_ref()
                .map(|message| message.envelope.text.as_str()),
            Some("for caller session")
        );
        assert!(!outcome.mutation_applied);
    }

    #[test]
    fn read_mail_rejects_explicit_cross_agent_targets_on_mutating_path() {
        let tempdir = tempdir().expect("tempdir");
        let runtime = ReadRuntime {
            roster_present: true,
            metadata_rows: Vec::new(),
            metadata_row_batches: None,
            message_records: HashMap::new(),
            query_mailbox_metadata_rows_count: Arc::new(AtomicUsize::new(0)),
            load_message_record_count: Arc::new(AtomicUsize::new(0)),
            save_seen_watermark_count: Arc::new(AtomicUsize::new(0)),
            persist_message_state_count: Arc::new(AtomicUsize::new(0)),
            fail_load_message_record: false,
        };

        let error = read_mail_with_runtime_impl(
            explicit_read_query(tempdir.path().to_path_buf(), tempdir.path().to_path_buf()),
            &NullObservability,
            &runtime,
        )
        .expect_err("cross-agent owner-only read must fail");

        assert!(
            error.code() == crate::error_codes::AtmErrorCode::MessageValidationFailed,
            "{error:?}"
        );
        assert!(
            error.message().contains("owner-only `atm read`"),
            "{error:?}"
        );
    }

    #[test]
    fn peek_mail_rejects_explicit_targets_missing_from_atm_roster() {
        let tempdir = tempdir().expect("tempdir");
        let runtime = ReadRuntime {
            roster_present: false,
            metadata_rows: Vec::new(),
            metadata_row_batches: None,
            message_records: HashMap::new(),
            query_mailbox_metadata_rows_count: Arc::new(AtomicUsize::new(0)),
            load_message_record_count: Arc::new(AtomicUsize::new(0)),
            save_seen_watermark_count: Arc::new(AtomicUsize::new(0)),
            persist_message_state_count: Arc::new(AtomicUsize::new(0)),
            fail_load_message_record: false,
        };

        let error = peek_mail_with_runtime_impl(
            explicit_peek_query(tempdir.path().to_path_buf(), tempdir.path().to_path_buf()),
            &NullObservability,
            &runtime,
        )
        .expect_err("peek with explicit missing roster target must fail");

        assert_eq!(
            error.code(),
            crate::error_codes::AtmErrorCode::AgentNotFound
        );
        assert!(error.message().contains("recipient"), "{error:?}");
    }

    #[test]
    fn read_wait_propagates_contains_reload_errors_instead_of_timeout() {
        let tempdir = tempdir().expect("tempdir");
        let (metadata_row, message_record) = metadata_row(
            "durable body with needle",
            Some("summary miss"),
            TEST_SENDER,
        );
        let runtime = ReadRuntime {
            roster_present: true,
            metadata_rows: Vec::new(),
            metadata_row_batches: Some(vec![Vec::new(), vec![metadata_row]]),
            message_records: HashMap::from([(message_record.message_key.clone(), message_record)]),
            query_mailbox_metadata_rows_count: Arc::new(AtomicUsize::new(0)),
            load_message_record_count: Arc::new(AtomicUsize::new(0)),
            save_seen_watermark_count: Arc::new(AtomicUsize::new(0)),
            persist_message_state_count: Arc::new(AtomicUsize::new(0)),
            fail_load_message_record: true,
        };
        let query = ReadQuery::new(
            tempdir.path().to_path_buf(),
            tempdir.path().to_path_buf(),
            "recipient".parse().expect("caller"),
            None,
            TEST_TEAM.parse().expect("team"),
            ReadSelection::All,
            false,
            false,
            None,
            None,
            None,
            None,
            Some("needle"),
            Some(1),
        )
        .expect("read query");

        let error = read_mail_with_runtime_impl(query, &NullObservability, &runtime)
            .expect_err("durable reload failure should surface");

        assert!(
            error.code() == crate::error_codes::AtmErrorCode::MailboxReadFailed,
            "{error:?}"
        );
        assert!(error.message().contains("simulated durable reload failure"));
    }

    #[test]
    fn mutating_read_reloads_from_the_selection_snapshot_when_rows_vanish_concurrently() {
        let tempdir = tempdir().expect("tempdir");
        let (metadata_row, message_record) =
            metadata_row("unread body", Some("unread summary"), TEST_SENDER);
        let expected_message_id = message_record.envelope.message_id;
        let persist_count = Arc::new(AtomicUsize::new(0));
        // The first query feeds selection; a concurrent `clear` then removes
        // every row and the durable record before the post-mutation re-query
        // runs, so the read must complete with the selected snapshot.
        let runtime = ReadRuntime {
            roster_present: true,
            metadata_rows: Vec::new(),
            metadata_row_batches: Some(vec![vec![metadata_row], Vec::new()]),
            message_records: HashMap::new(),
            query_mailbox_metadata_rows_count: Arc::new(AtomicUsize::new(0)),
            load_message_record_count: Arc::new(AtomicUsize::new(0)),
            save_seen_watermark_count: Arc::new(AtomicUsize::new(0)),
            persist_message_state_count: persist_count.clone(),
            fail_load_message_record: false,
        };
        let query = ReadQuery::new(
            tempdir.path().to_path_buf(),
            tempdir.path().to_path_buf(),
            "recipient".parse().expect("caller"),
            None,
            TEST_TEAM.parse().expect("team"),
            ReadSelection::All,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query");

        let outcome = read_mail_with_runtime_impl(query, &NullObservability, &runtime)
            .expect("read completes with the selected snapshot");

        assert!(outcome.mutation_applied);
        assert_eq!(persist_count.load(Ordering::SeqCst), 1);
        assert_eq!(outcome.count, 1);
        let message = outcome
            .message
            .as_ref()
            .expect("selected snapshot is returned");
        assert_eq!(message.envelope.message_id, expected_message_id);
        assert_eq!(message.envelope.summary.as_deref(), Some("unread summary"));
    }
}
