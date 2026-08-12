use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::address::AgentAddress;
use crate::boundary;
use crate::error::AtmError;
use crate::mailbox::source::resolve_target;
use crate::observability::ObservabilityPort;
use crate::read::{
    BucketCounts, ClassifiedMessage, filters,
    metadata_selection::{
        bucket_counts_for, classify_mailbox_metadata_rows,
        filter_metadata_backed_contains_candidates, logical_current_messages, select_messages,
        sort_and_limit_selected,
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
    /// Replaces caller-supplied filesystem roots with the daemon-owned root
    /// before a request crosses the long-lived service boundary.
    #[must_use]
    pub fn with_daemon_paths(mut self, daemon_home: PathBuf) -> Self {
        self.home_dir = daemon_home.clone();
        self.current_dir = daemon_home;
        self
    }

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
        Some(0) => Err(AtmError::validation(
            "list limit must be at least 1".to_string(),
        )),
        Some(value) if value > MAX_LIST_LIMIT => Err(AtmError::validation(format!(
            "list limit exceeds the {} row maximum",
            MAX_LIST_LIMIT
        ))),
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
    let contains_needle = query.contains_filter.as_deref();
    let config = runtime.load_config(&query.current_dir)?;
    let actor = query.caller_identity.clone();
    let target = resolve_target(
        query.target_address.as_ref(),
        &actor,
        &query.caller_team,
        config.as_ref(),
    )?;
    validate_target_member_in_roster(runtime, &target)?;

    let seen_watermark = if query.seen_state_filter && query.selection_mode != ReadSelection::All {
        runtime.load_seen_watermark(&query.home_dir, &target.team, &target.agent)?
    } else {
        None
    };

    let storage_bucket_counts = runtime.query_mailbox_bucket_counts(&target.team, &target.agent)?;
    let metadata_limit = metadata_limit_for_list(&query, storage_bucket_counts.is_some());
    let metadata_rows = runtime.query_mailbox_metadata_rows(
        &query.home_dir,
        &target.team,
        &target.agent,
        metadata_limit,
    )?;
    let classified_all = classify_mailbox_metadata_rows(&metadata_rows);
    let logical_current = logical_current_messages(classified_all);
    let bucket_counts = storage_bucket_counts
        .map(|counts| BucketCounts {
            unread: counts.unread,
            pending_ack: counts.pending_ack,
            history: counts.history,
        })
        .unwrap_or_else(|| bucket_counts_for(&logical_current));
    let filtered = apply_list_filters(
        logical_current,
        query.sender_filter.as_ref(),
        query.timestamp_filter,
        query.task_filter.as_ref(),
    );
    let selected = select_messages(&filtered, query.selection_mode, seen_watermark);
    let mut selected = filter_metadata_backed_contains_candidates(
        runtime,
        &query.home_dir,
        &target.team,
        &target.agent,
        &metadata_rows,
        selected,
        contains_needle,
    )?;
    sort_and_limit_selected(&mut selected, query.limit);

    for message in &mut selected {
        let key = boundary::MessageKey::new(message.source_path.to_string_lossy().into_owned())?;
        message.envelope =
            runtime.render_message_body(&key, message.envelope.message_id, &message.envelope)?;
    }

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

fn metadata_limit_for_list(query: &ListQuery, storage_counts_available: bool) -> Option<usize> {
    // The normal list view has no metadata predicates. Its bounded display
    // window requires a backend aggregate; otherwise fallback counting needs
    // full metadata. Predicate/contains views always retain full candidates.
    storage_counts_available
        .then(|| {
            (query.sender_filter.is_none()
                && query.timestamp_filter.is_none()
                && query.task_filter.is_none()
                && query.contains_filter.is_none())
            .then_some(query.limit)
            .flatten()
        })
        .flatten()
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

fn apply_list_filters(
    messages: Vec<ClassifiedMessage>,
    sender_filter: Option<&AgentName>,
    timestamp_filter: Option<IsoTimestamp>,
    task_filter: Option<&TaskId>,
) -> Vec<ClassifiedMessage> {
    filters::apply_task_filter(
        filters::apply_timestamp_filter(
            filters::apply_sender_filter(messages, sender_filter),
            timestamp_filter,
        ),
        task_filter,
    )
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
        pending_ack: matches!(
            atm_storage::derive_ack_requirement(&message.envelope),
            atm_storage::AckRequirementState::RequiredPending
        ),
        task_id: message.envelope.task_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use serde_json::Map;
    use tempfile::tempdir;

    use super::{
        ListQuery, apply_list_filters, list_mail_with_runtime_impl, logical_current_messages,
    };
    use crate::boundary::{self, MessageKey, RosterHarness, RosterMemberKind};
    use crate::error::AtmError;
    use crate::observability::NullObservability;
    use crate::read::ClassifiedMessage;
    use crate::read::filters;
    use crate::schema::{AtmMessageId, InboxMessage, ThreadMode};
    use crate::service_runtime::RetainedServiceRuntime;
    use crate::service_runtime_store::RetainedMailboxRuntime;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{
        AgentName, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection, TaskId, TeamName,
    };

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
                source_chat_id: None,
                text: text.to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
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

        let filtered = filters::apply_contains_filter(
            apply_list_filters(current, None, None, None),
            Some("root context"),
        );

        assert!(filtered.is_empty());
    }

    struct ListRuntime {
        roster_present: bool,
        metadata_rows: TestMetadataRows,
        message_records: HashMap<MessageKey, boundary::Message>,
        load_message_record_count: Arc<AtomicUsize>,
        metadata_query_limits: Arc<Mutex<Vec<Option<usize>>>>,
    }

    enum TestMetadataRows {
        Static(Vec<boundary::MailStoreMailboxMetadataRow>),
        Generated(usize),
    }

    impl TestMetadataRows {
        fn query(&self, limit: Option<usize>) -> Vec<boundary::MailStoreMailboxMetadataRow> {
            match self {
                Self::Static(rows) => rows
                    .iter()
                    .take(limit.unwrap_or(usize::MAX))
                    .cloned()
                    .collect(),
                Self::Generated(count) => (0..limit.unwrap_or(*count).min(*count))
                    .map(generated_metadata_row)
                    .collect(),
            }
        }
    }

    impl crate::boundary::sealed::Sealed for ListRuntime {}

    impl RetainedServiceRuntime for ListRuntime {
        fn load_config(
            &self,
            _current_dir: &Path,
        ) -> Result<Option<crate::config::AtmConfig>, AtmError> {
            Ok(None)
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
    }

    impl RetainedMailboxRuntime for ListRuntime {
        fn acknowledge_message_atomically(
            &self,
            _source: &atm_storage::contract::AcknowledgementSource,
            _builder: std::sync::Arc<dyn atm_storage::contract::AcknowledgementReplyBuilder>,
        ) -> Result<atm_storage::contract::AcknowledgementCommit, AtmError> {
            unreachable!("list roster-truth tests do not admit acknowledgements")
        }

        fn query_mailbox_metadata_rows(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            limit: Option<usize>,
        ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
            self.metadata_query_limits
                .lock()
                .expect("metadata query limits lock")
                .push(limit);
            Ok(self.metadata_rows.query(limit))
        }

        fn load_message_record(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            message_key: &boundary::MessageKey,
        ) -> Result<Option<boundary::Message>, AtmError> {
            self.load_message_record_count
                .fetch_add(1, Ordering::SeqCst);
            Ok(self.message_records.get(message_key).cloned())
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
            extra: Map::new(),
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

    const LARGE_MAILBOX_ROW_COUNT: usize = 500_000;

    fn generated_metadata_row(index: usize) -> boundary::MailStoreMailboxMetadataRow {
        let pending_ack =
            (LARGE_MAILBOX_ROW_COUNT - 4..LARGE_MAILBOX_ROW_COUNT - 2).contains(&index);
        let read = index >= LARGE_MAILBOX_ROW_COUNT - 2;
        boundary::MailStoreMailboxMetadataRow {
            message_key: MessageKey::new(format!("atm:scale-{index}")).expect("message key"),
            message_id: None,
            parent_message_id: None,
            thread_mode: None,
            from_agent: AgentName::from_validated(TEST_SENDER),
            source_chat_id: None,
            destination_chat_id: None,
            summary: None,
            message_at: IsoTimestamp::now(),
            read,
            requires_ack: pending_ack,
            pending_ack,
            acknowledged_at: None,
            expires_at: None,
            task_id: None,
        }
    }

    fn runtime_with_messages(
        _tempdir: &tempfile::TempDir,
        rows_and_messages: Vec<(boundary::MailStoreMailboxMetadataRow, boundary::Message)>,
        roster_present: bool,
    ) -> (ListRuntime, Arc<AtomicUsize>) {
        let load_message_record_count = Arc::new(AtomicUsize::new(0));
        let (metadata_rows, message_records): (Vec<_>, Vec<_>) = rows_and_messages
            .into_iter()
            .map(|(row, message)| {
                let key = row.message_key.clone();
                (row, (key, message))
            })
            .unzip();
        (
            ListRuntime {
                roster_present,
                metadata_rows: TestMetadataRows::Static(metadata_rows),
                message_records: message_records.into_iter().collect(),
                load_message_record_count: load_message_record_count.clone(),
                metadata_query_limits: Arc::new(Mutex::new(Vec::new())),
            },
            load_message_record_count,
        )
    }

    #[test]
    fn list_mail_uses_atm_roster_truth_for_explicit_targets() {
        let tempdir = tempdir().expect("tempdir");
        let runtime = ListRuntime {
            roster_present: true,
            metadata_rows: TestMetadataRows::Static(Vec::new()),
            message_records: HashMap::new(),
            load_message_record_count: Arc::new(AtomicUsize::new(0)),
            metadata_query_limits: Arc::new(Mutex::new(Vec::new())),
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
        let runtime = ListRuntime {
            roster_present: false,
            metadata_rows: TestMetadataRows::Static(Vec::new()),
            message_records: HashMap::new(),
            load_message_record_count: Arc::new(AtomicUsize::new(0)),
            metadata_query_limits: Arc::new(Mutex::new(Vec::new())),
        };

        let error = list_mail_with_runtime_impl(
            list_query(tempdir.path().to_path_buf(), tempdir.path().to_path_buf()),
            &NullObservability,
            &runtime,
        )
        .expect_err("missing ATM roster member should fail");

        assert!(
            error.code() == crate::error_codes::AtmErrorCode::AgentNotFound,
            "{error:?}"
        );
    }

    #[test]
    fn list_mail_counts_a_500k_mailbox_correctly_without_backend_aggregate() {
        let tempdir = tempdir().expect("tempdir");
        let query_limits = Arc::new(Mutex::new(Vec::new()));
        let runtime = ListRuntime {
            roster_present: true,
            metadata_rows: TestMetadataRows::Generated(LARGE_MAILBOX_ROW_COUNT),
            message_records: HashMap::new(),
            load_message_record_count: Arc::new(AtomicUsize::new(0)),
            metadata_query_limits: query_limits.clone(),
        };
        let target = format!("recipient@{TEST_TEAM}");
        let query = ListQuery::new(
            tempdir.path().to_path_buf(),
            tempdir.path().to_path_buf(),
            TEST_SENDER.parse().expect("caller"),
            Some(&target),
            TEST_TEAM.parse().expect("team"),
            ReadSelection::PendingAck,
            false,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("list query");

        let outcome = list_mail_with_runtime_impl(query, &NullObservability, &runtime)
            .expect("large fallback list outcome");

        assert_eq!(outcome.count, 1);
        assert_eq!(outcome.bucket_counts.unread, LARGE_MAILBOX_ROW_COUNT - 4);
        assert_eq!(outcome.bucket_counts.pending_ack, 2);
        assert_eq!(outcome.bucket_counts.history, 2);
        assert!(outcome.history_collapsed);
        assert_eq!(
            query_limits
                .lock()
                .expect("metadata query limits lock")
                .as_slice(),
            &[None],
            "the no-aggregate fallback must read full metadata before counting"
        );
    }

    #[test]
    fn metadata_backed_contains_fetches_durable_body_only_for_surviving_summary_miss_rows() {
        let tempdir = tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        std::fs::create_dir_all(&team_dir).expect("team dir");
        let (summary_row, summary_message) = metadata_row(
            "durable body with needle",
            Some("summary needle"),
            TEST_SENDER,
        );
        let (body_row, body_message) = metadata_row(
            "durable body with needle",
            Some("summary miss"),
            TEST_SENDER,
        );
        let (rejected_row, rejected_message) = metadata_row(
            "durable body with needle",
            Some("summary miss"),
            "other-sender",
        );
        let (runtime, load_count) = runtime_with_messages(
            &tempdir,
            vec![
                (summary_row, summary_message),
                (body_row, body_message),
                (rejected_row, rejected_message),
            ],
            true,
        );
        let outcome = list_mail_with_runtime_impl(
            ListQuery::new(
                tempdir.path().to_path_buf(),
                tempdir.path().to_path_buf(),
                TEST_SENDER.parse().expect("caller"),
                Some(&format!("recipient@{TEST_TEAM}")),
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
                false,
                None,
                Some(TEST_SENDER),
                None,
                None,
                Some("needle"),
            )
            .expect("list query"),
            &NullObservability,
            &runtime,
        )
        .expect("list outcome");

        assert_eq!(outcome.count, 2);
        assert_eq!(load_count.load(Ordering::SeqCst), 1);
    }
}
