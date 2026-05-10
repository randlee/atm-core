use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::address::AgentAddress;
use crate::error::AtmError;
use crate::identity;
use crate::mailbox::source::resolve_target;
use crate::observability::ObservabilityPort;
use crate::read::{
    BucketCounts, ClassifiedMessage, ReadQuery, selection_state_for_source_files,
    sort_and_limit_selected,
};
use crate::schema::LegacyMessageId;
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::types::{AgentName, IsoTimestamp, ReadSelection, TaskId, TeamName};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<AgentName>,
    pub target_address: Option<AgentAddress>,
    pub team_override: Option<TeamName>,
    pub selection_mode: ReadSelection,
    pub seen_state_filter: bool,
    pub limit: Option<usize>,
    pub sender_filter: Option<AgentName>,
    pub timestamp_filter: Option<IsoTimestamp>,
    pub task_filter: Option<TaskId>,
    pub contains_filter: Option<String>,
}

impl ListQuery {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        home_dir: PathBuf,
        current_dir: PathBuf,
        actor_override: Option<&str>,
        target_address: Option<&str>,
        team_override: Option<&str>,
        selection_mode: ReadSelection,
        seen_state_filter: bool,
        limit: Option<usize>,
        sender_filter: Option<&str>,
        timestamp_filter: Option<IsoTimestamp>,
        task_filter: Option<&str>,
        contains_filter: Option<&str>,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            home_dir,
            current_dir,
            actor_override: actor_override.map(str::parse).transpose()?,
            target_address: target_address.map(str::parse).transpose()?,
            team_override: team_override.map(str::parse).transpose()?,
            selection_mode,
            seen_state_filter,
            limit,
            sender_filter: sender_filter.map(str::parse).transpose()?,
            timestamp_filter,
            task_filter: task_filter.map(str::parse).transpose()?,
            contains_filter: contains_filter.map(ToOwned::to_owned),
        })
    }

    pub(crate) fn as_read_query(&self) -> ReadQuery {
        ReadQuery {
            home_dir: self.home_dir.clone(),
            current_dir: self.current_dir.clone(),
            actor_override: self.actor_override.clone(),
            target_address: self.target_address.clone(),
            team_override: self.team_override.clone(),
            selection_mode: self.selection_mode,
            seen_state_filter: self.seen_state_filter,
            seen_state_update: false,
            ack_activation_mode: crate::types::AckActivationMode::ReadOnly,
            message_id_filter: None,
            sender_filter: self.sender_filter.clone(),
            timestamp_filter: self.timestamp_filter,
            task_filter: self.task_filter.clone(),
            contains_filter: self.contains_filter.clone(),
            timeout_secs: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRow {
    #[serde(default)]
    pub message_id: Option<LegacyMessageId>,
    pub summary: String,
    pub from: AgentName,
    pub timestamp: IsoTimestamp,
    pub read: bool,
    pub pending_ack: bool,
    pub task_id: Option<TaskId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListOutcome {
    pub action: String,
    pub team: TeamName,
    pub agent: AgentName,
    pub selection_mode: ReadSelection,
    pub history_collapsed: bool,
    pub count: usize,
    pub rows: Vec<ListRow>,
    pub bucket_counts: BucketCounts,
}

pub fn list_mail(
    query: ListQuery,
    observability: &dyn ObservabilityPort,
) -> Result<ListOutcome, AtmError> {
    let runtime = LocalServiceRuntime::default();
    list_mail_with_runtime(query, observability, &runtime)
}

fn list_mail_with_runtime<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    query: ListQuery,
    _observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<ListOutcome, AtmError> {
    let config = runtime.load_config(&query.current_dir)?;
    let actor = identity::resolve_actor_identity(query.actor_override.as_deref(), config.as_ref())?;
    let target = resolve_target(
        query.target_address.as_ref(),
        &actor,
        query.team_override.as_ref(),
        config.as_ref(),
    )?;
    let team_dir = runtime.team_dir(&query.home_dir, &target.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&target.team).with_recovery(
            "Create the team config for the requested team or target a different team before retrying `atm list`.",
        ));
    }

    let team_config = runtime.load_team_config(&team_dir)?;
    if target.explicit
        && !team_config
            .members
            .iter()
            .any(|member| member.name == target.agent.as_str())
    {
        return Err(
            AtmError::agent_not_found(&target.agent, &target.team).with_recovery(
                "Update the team membership in config.json or list a different mailbox target.",
            ),
        );
    }

    let seen_watermark = if query.seen_state_filter && query.selection_mode != ReadSelection::All {
        runtime.load_seen_watermark(&query.home_dir, &target.team, &target.agent)?
    } else {
        None
    };

    let workflow_state =
        runtime.load_workflow_state(&query.home_dir, &target.team, &target.agent)?;
    let source_files =
        runtime.observe_source_files(&query.home_dir, &target.team, &target.agent)?;
    let read_query = query.as_read_query();
    let (bucket_counts, mut selected) = selection_state_for_source_files(
        &source_files,
        &workflow_state,
        &read_query,
        seen_watermark,
    );
    sort_and_limit_selected(&mut selected, query.limit);

    let rows = selected
        .iter()
        .map(list_row_from_message)
        .collect::<Vec<_>>();
    let history_collapsed = query.selection_mode != ReadSelection::All && bucket_counts.history > 0;

    Ok(ListOutcome {
        action: "list".to_string(),
        team: target.team,
        agent: target.agent,
        selection_mode: query.selection_mode,
        history_collapsed,
        count: rows.len(),
        rows,
        bucket_counts,
    })
}

fn list_row_from_message(message: &ClassifiedMessage) -> ListRow {
    ListRow {
        message_id: message.envelope.message_id,
        summary: summary_for_row(&message.envelope),
        from: message.envelope.from.clone(),
        timestamp: message.envelope.timestamp,
        read: message.envelope.read,
        pending_ack: message.envelope.pending_ack_at.is_some()
            && message.envelope.acknowledged_at.is_none(),
        task_id: message.envelope.task_id.clone(),
    }
}

fn summary_for_row(envelope: &crate::schema::MessageEnvelope) -> String {
    if let Some(summary) = envelope
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return summary.to_string();
    }

    let first_line = envelope
        .text
        .lines()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("");
    let mut summary = first_line.to_string();
    if summary.len() > 120 {
        summary.truncate(117);
        summary.push_str("...");
    }
    summary
}
