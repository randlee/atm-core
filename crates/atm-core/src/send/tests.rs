use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Map, Value};
use tempfile::tempdir;

use super::{
    DeliveryPersistenceDisposition, ResolvedRecipient, SendExecutionContext, WarningEntry,
    alert_state, build_send_delivery_plan, persist_message, prepare_threaded_message,
};
use crate::boundary::{
    BuiltInPostSendDispatch, GraftNudgeTarget, MailMessageState, MailStoreMailboxMetadataRow,
    Message, MessageKey, NonClaudeOutboundDeliveryRequest, PostSendBuiltInTarget,
    PostSendEmissionPath, PostSendHookEmitter, RosterEntry, RosterHarness, RosterMemberKind,
};
use crate::config::AtmConfig;
use crate::delivery_execution::{DeliveryExecutionDisposition, execute_delivery_plan};
use crate::delivery_policy::{DeliveryEventFamily, DeliveryHarnessPath, DeliveryRecipientSnapshot};
use crate::error::{AtmError, AtmErrorKind};
use crate::error_codes::AtmErrorCode;
use crate::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState, CommandEvent,
    LogTailSession, ObservabilityPort,
};
use crate::process::process_is_alive;
use crate::protocol::NotificationEvent;
use crate::roles::ROLE_TEAM_LEAD;
use crate::schema::{AckIntentFields, AtmMessageId, InboxMessage, ThreadMode};
use crate::send::{SendCommandOutcome, SendMessageSource, SendRequest};
use crate::service_runtime::RetainedServiceRuntime;
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::test_support::{EnvGuard, TEST_SENDER, TEST_TEAM};
use crate::types::{AgentName, IsoTimestamp, TeamName};

fn message(
    from: &str,
    message_id: AtmMessageId,
    parent_message_id: Option<AtmMessageId>,
    thread_mode: Option<ThreadMode>,
) -> InboxMessage {
    let ack_intent = AckIntentFields::not_required();
    InboxMessage {
        from: from.parse::<AgentName>().expect("agent"),
        text: "hello".to_string(),
        timestamp: IsoTimestamp::now(),
        read: false,
        source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
        summary: None,
        message_id: Some(message_id),
        requires_ack: ack_intent.requires_ack,
        pending_ack_at: ack_intent.pending_ack_at,
        acknowledged_at: ack_intent.acknowledged_at,
        acknowledges_message_id: None,
        parent_message_id,
        thread_mode,
        expires_at: None,
        task_id: None,
        extra: Map::new(),
    }
}

pub(super) fn notification_detail(event: &NotificationEvent) -> Value {
    serde_json::from_str(&event.detail).expect("structured notification detail")
}

pub(super) fn read_notification_events(home_dir: &Path) -> Vec<NotificationEvent> {
    fs::read_to_string(
        crate::home::host_runtime_dir_from_home(home_dir).join("notifications.jsonl"),
    )
    .expect("notifications")
    .lines()
    .map(|line| serde_json::from_str(line).expect("notification event"))
    .collect()
}

pub(super) fn install_home_env(home_dir: &Path) -> EnvGuard {
    EnvGuard::set_many([
        ("HOME", Some(home_dir.to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
        ("ATM_LOG_DIR", None),
    ])
}

fn assert_recovered_payload_texts(
    original: &InboxMessage,
    companion: &InboxMessage,
    expected_original: &str,
) {
    assert_eq!(original.text, expected_original);
    assert_eq!(companion.from.as_str(), "atm-system");
    assert!(
        companion
            .text
            .contains("ATM error: SQLite persistence failed"),
        "expected sqlite failure companion message, got: {}",
        companion.text
    );
}

pub(super) struct TestRuntime {
    commit_error_message: Option<&'static str>,
    recipient_harness: DeliveryHarnessPath,
    claude_roster_members: Vec<AgentName>,
    roster_member_missing: bool,
    pub(super) appended_messages: Mutex<Vec<InboxMessage>>,
    pub(super) non_claude_deliveries: Mutex<Vec<NonClaudeOutboundDeliveryRequest>>,
}

pub(super) struct RecordingPostSendEmitter {
    fail_code: Option<AtmErrorCode>,
    emitted: Mutex<Vec<BuiltInPostSendDispatch>>,
}

impl RecordingPostSendEmitter {
    pub(super) fn succeed() -> Self {
        Self {
            fail_code: None,
            emitted: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn fail(code: AtmErrorCode) -> Self {
        Self {
            fail_code: Some(code),
            emitted: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn emitted(&self) -> Vec<BuiltInPostSendDispatch> {
        self.emitted.lock().expect("post-send emitter lock").clone()
    }
}

impl TestRuntime {
    pub(super) fn new(
        commit_error_message: Option<&'static str>,
        recipient_harness: DeliveryHarnessPath,
    ) -> Self {
        Self {
            commit_error_message,
            recipient_harness,
            claude_roster_members: vec![AgentName::from_validated("recipient")],
            roster_member_missing: false,
            appended_messages: Mutex::new(Vec::new()),
            non_claude_deliveries: Mutex::new(Vec::new()),
        }
    }
}

impl crate::boundary::sealed::Sealed for TestRuntime {}
impl crate::boundary::sealed::Sealed for RecordingPostSendEmitter {}

impl PostSendHookEmitter for RecordingPostSendEmitter {
    fn emit_post_send(
        &self,
        dispatch: &BuiltInPostSendDispatch,
    ) -> Result<PostSendEmissionPath, AtmError> {
        self.emitted
            .lock()
            .expect("post-send emitter lock")
            .push(dispatch.clone());
        if let Some(code) = self.fail_code {
            return Err(AtmError::new_with_code(
                code,
                AtmErrorKind::DaemonUnavailable,
                "test post-send emitter failure",
            )
            .with_recovery("Repair the test post-send emitter and retry."));
        }
        Ok(match dispatch.target {
            PostSendBuiltInTarget::LocalTmux(_) => PostSendEmissionPath::LocalTmux,
            PostSendBuiltInTarget::Graft(_) => PostSendEmissionPath::GraftPort,
        })
    }
}

impl RetainedServiceRuntime for TestRuntime {
    fn load_config(&self, _current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
        Ok(None)
    }

    fn load_team_config_for_doctor_compare(
        &self,
        _team_dir: &Path,
    ) -> Result<crate::schema::TeamConfig, AtmError> {
        Ok(crate::schema::TeamConfig::default())
    }

    fn team_dir(&self, home_dir: &Path, _team: &TeamName) -> Result<PathBuf, AtmError> {
        Ok(home_dir.to_path_buf())
    }

    fn inbox_path(
        &self,
        home_dir: &Path,
        _team: &TeamName,
        _agent: &AgentName,
    ) -> Result<PathBuf, AtmError> {
        Ok(home_dir.join("inbox.jsonl"))
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

    fn rebuild_compat_inbox_projection(
        &self,
        _inbox_path: &Path,
        _team: &TeamName,
        _agent: &AgentName,
    ) -> Result<(), AtmError> {
        Ok(())
    }

    fn deliver_non_claude_payloads(
        &self,
        recipient: &DeliveryRecipientSnapshot,
        messages: &[InboxMessage],
    ) -> Result<(), AtmError> {
        self.non_claude_deliveries
            .lock()
            .expect("non-claude deliveries lock")
            .push(NonClaudeOutboundDeliveryRequest {
                team: recipient.team.clone(),
                agent: recipient.agent.clone(),
                recipient_pane_id: recipient.recipient_pane_id.clone(),
                messages: messages.to_vec(),
            });
        Ok(())
    }

    fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::boundary::RosterEntry>, AtmError> {
        if self.roster_member_missing {
            return Ok(None);
        }
        Ok(Some(RosterEntry {
            team_name: team.clone(),
            agent_name: agent.clone(),
            member_kind: RosterMemberKind::Permanent,
            harness: match self.recipient_harness {
                DeliveryHarnessPath::ClaudeCode => RosterHarness::ClaudeCode,
                DeliveryHarnessPath::NonClaude => RosterHarness::CodexCli,
            },
            agent_type: crate::schema::AgentType::default(),
            model: crate::types::ModelName::default(),
            recipient_pane_id: None,
            metadata_json: Map::new(),
        }))
    }

    fn load_team_roster(
        &self,
        team: &TeamName,
    ) -> Result<Vec<crate::boundary::RosterEntry>, AtmError> {
        if self.roster_member_missing {
            return Ok(Vec::new());
        }
        Ok(vec![RosterEntry {
            team_name: team.clone(),
            agent_name: AgentName::from_validated("recipient"),
            member_kind: RosterMemberKind::Permanent,
            harness: match self.recipient_harness {
                DeliveryHarnessPath::ClaudeCode => RosterHarness::ClaudeCode,
                DeliveryHarnessPath::NonClaude => RosterHarness::CodexCli,
            },
            agent_type: crate::schema::AgentType::default(),
            model: crate::types::ModelName::default(),
            recipient_pane_id: None,
            metadata_json: Map::new(),
        }])
    }

    fn load_claude_code_team_roster(
        &self,
        team: &TeamName,
    ) -> Result<crate::boundary::ProjectionRoster, AtmError> {
        let records = self
            .claude_roster_members
            .iter()
            .cloned()
            .map(|agent_name| RosterEntry {
                team_name: team.clone(),
                agent_name,
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: crate::schema::AgentType::default(),
                model: crate::types::ModelName::default(),
                recipient_pane_id: None,
                metadata_json: Map::new(),
            })
            .collect::<Vec<_>>();
        Ok(crate::boundary::ProjectionRoster::from_roster_snapshot(
            team.clone(),
            &records,
        ))
    }
}

impl RetainedMailboxRuntime for TestRuntime {
    fn query_mailbox_metadata_rows(
        &self,
        _home_dir: &Path,
        _team: &TeamName,
        _agent: &AgentName,
        _limit: Option<usize>,
    ) -> Result<Vec<MailStoreMailboxMetadataRow>, AtmError> {
        Ok(Vec::new())
    }

    fn load_message_record(
        &self,
        _home_dir: &Path,
        _team: &TeamName,
        _agent: &AgentName,
        _message_key: &MessageKey,
    ) -> Result<Option<Message>, AtmError> {
        Ok(None)
    }

    fn persist_message_record(&self, _record: Message) -> Result<(), AtmError> {
        if let Some(message) = self.commit_error_message {
            return Err(AtmError::mailbox_write(message));
        }
        Ok(())
    }

    fn persist_message_state(&self, _state: MailMessageState) -> Result<(), AtmError> {
        Ok(())
    }
}

fn delivery_snapshot(harness: DeliveryHarnessPath) -> DeliveryRecipientSnapshot {
    DeliveryRecipientSnapshot {
        agent: AgentName::from_validated("recipient"),
        team: TeamName::from_validated(TEST_TEAM),
        harness,
        recipient_pane_id: None,
        local_tmux_post_send: false,
        graft_post_send: false,
        roster_backed: true,
    }
}

fn outbound_message() -> InboxMessage {
    let ack_intent = AckIntentFields::not_required();
    InboxMessage {
        from: AgentName::from_validated(TEST_SENDER),
        text: "hello".to_string(),
        timestamp: IsoTimestamp::now(),
        read: false,
        source_team: Some(TeamName::from_validated(TEST_TEAM)),
        summary: Some("hello".to_string()),
        message_id: Some(AtmMessageId::new()),
        requires_ack: ack_intent.requires_ack,
        pending_ack_at: ack_intent.pending_ack_at,
        acknowledged_at: ack_intent.acknowledged_at,
        acknowledges_message_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: Some("task-123".parse().expect("task id")),
        extra: Map::new(),
    }
}

pub(super) fn send_request(home_dir: &Path) -> SendRequest {
    SendRequest {
        home_dir: home_dir.to_path_buf(),
        current_dir: home_dir.to_path_buf(),
        caller_identity: AgentName::from_validated(TEST_SENDER),
        caller_team: TeamName::from_validated(TEST_TEAM),
        to: format!("recipient@{TEST_TEAM}").parse().expect("address"),
        message_source: SendMessageSource::Inline("hello".to_string()),
        summary_override: Some("hello".to_string()),
        requires_ack: false,
        task_id: Some("task-123".parse().expect("task id")),
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        dry_run: false,
    }
}

fn self_addressed_send_request(home_dir: &Path) -> SendRequest {
    let mut request = send_request(home_dir);
    request.to = format!("{TEST_SENDER}@{TEST_TEAM}")
        .parse()
        .expect("self address");
    request
}

fn send_runtime_with_missing_claude_member() -> TestRuntime {
    let mut runtime = TestRuntime::new(None, DeliveryHarnessPath::ClaudeCode);
    runtime.claude_roster_members.clear();
    runtime
}

fn send_runtime_with_missing_atm_roster_member() -> TestRuntime {
    let mut runtime = TestRuntime::new(None, DeliveryHarnessPath::ClaudeCode);
    runtime.roster_member_missing = true;
    runtime
}

#[derive(Default)]
struct RecordingObservability {
    // Mutex required: tests may emit delivery-policy events from multiple
    // runtime calls before assertions snapshot the collected sequence.
    events: Mutex<Vec<CommandEvent>>,
}

impl crate::boundary::sealed::Sealed for RecordingObservability {}

impl ObservabilityPort for RecordingObservability {
    fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
        self.events.lock().expect("events lock").push(event);
        Ok(())
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Ok(AtmLogSnapshot::default())
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Ok(LogTailSession::empty())
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        Ok(AtmObservabilityHealth {
            active_log_path: None,
            logging_state: AtmObservabilityHealthState::Unavailable,
            query_state: Some(AtmObservabilityHealthState::Unavailable),
            maintenance: None,
            diagnostic: None,
            detail: Some("test observer".to_string()),
        })
    }
}

#[test]
fn sqlite_failure_for_claude_preserves_original_and_companion_error_payloads() {
    let outbound = outbound_message();
    let runtime = TestRuntime::new(Some("sqlite write failed"), DeliveryHarnessPath::ClaudeCode);
    let tempdir = tempdir().expect("tempdir");
    let inbox_path = tempdir.path().join("recipient.jsonl");

    let result = persist_message(
        &runtime,
        tempdir.path(),
        &delivery_snapshot(DeliveryHarnessPath::ClaudeCode),
        &inbox_path,
        &outbound,
        false,
    )
    .expect("sqlite fallback recovery");

    assert_eq!(
        result.disposition,
        DeliveryPersistenceDisposition::SqliteFailedRecovered
    );
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.original_message.from.as_str(), TEST_SENDER);
    assert_recovered_payload_texts(
        &result.original_message,
        result.companion_message.as_ref().expect("companion"),
        &outbound.text,
    );
}

#[test]
fn sqlite_failure_for_non_claude_preserves_original_and_companion_payloads() {
    let outbound = outbound_message();
    let runtime = TestRuntime::new(Some("sqlite write failed"), DeliveryHarnessPath::NonClaude);
    let tempdir = tempdir().expect("tempdir");
    let inbox_path = tempdir.path().join("recipient.jsonl");

    let result = persist_message(
        &runtime,
        tempdir.path(),
        &delivery_snapshot(DeliveryHarnessPath::NonClaude),
        &inbox_path,
        &outbound,
        false,
    )
    .expect("sqlite fallback recovery");

    assert_eq!(
        result.disposition,
        DeliveryPersistenceDisposition::SqliteFailedRecovered
    );
    assert_eq!(result.original_message.from.as_str(), TEST_SENDER);
    assert_recovered_payload_texts(
        &result.original_message,
        result.companion_message.as_ref().expect("companion"),
        &outbound.text,
    );
}

#[test]
#[serial_test::serial(env)]
fn claude_harness_delivery_no_longer_has_append_degradation_path() {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::ClaudeCode);
    let tempdir = tempdir().expect("tempdir");
    let context = SendExecutionContext {
        command_config: None,
        post_send_config: None,
        recipient: ResolvedRecipient {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated(TEST_TEAM),
        },
        canonical_sender: AgentName::from_validated(TEST_SENDER),
        inbox_path: tempdir.path().join("recipient.jsonl"),
        delivery_snapshot: delivery_snapshot(DeliveryHarnessPath::ClaudeCode),
        delivery_family: DeliveryEventFamily::NewMessage,
        warnings: Vec::new(),
    };
    let persistence = crate::send::DeliveryPersistenceResult::persisted(outbound_message());
    let plan = build_send_delivery_plan(&context, false, &persistence).expect("plan");
    let execution = execute_delivery_plan(&runtime, None, &plan).expect("direct delivery");

    assert_eq!(
        execution.disposition,
        DeliveryExecutionDisposition::Delivered
    );
    assert!(execution.warnings.is_empty());
}

#[test]
fn named_plan_builder_proves_payload_equality_across_harnesses() {
    let tempdir = tempdir().expect("tempdir");
    let original = outbound_message();
    let ack_intent = AckIntentFields::not_required();
    let companion = InboxMessage {
        from: AgentName::from_validated("atm-system"),
        text: "sqlite failed".to_string(),
        timestamp: IsoTimestamp::now(),
        read: false,
        source_team: Some(TeamName::from_validated(TEST_TEAM)),
        summary: Some("sqlite failed".to_string()),
        message_id: Some(AtmMessageId::new()),
        requires_ack: ack_intent.requires_ack,
        pending_ack_at: ack_intent.pending_ack_at,
        acknowledged_at: ack_intent.acknowledged_at,
        acknowledges_message_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: original.task_id.clone(),
        extra: Map::new(),
    };
    let persistence = crate::send::DeliveryPersistenceResult::sqlite_failed_recovered(
        original.clone(),
        companion.clone(),
        WarningEntry::new("sqlite failed", Some("repair sqlite")),
    );
    let base_context = SendExecutionContext {
        command_config: None,
        post_send_config: None,
        recipient: ResolvedRecipient {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated(TEST_TEAM),
        },
        canonical_sender: AgentName::from_validated(TEST_SENDER),
        inbox_path: tempdir.path().join("recipient.jsonl"),
        delivery_snapshot: delivery_snapshot(DeliveryHarnessPath::ClaudeCode),
        delivery_family: DeliveryEventFamily::NewMessage,
        warnings: Vec::new(),
    };
    let claude_plan =
        build_send_delivery_plan(&base_context, false, &persistence).expect("claude plan");
    let non_claude_context = SendExecutionContext {
        delivery_snapshot: delivery_snapshot(DeliveryHarnessPath::NonClaude),
        ..base_context
    };
    let non_claude_plan = build_send_delivery_plan(&non_claude_context, false, &persistence)
        .expect("non-claude plan");

    assert_eq!(claude_plan.messages, non_claude_plan.messages);
    assert!(matches!(
        claude_plan.delivery_target,
        crate::delivery_plan::DeliveryTarget::NonClaude { .. }
    ));
    assert!(matches!(
        non_claude_plan.delivery_target,
        crate::delivery_plan::DeliveryTarget::NonClaude { .. }
    ));
}

#[test]
#[serial_test::serial(env)]
fn recovered_claude_harness_sqlite_failure_routes_via_non_claude_outbound() {
    let runtime = TestRuntime::new(Some("sqlite write failed"), DeliveryHarnessPath::ClaudeCode);
    let observability = RecordingObservability::default();
    let tempdir = tempdir().expect("tempdir");
    let home_dir = tempdir.path().join("home");
    let _env = install_home_env(&home_dir);

    let outcome = super::send_mail_with_runtime_impl(
        send_request(tempdir.path()),
        &observability,
        &runtime,
        None,
    )
    .expect("send outcome");

    assert_eq!(outcome.outcome, SendCommandOutcome::Sent);
    assert_eq!(outcome.warnings.len(), 1);
    assert_non_claude_sqlite_failure_delivery(&runtime);
    assert_notification_log_absent(&home_dir);
    assert_non_claude_sqlite_failure_observability(&observability);
}

fn assert_non_claude_sqlite_failure_delivery(runtime: &TestRuntime) {
    assert!(
        runtime
            .appended_messages
            .lock()
            .expect("append lock")
            .is_empty()
    );
    let deliveries = runtime
        .non_claude_deliveries
        .lock()
        .expect("non-claude deliveries lock");
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].team.as_str(), TEST_TEAM);
    assert_eq!(deliveries[0].agent.as_str(), "recipient");
    assert_eq!(deliveries[0].messages.len(), 2);
    assert_recovered_payload_texts(
        &deliveries[0].messages[0],
        &deliveries[0].messages[1],
        "hello",
    );
}

fn assert_notification_log_absent(home_dir: &Path) {
    let notification_path =
        crate::home::host_runtime_dir_from_home(home_dir).join("notifications.jsonl");
    assert!(
        !notification_path.exists(),
        "notification log should stay absent when no post-send emitter succeeds"
    );
}

fn assert_non_claude_sqlite_failure_observability(observability: &RecordingObservability) {
    let observability_events = observability.events.lock().expect("events lock");
    assert!(observability_events.iter().any(|event| {
        event.command == "delivery_policy"
            && event.outcome.as_str() == "delivery_policy.new_message.non_claude_original"
    }));
    assert!(observability_events.iter().any(|event| {
        event.command == "delivery_policy"
            && event.outcome.as_str() == "delivery_policy.new_message.non_claude_error"
    }));
}

#[test]
#[serial_test::serial(env)]
fn send_non_claude_sqlite_failure_delivers_original_and_error_via_outbound_boundary() {
    let runtime = TestRuntime::new(Some("sqlite write failed"), DeliveryHarnessPath::NonClaude);
    let observability = RecordingObservability::default();
    let tempdir = tempdir().expect("tempdir");
    let home_dir = tempdir.path().join("home");
    let _env = install_home_env(&home_dir);
    let post_send_emitter = RecordingPostSendEmitter::succeed();

    let outcome = super::send_mail_with_runtime_impl(
        send_request(tempdir.path()),
        &observability,
        &runtime,
        Some(&post_send_emitter),
    )
    .expect("send outcome");

    assert_eq!(outcome.outcome, SendCommandOutcome::Sent);
    assert_eq!(outcome.warnings.len(), 1);
    assert_non_claude_sqlite_failure_delivery(&runtime);
    let events = read_notification_events(&home_dir);
    assert_eq!(events.len(), 1);
    assert_non_claude_sqlite_failure_observability(&observability);
    let emitted = post_send_emitter.emitted();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].event.sender.as_str(), TEST_SENDER);
    assert!(!emitted[0].event.is_ack);
    match &emitted[0].target {
        PostSendBuiltInTarget::Graft(GraftNudgeTarget {
            recipient,
            recipient_team,
        }) => {
            assert_eq!(recipient.as_str(), "recipient");
            assert_eq!(recipient_team.as_str(), TEST_TEAM);
        }
        other => panic!("expected graft post-send target, got {other:?}"),
    }
}

#[test]
#[serial_test::serial(env)]
fn send_claude_harness_success_delivers_original_via_outbound_boundary() {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::ClaudeCode);
    let observability = RecordingObservability::default();
    let tempdir = tempdir().expect("tempdir");
    let home_dir = tempdir.path().join("home");
    let _env = install_home_env(&home_dir);

    let outcome = super::send_mail_with_runtime_impl(
        send_request(tempdir.path()),
        &observability,
        &runtime,
        None,
    )
    .expect("send outcome");

    assert_eq!(outcome.outcome, SendCommandOutcome::Sent);
    assert!(outcome.warnings.is_empty());
    assert!(
        runtime
            .appended_messages
            .lock()
            .expect("append lock")
            .is_empty()
    );
    let deliveries = runtime
        .non_claude_deliveries
        .lock()
        .expect("non-claude deliveries lock");
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].messages.len(), 1);
    assert_eq!(deliveries[0].messages[0].from.as_str(), TEST_SENDER);
    drop(deliveries);
    assert_notification_log_absent(&home_dir);

    let events = observability.events.lock().expect("events lock");
    assert!(events.iter().any(|event| {
        event.command == "delivery_policy"
            && event.outcome.as_str() == "delivery_policy.new_message.non_claude_original"
    }));
}

#[test]
#[serial_test::serial(env)]
fn z6_post_write_warning_uses_store_backed_claude_roster() {
    let runtime = send_runtime_with_missing_claude_member();
    let observability = RecordingObservability::default();
    let tempdir = tempdir().expect("tempdir");
    fs::write(tempdir.path().join("config.json"), r#"{"members":[]}"#).expect("config");

    let outcome = super::send_mail_with_runtime_impl(
        send_request(tempdir.path()),
        &observability,
        &runtime,
        None,
    )
    .expect("send outcome");

    assert_eq!(outcome.outcome, SendCommandOutcome::Sent);
    assert_eq!(
        runtime
            .non_claude_deliveries
            .lock()
            .expect("non-claude deliveries lock")
            .len(),
        1
    );
    assert_eq!(outcome.warnings.len(), 1);
    assert!(
        outcome.warnings[0]
            .message
            .contains("'recipient' is not on claude code roster")
    );
}

#[test]
#[serial_test::serial(env)]
fn z11_empty_atm_roster_failure_is_actionable_without_fallback() {
    let runtime = send_runtime_with_missing_atm_roster_member();
    let observability = RecordingObservability::default();
    let tempdir = tempdir().expect("tempdir");

    let error = super::send_mail_with_runtime_impl(
        send_request(tempdir.path()),
        &observability,
        &runtime,
        None,
    )
    .expect_err("empty atm roster must fail");

    assert!(error.is_agent_not_found());
    assert_eq!(
        error.message,
        format!("agent 'recipient' was not found in team '{TEST_TEAM}'")
    );
    assert_eq!(
        error.primary_recovery(),
        Some("Update the team membership or target a different recipient.")
    );
    assert!(
        runtime
            .appended_messages
            .lock()
            .expect("append lock")
            .is_empty()
    );
    assert!(
        runtime
            .non_claude_deliveries
            .lock()
            .expect("non-claude deliveries lock")
            .is_empty()
    );
}

#[test]
#[serial_test::serial(env)]
fn self_addressed_plain_send_is_rejected_before_persistence() {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let observability = RecordingObservability::default();
    let tempdir = tempdir().expect("tempdir");
    let mut request = self_addressed_send_request(tempdir.path());
    request.task_id = None;
    request.summary_override = None;

    let error = super::send_mail_with_runtime_impl(request, &observability, &runtime, None)
        .expect_err("self-addressed send must fail");

    assert!(error.is_validation());
    assert_eq!(error.code, AtmErrorCode::SelfAddressedSendInvalid);
    assert!(
        error
            .message
            .contains("self-addressed messages are invalid ATM input")
    );
    assert!(
        runtime
            .appended_messages
            .lock()
            .expect("append lock")
            .is_empty()
    );
    assert!(
        runtime
            .non_claude_deliveries
            .lock()
            .expect("non-claude deliveries lock")
            .is_empty()
    );
}

#[test]
#[serial_test::serial(env)]
fn self_addressed_task_send_is_rejected_before_persistence() {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let observability = RecordingObservability::default();
    let tempdir = tempdir().expect("tempdir");

    let error = super::send_mail_with_runtime_impl(
        self_addressed_send_request(tempdir.path()),
        &observability,
        &runtime,
        None,
    )
    .expect_err("self-addressed task send must fail");

    assert!(error.is_validation());
    assert_eq!(error.code, AtmErrorCode::SelfAddressedSendInvalid);
    assert!(
        runtime
            .appended_messages
            .lock()
            .expect("append lock")
            .is_empty()
    );
    assert!(
        runtime
            .non_claude_deliveries
            .lock()
            .expect("non-claude deliveries lock")
            .is_empty()
    );
}

#[test]
#[serial_test::serial(env)]
fn self_addressed_dry_run_is_rejected_before_reporting_success() {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let observability = RecordingObservability::default();
    let tempdir = tempdir().expect("tempdir");
    let mut request = self_addressed_send_request(tempdir.path());
    request.to = format!(
        "{}@{}",
        TEST_SENDER.to_ascii_uppercase(),
        TEST_TEAM.to_ascii_uppercase()
    )
    .parse()
    .expect("case-variant self address");
    request.dry_run = true;

    let error = super::send_mail_with_runtime_impl(request, &observability, &runtime, None)
        .expect_err("self-addressed dry-run must fail");

    assert!(error.is_validation());
    assert_eq!(error.code, AtmErrorCode::SelfAddressedSendInvalid);
    assert!(
        runtime
            .appended_messages
            .lock()
            .expect("append lock")
            .is_empty()
    );
    assert!(
        runtime
            .non_claude_deliveries
            .lock()
            .expect("non-claude deliveries lock")
            .is_empty()
    );
}

#[test]
fn save_send_alert_state_round_trips() {
    let tempdir = tempdir().expect("tempdir");
    let path = alert_state::state_path(tempdir.path());
    let mut state = alert_state::SendAlertState::default();
    state
        .missing_team_config_keys
        .insert(format!("teams/{TEST_TEAM}/config.json"));

    alert_state::save(&path, &state).expect("save");
    let loaded = alert_state::load(&path).expect("load");
    assert_eq!(
        loaded.missing_team_config_keys,
        state.missing_team_config_keys
    );
}

#[test]
fn process_is_alive_reports_current_process() {
    assert!(process_is_alive(std::process::id()));
}

#[test]
fn acquire_send_alert_lock_evicts_stale_pid_lock() {
    let tempdir = tempdir().expect("tempdir");
    let path = alert_state::lock_path(tempdir.path());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("lock dir");
    }
    fs::write(&path, u32::MAX.to_string()).expect("stale lock");

    let guard = alert_state::acquire_lock(&path).expect("acquire lock");
    let pid = fs::read_to_string(&path).expect("lock contents");
    assert_eq!(pid.trim(), std::process::id().to_string());
    drop(guard);
    assert!(!path.exists());
}

#[test]
fn send_request_new_rejects_invalid_recipient_before_command_execution() {
    let tempdir = tempdir().expect("tempdir");
    let error = SendRequest::new(
        tempdir.path().to_path_buf(),
        tempdir.path().to_path_buf(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "../evil",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("hello".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect_err("invalid address");

    assert!(error.message.contains("agent name"));
}

#[test]
fn send_request_new_rejects_invalid_caller_team_before_command_execution() {
    let error: AtmError = "../evil".parse::<TeamName>().expect_err("invalid team");

    assert!(error.message.contains("team name"));
}

#[test]
fn resolve_recipient_rejects_invalid_alias_target() {
    let mut aliases = std::collections::BTreeMap::new();
    aliases.insert("tl".to_string(), "../bad-agent".to_string());
    let config = AtmConfig {
        aliases,
        ..Default::default()
    };

    let error = super::resolve_recipient(
        &"tl".parse().expect("address"),
        &TEST_TEAM.parse().expect("team"),
        Some(&config),
    )
    .expect_err("invalid alias target");

    assert!(error.is_address());
}

#[test]
fn prepare_threaded_message_reopens_ack_for_ack_required_thread() {
    let root_id = AtmMessageId::new();
    let mut root = message(TEST_SENDER, root_id, None, None);
    root.requires_ack = true;
    root.acknowledged_at = Some(IsoTimestamp::now());
    let mut update = message(
        TEST_SENDER,
        AtmMessageId::new(),
        Some(root_id),
        Some(ThreadMode::AddDetails),
    );

    prepare_threaded_message(&mut update, &[root]).expect("prepare update");

    assert!(update.pending_ack_at.is_some());
    assert!(update.acknowledged_at.is_none());
}

#[test]
fn prepare_threaded_message_reopens_ack_for_ack_required_supersede_thread() {
    let root_id = AtmMessageId::new();
    let mut root = message(TEST_SENDER, root_id, None, None);
    root.requires_ack = true;
    root.acknowledged_at = Some(IsoTimestamp::now());
    let mut update = message(
        TEST_SENDER,
        AtmMessageId::new(),
        Some(root_id),
        Some(ThreadMode::Supersede),
    );

    prepare_threaded_message(&mut update, &[root]).expect("prepare update");

    assert!(update.pending_ack_at.is_some());
    assert!(update.acknowledged_at.is_none());
}

#[test]
fn prepare_threaded_message_rejects_non_originating_sender() {
    let root_id = AtmMessageId::new();
    let root = message(TEST_SENDER, root_id, None, None);
    let mut update = message(
        ROLE_TEAM_LEAD,
        AtmMessageId::new(),
        Some(root_id),
        Some(ThreadMode::Supersede),
    );

    let error = prepare_threaded_message(&mut update, &[root]).expect_err("different sender");

    assert!(error.message.contains("original sender"));
}
