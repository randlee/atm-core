use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::address::AgentAddress;
use crate::error::AtmError;
use crate::mailbox::source::resolve_target;
use crate::observability::ObservabilityPort;
use crate::read::{
    BucketCounts, ClassifiedMessage, filters,
    metadata_selection::{
        bucket_counts_for, classify_mailbox_metadata_rows, logical_current_messages,
        select_messages, sort_and_limit_selected,
    },
    normalize_contains_filter,
};
use crate::schema::AtmMessageId;
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{AgentName, CommandAction, IsoTimestamp, ReadSelection, TaskId, TeamName};

const DEFAULT_LIST_LIMIT: usize = 200;
const MAX_LIST_LIMIT: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub target_address: Option<AgentAddress>,
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
        caller_identity: AgentName,
        target_address: Option<&str>,
        caller_team: TeamName,
        selection_mode: ReadSelection,
        seen_state_filter: bool,
        limit: Option<usize>,
        sender_filter: Option<&str>,
        timestamp_filter: Option<IsoTimestamp>,
        task_filter: Option<&str>,
        contains_filter: Option<&str>,
    ) -> Result<Self, AtmError> {
        let limit = normalize_limit(limit)?;
        Ok(Self {
            home_dir,
            current_dir,
            caller_identity,
            caller_team,
            target_address: target_address.map(str::parse).transpose()?,
            selection_mode,
            seen_state_filter,
            limit,
            sender_filter: sender_filter.map(str::parse).transpose()?,
            timestamp_filter,
            task_filter: task_filter.map(str::parse).transpose()?,
            contains_filter: normalize_contains_filter(contains_filter)?,
        })
    }
}

fn normalize_limit(limit: Option<usize>) -> Result<Option<usize>, AtmError> {
    match limit {
        Some(0) => Err(
            AtmError::validation("list limit must be at least 1".to_string()).with_recovery(
                "Use `--limit` with a positive integer before retrying `atm list`.",
            ),
        ),
        Some(value) if value > MAX_LIST_LIMIT => Err(
            AtmError::validation(format!(
                "list limit exceeds the {} row maximum",
                MAX_LIST_LIMIT
            ))
            .with_recovery(
                "Use a smaller `--limit` value before retrying `atm list` so daemon responses remain bounded.",
            ),
        ),
        Some(value) => Ok(Some(value)),
        None => Ok(Some(DEFAULT_LIST_LIMIT)),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRow {
    #[serde(default)]
    pub message_id: Option<AtmMessageId>,
    pub summary: String,
    pub from: AgentName,
    pub timestamp: IsoTimestamp,
    pub read: bool,
    pub pending_ack: bool,
    pub task_id: Option<TaskId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListOutcome {
    pub action: CommandAction,
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
    let runtime = default_runtime()?;
    list_mail_with_runtime_impl(query, observability, &runtime)
}

pub fn list_mail_with_runtime(
    query: ListQuery,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<ListOutcome, AtmError> {
    list_mail_with_runtime_impl(query, observability, runtime)
}

fn list_mail_with_runtime_impl<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    query: ListQuery,
    _observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<ListOutcome, AtmError> {
    let config = runtime.load_config(&query.current_dir)?;
    let actor = query.caller_identity.clone();
    let target = resolve_target(
        query.target_address.as_ref(),
        &actor,
        &query.caller_team,
        config.as_ref(),
    )?;
    let team_dir = runtime.team_dir(&query.home_dir, &target.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&target.team).with_recovery(
            "Create the team config for the requested team or target a different team before retrying `atm list`.",
        ));
    }

    validate_target_member_in_roster(runtime, &target)?;

    let seen_watermark = if query.seen_state_filter && query.selection_mode != ReadSelection::All {
        runtime.load_seen_watermark(&query.home_dir, &target.team, &target.agent)?
    } else {
        None
    };

    let metadata_rows =
        runtime.query_mailbox_metadata_rows(&query.home_dir, &target.team, &target.agent, None)?;
    let classified_all = classify_mailbox_metadata_rows(&metadata_rows);
    let logical_current = logical_current_messages(classified_all);
    let bucket_counts = bucket_counts_for(&logical_current);
    let filtered = apply_list_filters(
        logical_current,
        query.sender_filter.as_ref(),
        query.timestamp_filter,
        query.task_filter.as_ref(),
        query.contains_filter.as_deref(),
    );
    let mut selected = select_messages(&filtered, query.selection_mode, seen_watermark);
    sort_and_limit_selected(&mut selected, query.limit);

    let rows = selected
        .iter()
        .map(list_row_from_message)
        .collect::<Vec<_>>();
    let history_collapsed = query.selection_mode != ReadSelection::All && bucket_counts.history > 0;

    Ok(ListOutcome {
        action: CommandAction::List,
        team: target.team,
        agent: target.agent,
        selection_mode: query.selection_mode,
        history_collapsed,
        count: rows.len(),
        rows,
        bucket_counts,
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
        return Err(
            AtmError::agent_not_found(&target.agent, &target.team).with_recovery(
                "Repair or reload the ATM roster, or list a different mailbox target.",
            ),
        );
    }

    Ok(())
}

fn apply_list_filters(
    messages: Vec<ClassifiedMessage>,
    sender_filter: Option<&AgentName>,
    timestamp_filter: Option<IsoTimestamp>,
    task_filter: Option<&TaskId>,
    contains_filter: Option<&str>,
) -> Vec<ClassifiedMessage> {
    let filtered = filters::apply_task_filter(
        filters::apply_timestamp_filter(
            filters::apply_sender_filter(messages, sender_filter),
            timestamp_filter,
        ),
        task_filter,
    );

    filters::apply_contains_filter(filtered, contains_filter)
}

fn list_row_from_message(message: &ClassifiedMessage) -> ListRow {
    ListRow {
        message_id: message.envelope.message_id,
        summary: message
            .envelope
            .summary
            .as_deref()
            .unwrap_or_default()
            .to_string(),
        from: message.envelope.from.clone(),
        timestamp: message.envelope.timestamp,
        read: message.envelope.read,
        pending_ack: message.envelope.pending_ack_at.is_some()
            && message.envelope.acknowledged_at.is_none(),
        task_id: message.envelope.task_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use serde_json::Map;
    use tempfile::tempdir;

    use super::{
        ListQuery, apply_list_filters, list_mail_with_runtime_impl, logical_current_messages,
    };
    use crate::boundary::{self, ProjectionAppendMode, RosterHarness, RosterMemberKind};
    use crate::error::AtmError;
    use crate::observability::NullObservability;
    use crate::read::ClassifiedMessage;
    use crate::schema::{AtmMessageId, InboxMessage, ThreadMode};
    use crate::service_runtime::{RetainedMailboxTimeoutPolicy, RetainedServiceRuntime};
    use crate::service_runtime_store::RetainedMailboxRuntime;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{
        AgentName, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection, TaskId, TeamName,
    };
    use crate::workflow::WorkflowStateFile;

    fn classified_message(
        text: &str,
        message_id: AtmMessageId,
        parent_message_id: Option<AtmMessageId>,
        thread_mode: Option<ThreadMode>,
        source_index: usize,
    ) -> ClassifiedMessage {
        ClassifiedMessage {
            source_index: source_index.into(),
            source_path: PathBuf::from("recipient.json"),
            bucket: DisplayBucket::Unread,
            class: MessageClass::Unread,
            envelope: InboxMessage {
                from: TEST_SENDER.parse::<AgentName>().expect("agent"),
                text: text.to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
                summary: None,
                message_id: Some(message_id),
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id,
                thread_mode,
                expires_at: None,
                task_id: None::<TaskId>,
                extra: Map::new(),
            },
        }
    }

    #[test]
    fn logical_current_messages_preserves_parent_context_for_add_details() {
        let root_id = AtmMessageId::new();
        let detail_id = AtmMessageId::new();
        let current = logical_current_messages(vec![
            classified_message("root context", root_id, None, None, 0),
            classified_message(
                "follow-up detail",
                detail_id,
                Some(root_id),
                Some(ThreadMode::AddDetails),
                1,
            ),
        ]);

        assert_eq!(current.len(), 1);
        assert_eq!(current[0].envelope.message_id, Some(detail_id));
        assert_eq!(current[0].envelope.text, "root context\n\nfollow-up detail");
    }

    #[test]
    fn list_contains_filter_drops_superseded_parent_context() {
        let root_id = AtmMessageId::new();
        let supersede_id = AtmMessageId::new();
        let current = logical_current_messages(vec![
            classified_message("root context", root_id, None, None, 0),
            classified_message(
                "replacement instruction",
                supersede_id,
                Some(root_id),
                Some(ThreadMode::Supersede),
                1,
            ),
        ]);

        let filtered = apply_list_filters(current, None, None, None, Some("root context"));

        assert!(filtered.is_empty());
    }

    struct ListRuntime {
        team_dir: PathBuf,
        roster_present: bool,
    }

    impl crate::boundary::sealed::Sealed for ListRuntime {}

    impl RetainedServiceRuntime for ListRuntime {
        fn load_config(
            &self,
            _current_dir: &Path,
        ) -> Result<Option<crate::config::AtmConfig>, AtmError> {
            Ok(None)
        }

        fn load_team_config_for_doctor_compare(
            &self,
            _team_dir: &Path,
        ) -> Result<crate::schema::TeamConfig, AtmError> {
            unreachable!("list roster-truth tests must not load team config")
        }

        fn team_dir(&self, _home_dir: &Path, _team: &TeamName) -> Result<PathBuf, AtmError> {
            Ok(self.team_dir.clone())
        }

        fn inbox_path(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<PathBuf, AtmError> {
            unreachable!("list roster-truth tests do not resolve inbox paths")
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<IsoTimestamp>, AtmError> {
            Ok(None)
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: IsoTimestamp,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn mailbox_timeout_policy(&self) -> RetainedMailboxTimeoutPolicy {
            RetainedMailboxTimeoutPolicy {
                workflow_lock_timeout: Duration::from_millis(1),
            }
        }

        fn rebuild_compat_inbox_projection(
            &self,
            _inbox_path: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<(), AtmError> {
            unreachable!("list roster-truth tests do not rebuild projections")
        }

        fn append_compat_inbox_message(
            &self,
            _inbox_path: &Path,
            _message: &InboxMessage,
        ) -> Result<(), AtmError> {
            unreachable!("list roster-truth tests do not append compat inbox messages")
        }

        fn append_compat_inbox_message_set(
            &self,
            _inbox_path: &Path,
            _mode: ProjectionAppendMode,
            _messages: &[InboxMessage],
        ) -> Result<(), AtmError> {
            unreachable!("list roster-truth tests do not append compat inbox message sets")
        }

        fn deliver_non_claude_payloads(
            &self,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            _messages: &[InboxMessage],
        ) -> Result<(), AtmError> {
            unreachable!("list roster-truth tests do not route outbound payloads")
        }

        fn load_roster_member(
            &self,
            team: &TeamName,
            agent: &AgentName,
        ) -> Result<Option<boundary::RosterEntry>, AtmError> {
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
        ) -> Result<Vec<boundary::RosterEntry>, AtmError> {
            Ok(Vec::new())
        }

        fn commit_workflow_state<T, I, F>(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _extra_write_paths: I,
            _timeout: Duration,
            _body: F,
        ) -> Result<T, AtmError>
        where
            I: IntoIterator<Item = PathBuf>,
            F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), AtmError>,
        {
            unreachable!("list roster-truth tests do not commit workflow state")
        }
    }

    impl RetainedMailboxRuntime for ListRuntime {
        fn query_mailbox_metadata_rows(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _limit: Option<usize>,
        ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
            Ok(Vec::new())
        }

        fn load_message_record(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _message_key: &boundary::MessageKey,
        ) -> Result<Option<boundary::Message>, AtmError> {
            unreachable!("list roster-truth tests do not load message records")
        }

        fn persist_message_record(&self, _record: boundary::Message) -> Result<(), AtmError> {
            unreachable!("list roster-truth tests do not persist message records")
        }

        fn persist_message_state(
            &self,
            _state: boundary::MailMessageState,
        ) -> Result<(), AtmError> {
            unreachable!("list roster-truth tests do not persist message state")
        }
    }

    fn list_query(home_dir: PathBuf, current_dir: PathBuf) -> ListQuery {
        let target = format!("recipient@{TEST_TEAM}");
        ListQuery::new(
            home_dir,
            current_dir,
            TEST_SENDER.parse().expect("caller"),
            Some(&target),
            TEST_TEAM.parse().expect("team"),
            ReadSelection::All,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("list query")
    }

    #[test]
    fn list_mail_uses_atm_roster_truth_for_explicit_targets() {
        let tempdir = tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        std::fs::create_dir_all(&team_dir).expect("team dir");
        let runtime = ListRuntime {
            team_dir,
            roster_present: true,
        };

        let outcome = list_mail_with_runtime_impl(
            list_query(tempdir.path().to_path_buf(), tempdir.path().to_path_buf()),
            &NullObservability,
            &runtime,
        )
        .expect("list outcome");

        assert_eq!(outcome.team, TeamName::from_validated(TEST_TEAM));
        assert_eq!(outcome.agent, AgentName::from_validated("recipient"));
        assert_eq!(outcome.count, 0);
    }

    #[test]
    fn list_mail_rejects_explicit_targets_missing_from_atm_roster() {
        let tempdir = tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        std::fs::create_dir_all(&team_dir).expect("team dir");
        let runtime = ListRuntime {
            team_dir,
            roster_present: false,
        };

        let error = list_mail_with_runtime_impl(
            list_query(tempdir.path().to_path_buf(), tempdir.path().to_path_buf()),
            &NullObservability,
            &runtime,
        )
        .expect_err("missing ATM roster member should fail");

        assert!(error.is_agent_not_found(), "{error:?}");
    }
}
