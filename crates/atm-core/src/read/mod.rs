pub(crate) mod filters;
pub(crate) mod metadata_selection;
pub(crate) mod seen_state;
pub(crate) mod state;
pub(crate) mod wait;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::address::{AgentAddress, MessageParticipantFilter, ParticipantDirection};
use crate::boundary;
use crate::error::AtmError;
use crate::mailbox::source::resolve_target;
use crate::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use crate::schema::{AtmMessageId, InboxMessage};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{
    AgentName, ChatId, CommandAction, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection,
    SourceIndex, TaskId, TeamName,
};
use metadata_selection::{
    filter_metadata_backed_contains_candidates, load_durable_metadata_message,
    selection_state_for_mailbox_metadata_rows, sort_and_limit_selected,
};

pub const MAX_CONTAINS_FILTER_LEN: usize = 1024;
pub const MAX_TIMEOUT_SECS: u64 = 3600;

/// Parameters for querying and optionally mutating one mailbox display surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxQueryFields {
    pub(crate) home_dir: PathBuf,
    pub(crate) current_dir: PathBuf,
    pub(crate) target_address: Option<AgentAddress>,
    pub(crate) selection_mode: ReadSelection,
    pub(crate) seen_state_filter: bool,
    pub(crate) message_id_filter: Option<AtmMessageId>,
    pub(crate) sender_filter: Option<AgentName>,
    pub(crate) participant_filter: Option<MessageParticipantFilter>,
    pub(crate) timestamp_filter: Option<IsoTimestamp>,
    pub(crate) task_filter: Option<TaskId>,
    pub(crate) contains_filter: Option<String>,
    pub(crate) timeout_secs: Option<u64>,
}

impl MailboxQueryFields {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        home_dir: PathBuf,
        current_dir: PathBuf,
        target_address: Option<&str>,
        selection_mode: ReadSelection,
        seen_state_filter: bool,
        message_id_filter: Option<&str>,
        sender_filter: Option<&str>,
        timestamp_filter: Option<IsoTimestamp>,
        task_filter: Option<&str>,
        contains_filter: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> Result<Self, AtmError> {
        let contains_filter = normalize_contains_filter(contains_filter)?;
        let timeout_secs = validate_timeout_secs(timeout_secs)?;
        Ok(Self {
            home_dir,
            current_dir,
            target_address: target_address.map(str::parse).transpose()?,
            selection_mode,
            seen_state_filter,
            message_id_filter: message_id_filter
                .map(|value| {
                    value.parse::<AtmMessageId>().map_err(|_source| {
                        AtmError::validation(format!("invalid message id: {value}"))
                    })
                })
                .transpose()?,
            sender_filter: sender_filter.map(str::parse).transpose()?,
            participant_filter: None,
            timestamp_filter,
            task_filter: task_filter.map(str::parse).transpose()?,
            contains_filter,
            timeout_secs,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn build_mailbox_query_fields(
    home_dir: PathBuf,
    current_dir: PathBuf,
    target_address: Option<&str>,
    selection_mode: ReadSelection,
    seen_state_filter: bool,
    message_id_filter: Option<&str>,
    sender_filter: Option<&str>,
    timestamp_filter: Option<IsoTimestamp>,
    task_filter: Option<&str>,
    contains_filter: Option<&str>,
    timeout_secs: Option<u64>,
) -> Result<MailboxQueryFields, AtmError> {
    MailboxQueryFields::new(
        home_dir,
        current_dir,
        target_address,
        selection_mode,
        seen_state_filter,
        message_id_filter,
        sender_filter,
        timestamp_filter,
        task_filter,
        contains_filter,
        timeout_secs,
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeekQuery {
    pub(crate) mailbox: MailboxQueryFields,
    pub(crate) caller_identity: AgentName,
    pub(crate) caller_team: TeamName,
}

impl PeekQuery {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        home_dir: PathBuf,
        current_dir: PathBuf,
        caller_identity: AgentName,
        target_address: Option<&str>,
        caller_team: TeamName,
        selection_mode: ReadSelection,
        seen_state_filter: bool,
        message_id_filter: Option<&str>,
        sender_filter: Option<&str>,
        timestamp_filter: Option<IsoTimestamp>,
        task_filter: Option<&str>,
        contains_filter: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> Result<Self, AtmError> {
        let mut mailbox = build_mailbox_query_fields(
            home_dir,
            current_dir,
            target_address,
            selection_mode,
            seen_state_filter,
            message_id_filter,
            sender_filter,
            timestamp_filter,
            task_filter,
            contains_filter,
            timeout_secs,
        )?;
        mailbox.participant_filter = Some(MessageParticipantFilter {
            agent: caller_identity.clone(),
            chat_id: None,
            direction: ParticipantDirection::To,
        });
        Ok(Self {
            mailbox,
            caller_identity,
            caller_team,
        })
    }
}

/// Parameters for querying and optionally mutating one mailbox display surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadQuery {
    pub(crate) mailbox: MailboxQueryFields,
    pub(crate) caller_identity: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) caller_chat_id: Option<ChatId>,
    pub(crate) caller_team: TeamName,
    pub(crate) seen_state_update: bool,
}

impl ReadQuery {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        home_dir: PathBuf,
        current_dir: PathBuf,
        caller_identity: AgentName,
        target_address: Option<&str>,
        caller_team: TeamName,
        selection_mode: ReadSelection,
        seen_state_filter: bool,
        seen_state_update: bool,
        message_id_filter: Option<&str>,
        sender_filter: Option<&str>,
        timestamp_filter: Option<IsoTimestamp>,
        task_filter: Option<&str>,
        contains_filter: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            mailbox: build_mailbox_query_fields(
                home_dir,
                current_dir,
                target_address,
                selection_mode,
                seen_state_filter,
                message_id_filter,
                sender_filter,
                timestamp_filter,
                task_filter,
                contains_filter,
                timeout_secs,
            )?,
            caller_identity,
            caller_chat_id: None,
            caller_team,
            seen_state_update,
        })
    }

    pub fn team_override(&self) -> Option<&TeamName> {
        Some(&self.caller_team)
    }

    pub fn selection_mode(&self) -> ReadSelection {
        self.mailbox.selection_mode
    }

    pub fn seen_state_filter(&self) -> bool {
        self.mailbox.seen_state_filter
    }

    pub fn seen_state_update(&self) -> bool {
        self.seen_state_update
    }

    pub fn message_id_filter(&self) -> Option<&AtmMessageId> {
        self.mailbox.message_id_filter.as_ref()
    }

    pub fn timeout_secs(&self) -> Option<u64> {
        self.mailbox.timeout_secs
    }

    pub fn caller_chat_id(&self) -> Option<&ChatId> {
        self.caller_chat_id.as_ref()
    }

    #[must_use]
    pub fn with_caller_chat_id(mut self, caller_chat_id: Option<ChatId>) -> Self {
        self.mailbox.participant_filter = Some(MessageParticipantFilter {
            agent: self.caller_identity.clone(),
            chat_id: caller_chat_id.clone(),
            direction: ParticipantDirection::To,
        });
        self.caller_chat_id = caller_chat_id;
        self
    }

    pub fn with_selection_mode(mut self, selection_mode: ReadSelection) -> Self {
        self.mailbox.selection_mode = selection_mode;
        self
    }
}

pub(crate) fn normalize_contains_filter(
    contains_filter: Option<&str>,
) -> Result<Option<String>, AtmError> {
    match contains_filter
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) if value.len() > MAX_CONTAINS_FILTER_LEN => Err(AtmError::validation(format!(
            "contains filter exceeds the {}-byte maximum",
            MAX_CONTAINS_FILTER_LEN
        ))),
        Some(value) => Ok(Some(value.to_ascii_lowercase())),
        None => Ok(None),
    }
}

fn validate_timeout_secs(timeout_secs: Option<u64>) -> Result<Option<u64>, AtmError> {
    match timeout_secs {
        Some(value) if value > MAX_TIMEOUT_SECS => Err(AtmError::validation(format!(
            "timeout exceeds the {} second maximum",
            MAX_TIMEOUT_SECS
        ))),
        _ => Ok(timeout_secs),
    }
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
    query: PeekQuery,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<ReadOutcome, AtmError> {
    let synthesized = ReadQuery {
        mailbox: query.mailbox,
        caller_identity: query.caller_identity,
        caller_chat_id: None,
        caller_team: query.caller_team,
        seen_state_update: false,
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
    query: ReadQuery,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<ReadOutcome, AtmError> {
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
    metadata_rows: Vec<boundary::MailStoreMailboxMetadataRow>,
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
    Vec<boundary::MailStoreMailboxMetadataRow>,
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
    let metadata_rows = load_checked_read_metadata(
        runtime,
        &query.mailbox.home_dir,
        &target.team,
        &target.agent,
    )?;
    let (updated_counts, _updated_selected) =
        selection_state_for_mailbox_metadata_rows(&metadata_rows, query, seen_watermark);
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
                &metadata_rows,
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
        .load_roster_member(&target.team, &target.agent)?
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
) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
    let metadata_rows = runtime.query_mailbox_metadata_rows(home_dir, team, agent, None)?;
    Ok(metadata_rows)
}

fn output_messages_from_metadata_selection<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    metadata_rows: &[boundary::MailStoreMailboxMetadataRow],
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
        BucketCounts, ClassifiedMessage, PeekQuery, ReadQuery, metadata_selection,
        peek_mail_with_runtime_impl, read_mail_with_runtime_impl, state,
    };
    use crate::boundary::{self, MessageKey, RosterHarness, RosterMemberKind};
    use crate::error::AtmError;
    use crate::mailbox::source::SourceFile;
    use crate::mailbox::source::SourcedMessage;
    use crate::mailbox::surface::dedupe_message_id_surface;
    use crate::observability::NullObservability;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::{AtmMessageId, InboxMessage, ThreadMode};
    use crate::service_runtime::RetainedServiceRuntime;
    use crate::service_runtime_store::RetainedMailboxRuntime;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::threading::ThreadIndex;
    use crate::types::{
        AgentName, ChatId, CommandAction, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection,
        TaskId, TeamName,
    };

    fn selection_state_for_source_files(
        source_files: &[SourceFile],
        query: &ReadQuery,
        seen_watermark: Option<IsoTimestamp>,
    ) -> (BucketCounts, Vec<ClassifiedMessage>) {
        let classified_all =
            classify_all(apply_idle_notification_dedup(dedupe_message_id_surface(
                merged_surface(source_files),
                |message: &SourcedMessage| message.envelope.message_id,
                |message: &SourcedMessage| message.envelope.timestamp,
            )));
        if let Some(message_id) = query.mailbox.message_id_filter {
            let selected = classified_all
                .iter()
                .filter(|message| message.envelope.message_id == Some(message_id))
                .filter(|message| {
                    crate::read::filters::matches_participant_filter(
                        message,
                        query.mailbox.participant_filter.as_ref(),
                    )
                })
                .cloned()
                .collect();
            let logical_current = metadata_selection::logical_current_messages(classified_all);
            let bucket_counts = metadata_selection::bucket_counts_for(&logical_current);
            return (bucket_counts, selected);
        }
        let logical_current = metadata_selection::logical_current_messages(classified_all);
        let bucket_counts = metadata_selection::bucket_counts_for(&logical_current);
        let filtered = crate::read::filters::apply_contains_filter(
            metadata_selection::apply_metadata_only_filters(
                logical_current,
                query.mailbox.sender_filter.as_ref(),
                query.mailbox.participant_filter.as_ref(),
                query.mailbox.timestamp_filter,
                query.mailbox.task_filter.as_ref(),
            ),
            query.mailbox.contains_filter.as_deref(),
        );
        let selected = metadata_selection::select_messages(
            &filtered,
            query.mailbox.selection_mode,
            seen_watermark,
        );
        (bucket_counts, selected)
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
                metadata_selection::logical_current_messages(classified),
                query.mailbox.sender_filter.as_ref(),
                query.mailbox.participant_filter.as_ref(),
                query.mailbox.timestamp_filter,
                query.mailbox.task_filter.as_ref(),
            ),
            query.mailbox.contains_filter.as_deref(),
        );
        metadata_selection::select_messages(&filtered, query.mailbox.selection_mode, seen_watermark)
    }

    fn merged_surface(source_files: &[SourceFile]) -> Vec<SourcedMessage> {
        source_files
            .iter()
            .flat_map(|source| {
                source
                    .messages
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(source_index, envelope)| SourcedMessage {
                        envelope,
                        source_path: source.path.clone(),
                        source_index: source_index.into(),
                    })
            })
            .collect()
    }

    fn apply_idle_notification_dedup(deduped: Vec<SourcedMessage>) -> Vec<SourcedMessage> {
        let latest_idle_for_sender = messages_from_idle_sender(&deduped);

        deduped
            .into_iter()
            .enumerate()
            .filter_map(|(index, message)| {
                dedupe_idle_notifications(index, &message, &latest_idle_for_sender)
                    .then_some(message)
            })
            .collect()
    }

    fn dedupe_idle_notifications(
        index: usize,
        message: &SourcedMessage,
        latest_idle_for_sender: &HashMap<AgentName, usize>,
    ) -> bool {
        if !is_unread_idle_notification(&message.envelope) {
            return true;
        }

        idle_sender(&message.envelope)
            .and_then(|sender| latest_idle_for_sender.get(&sender))
            .map(|keep_index| *keep_index == index)
            .unwrap_or(true)
    }

    fn messages_from_idle_sender(messages: &[SourcedMessage]) -> HashMap<AgentName, usize> {
        let mut latest_idle_for_sender = HashMap::new();

        for (index, message) in messages.iter().enumerate() {
            if !is_unread_idle_notification(&message.envelope) {
                continue;
            }

            if let Some(sender) = idle_sender(&message.envelope) {
                latest_idle_for_sender
                    .entry(sender)
                    .and_modify(|keep_index| *keep_index = index)
                    .or_insert(index);
            }
        }

        latest_idle_for_sender
    }

    fn is_unread_idle_notification(message: &InboxMessage) -> bool {
        !message.read && idle_notification_sender(message).is_some()
    }

    fn idle_sender(message: &InboxMessage) -> Option<AgentName> {
        idle_notification_sender(message)
    }

    fn idle_notification_sender(message: &InboxMessage) -> Option<AgentName> {
        let value = match serde_json::from_str::<Value>(&message.text) {
            Ok(value) => value,
            Err(error) => {
                if message.text.contains("idle_notification") {
                    tracing::debug!(
                        %error,
                        recovery = "Repair or remove the malformed Claude idle-notification JSON. ATM will continue treating the record as a normal mailbox message.",
                        message_text = %message.text,
                        "ignoring malformed idle-notification JSON while classifying read surface"
                    );
                }
                return None;
            }
        };

        if value.get("type").and_then(Value::as_str) != Some("idle_notification") {
            return None;
        }

        match value.get("from").and_then(Value::as_str) {
            Some(sender) => match sender.parse() {
                Ok(sender) => Some(sender),
                Err(error) => {
                    tracing::debug!(
                        %error,
                        recovery = "Ensure Claude idle-notification payloads include a valid ATM agent name in `from`. ATM will continue treating the record as a normal mailbox message.",
                        sender,
                        message_text = %message.text,
                        "ignoring malformed idle-notification payload with invalid `from`"
                    );
                    None
                }
            },
            None => {
                tracing::debug!(
                    recovery = "Ensure Claude idle-notification payloads include a string `from` field. ATM will continue treating the record as a normal mailbox message.",
                    message_text = %message.text,
                    "ignoring malformed idle-notification payload missing string `from`"
                );
                None
            }
        }
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
                let effective =
                    metadata_selection::effective_display_envelope(&projected, &thread_index);
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

    fn sourced_message(index: usize, text: &str) -> SourcedMessage {
        SourcedMessage {
            envelope: message(text, AtmMessageId::new(), None, None, false),
            source_path: PathBuf::from(format!("{TEST_SENDER}.json")),
            source_index: index.into(),
        }
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
        metadata_rows: Vec<boundary::MailStoreMailboxMetadataRow>,
        metadata_row_batches: Option<Vec<Vec<boundary::MailStoreMailboxMetadataRow>>>,
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
        ) -> Result<Option<boundary::RosterEntry>, crate::error::AtmError> {
            Ok(self.roster_present.then(|| boundary::RosterEntry {
                team_name: team.clone(),
                agent_name: agent.clone(),
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: crate::schema::AgentType::default(),
                model: crate::types::ModelName::default(),
                recipient_pane_id: None,
                metadata_json: Map::new(),
            }))
        }

        fn load_team_roster(
            &self,
            _team: &TeamName,
        ) -> Result<Vec<boundary::RosterEntry>, crate::error::AtmError> {
            Ok(Vec::new())
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

        fn query_mailbox_metadata_rows(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _limit: Option<usize>,
        ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, crate::error::AtmError> {
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

    fn metadata_row(
        text: &str,
        summary: Option<&str>,
        from: &str,
    ) -> (boundary::MailStoreMailboxMetadataRow, boundary::Message) {
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
            extra: serde_json::Map::new(),
        };
        (
            boundary::MailStoreMailboxMetadataRow {
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
    fn idle_notification_sender_returns_none_for_malformed_json() {
        let malformed = format!(
            r#"{{"type":"idle_notification","from":"{}""#,
            ROLE_TEAM_LEAD
        );
        let message = sourced_message(0, &malformed);

        assert_eq!(idle_notification_sender(&message.envelope), None);
    }

    #[test]
    fn malformed_idle_notification_adjacent_to_valid_records_remains_readable_and_classifiable() {
        let malformed = format!(
            r#"{{"type":"idle_notification","from":"{}""#,
            ROLE_TEAM_LEAD
        );
        let messages = vec![
            sourced_message(0, &malformed),
            sourced_message(1, "normal unread"),
        ];
        let query = base_read_query();

        let selected = std::panic::catch_unwind(|| selected_after_filters(&messages, &query, None))
            .expect("malformed idle notification should not panic");

        assert_eq!(selected.len(), 2);
        let valid = selected
            .iter()
            .find(|message| message.envelope.text == "normal unread")
            .expect("valid record");
        assert_eq!(valid.class, MessageClass::Unread);
        assert_eq!(valid.bucket, DisplayBucket::Unread);

        let malformed = selected
            .iter()
            .find(|message| message.envelope.text == malformed)
            .expect("malformed record");
        assert_eq!(malformed.class, MessageClass::Unread);
        assert_eq!(malformed.bucket, DisplayBucket::Unread);
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
    fn actionable_selection_prefers_terminal_thread_message() {
        let root_id = AtmMessageId::new();
        let terminal_id = AtmMessageId::new();
        let root_at =
            IsoTimestamp::from_datetime(chrono::Utc::now() - chrono::Duration::seconds(1));
        let terminal_at = IsoTimestamp::now();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                message_at("root", root_id, None, None, false, root_at),
                message_at(
                    "detail",
                    terminal_id,
                    Some(root_id),
                    Some(ThreadMode::AddDetails),
                    false,
                    terminal_at,
                ),
            ],
        }];
        let mut query = base_read_query();
        query.mailbox.selection_mode = ReadSelection::Actionable;

        let (_counts, selected) = selection_state_for_source_files(&source_files, &query, None);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].envelope.message_id, Some(terminal_id));
    }

    #[test]
    fn actionable_selection_preserves_parent_context_for_add_details() {
        let root_id = AtmMessageId::new();
        let detail_id = AtmMessageId::new();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                message("root context", root_id, None, None, false),
                message(
                    "follow-up detail",
                    detail_id,
                    Some(root_id),
                    Some(ThreadMode::AddDetails),
                    false,
                ),
            ],
        }];
        let mut query = base_read_query();
        query.mailbox.selection_mode = ReadSelection::Actionable;
        query.mailbox.contains_filter = Some("root context".to_string());

        let (_counts, selected) = selection_state_for_source_files(&source_files, &query, None);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].envelope.message_id, Some(detail_id));
        assert_eq!(
            selected[0].envelope.text,
            "root context\n\nfollow-up detail"
        );
    }

    #[test]
    fn actionable_selection_does_not_preserve_parent_context_for_supersede() {
        let root_id = AtmMessageId::new();
        let supersede_id = AtmMessageId::new();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                message("root context", root_id, None, None, false),
                message(
                    "replacement instruction",
                    supersede_id,
                    Some(root_id),
                    Some(ThreadMode::Supersede),
                    false,
                ),
            ],
        }];
        let mut query = base_read_query();
        query.mailbox.selection_mode = ReadSelection::Actionable;
        query.mailbox.contains_filter = Some("root context".to_string());

        let (_counts, selected) = selection_state_for_source_files(&source_files, &query, None);

        assert!(selected.is_empty());
    }

    #[test]
    fn successor_after_read_stays_actionable_for_add_details() {
        let root_id = AtmMessageId::new();
        let detail_id = AtmMessageId::new();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                message("root context", root_id, None, None, true),
                message(
                    "follow-up detail",
                    detail_id,
                    Some(root_id),
                    Some(ThreadMode::AddDetails),
                    false,
                ),
            ],
        }];
        let mut query = base_read_query();
        query.mailbox.selection_mode = ReadSelection::Actionable;

        let (_counts, selected) = selection_state_for_source_files(&source_files, &query, None);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].envelope.message_id, Some(detail_id));
        assert_eq!(selected[0].bucket, DisplayBucket::Unread);
    }

    #[test]
    fn successor_after_read_stays_actionable_for_supersede() {
        let root_id = AtmMessageId::new();
        let supersede_id = AtmMessageId::new();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                message("root context", root_id, None, None, true),
                message(
                    "replacement instruction",
                    supersede_id,
                    Some(root_id),
                    Some(ThreadMode::Supersede),
                    false,
                ),
            ],
        }];
        let mut query = base_read_query();
        query.mailbox.selection_mode = ReadSelection::Actionable;

        let (_counts, selected) = selection_state_for_source_files(&source_files, &query, None);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].envelope.message_id, Some(supersede_id));
        assert_eq!(selected[0].bucket, DisplayBucket::Unread);
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
    fn task_filter_matches_logical_current_terminal_message_only() {
        let root_id = AtmMessageId::new();
        let terminal_id = AtmMessageId::new();
        let task_id: TaskId = "TASK-77".parse().expect("task id");
        let root_at =
            IsoTimestamp::from_datetime(chrono::Utc::now() - chrono::Duration::seconds(1));
        let terminal_at = IsoTimestamp::now();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                InboxMessage {
                    task_id: Some(task_id.clone()),
                    ..message_at("root", root_id, None, None, false, root_at)
                },
                InboxMessage {
                    task_id: Some(task_id.clone()),
                    ..message_at(
                        "terminal",
                        terminal_id,
                        Some(root_id),
                        Some(ThreadMode::AddDetails),
                        false,
                        terminal_at,
                    )
                },
            ],
        }];
        let mut query = base_read_query();
        query.mailbox.task_filter = Some(task_id);

        let (_counts, selected) = selection_state_for_source_files(&source_files, &query, None);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].envelope.message_id, Some(terminal_id));
    }

    #[test]
    fn exact_message_id_bypasses_logical_current_collapse() {
        let root_id = AtmMessageId::new();
        let terminal_id = AtmMessageId::new();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                message("root", root_id, None, None, false),
                message(
                    "terminal",
                    terminal_id,
                    Some(root_id),
                    Some(ThreadMode::AddDetails),
                    false,
                ),
            ],
        }];
        let mut query = base_read_query();
        query.mailbox.message_id_filter = Some(root_id);

        let (_counts, selected) = selection_state_for_source_files(&source_files, &query, None);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].envelope.message_id, Some(root_id));
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
}
