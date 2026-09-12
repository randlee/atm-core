use super::*;
use atm_core::boundary::TaskStore;
use crate::{
    BareCliFifo, BareCliQueueFullDrops, RuntimeHealth, append_bare_cli_message,
    drain_bare_cli_messages,
};
use std::str::FromStr;
use tracing::instrument::WithSubscriber;

#[derive(Clone, Default)]
struct WarningLayer {
    events: Arc<Mutex<Vec<(String, String)>>>,
}

#[derive(Default)]
struct WarningFields {
    action: Option<String>,
    outcome: Option<String>,
}

impl tracing::field::Visit for WarningFields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "action" => self.action = Some(value.to_owned()),
            "outcome" => self.outcome = Some(value.to_owned()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let value = format!("{value:?}").trim_matches('"').to_owned();
        match field.name() {
            "action" => self.action = Some(value),
            "outcome" => self.outcome = Some(value),
            _ => {}
        }
    }
}

impl<S> tracing_subscriber::Layer<S> for WarningLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if *event.metadata().level() != tracing::Level::WARN {
            return;
        }
        let mut fields = WarningFields::default();
        event.record(&mut fields);
        self.events.lock().expect("warning events").push((
            fields.action.unwrap_or_default(),
            fields.outcome.unwrap_or_default(),
        ));
    }
}

struct BackendSelector {
    emitter: BackendEmitter,
}

struct BackendEmitter {
    paths: Arc<Mutex<Vec<&'static str>>>,
    fifo: BareCliFifo,
    drops: BareCliQueueFullDrops,
}

impl atm_core::boundary::sealed::Sealed for BackendSelector {}
impl atm_core::boundary::sealed::Sealed for BackendEmitter {}

impl MessageReceivedHookSelector for BackendSelector {
    fn select_emitter(
        &self,
        _dispatch: &BuiltInPostSendDispatch,
    ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
        Some(&self.emitter)
    }
}

impl AsyncMessageReceivedHookEmitter for BackendEmitter {
    fn emit_received_message(
        &self,
        dispatch: BuiltInPostSendDispatch,
        _deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>> {
        let result = match dispatch.target {
            atm_core::boundary::PostSendBuiltInTarget::LocalSteer(
                atm_core::boundary::LocalSteerTarget::Tmux(_),
            ) => {
                self.paths.lock().expect("paths").push("tmux");
                Ok(PostSendEmissionPath::LocalTmux)
            }
            atm_core::boundary::PostSendBuiltInTarget::QueuePull(target) => {
                let member = atm_storage::MemberKey::new(target.team, target.agent);
                let message = atm_core::protocol::QueuedNudgeMessage {
                    kind: target.kind,
                    msg_id: target.msg_id,
                    body: target.body,
                };
                append_bare_cli_message(&self.fifo, &self.drops, member, message).map(|()| {
                    self.paths.lock().expect("paths").push("fifo");
                    PostSendEmissionPath::QueuePull
                })
            }
            _ => Err(AtmError::new(
                AtmErrorCode::InternalError,
                "backend fixture received an unexpected dispatch",
            )),
        };
        Box::pin(async move { result })
    }
}

async fn daemon_mail_for(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &str,
) -> Vec<atm_storage::Message> {
    runtime
        .async_mailbox_reader()
        .expect("mailbox reader")
        .list_messages(
            atm_storage::MailboxScope::new(
                team.clone(),
                agent.parse().expect("mailbox agent"),
            ),
            atm_storage::MessageQuery {
                team: team.clone(),
                agent: agent.parse().expect("query agent"),
                sender: Some("atm-daemon".parse().expect("daemon")),
                task_id: None,
                limit: None,
            },
            atm_storage::ReadDeadline::new(std::time::Duration::from_secs(1))
                .expect("deadline"),
        )
        .await
        .expect("read daemon mail")
}

async fn daemon_stalled_mail_for(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &str,
) -> Vec<atm_storage::Message> {
    daemon_mail_for(runtime, team, agent)
        .await
        .into_iter()
        .filter(|message| {
            message
                .envelope
                .summary
                .as_deref()
                .is_some_and(|summary| summary.starts_with("escalation:lead_notified:"))
        })
        .collect()
}

async fn task_assignment_message_id(
    runtime: &LocalServiceRuntime,
    member: &atm_storage::MemberKey,
    task_id: &TaskId,
) -> AtmMessageId {
    let messages = runtime
        .async_mailbox_reader()
        .expect("mailbox reader")
        .list_messages(
            atm_storage::MailboxScope::new(member.team().clone(), member.agent().clone()),
            atm_storage::MessageQuery {
                team: member.team().clone(),
                agent: member.agent().clone(),
                sender: Some("sender".parse().expect("assigner")),
                task_id: Some(task_id.clone()),
                limit: None,
            },
            atm_storage::ReadDeadline::new(std::time::Duration::from_secs(1))
                .expect("deadline"),
        )
        .await
        .expect("read task assignment");
    let [message] = messages.as_slice() else {
        panic!("expected exactly one task assignment, got {}", messages.len());
    };
    message
        .message_key
        .as_atm_message_id()
        .expect("assignment message id")
}

fn bare_member(team: &TeamName, agent: &str) -> RosterEntry {
    let mut member = herdr_member(team, agent);
    member.metadata_json.clear();
    member
}

fn install_escalation_targets(
    runtime: &LocalServiceRuntime,
    store: &atm_storage::DummyTaskStore,
    members: &[atm_storage::MemberKey],
    recipients: &[&str],
) {
    let team = members[0].team();
    let mut lead = bare_member(team, atm_storage::roles::ROLE_TEAM_LEAD);
    lead.agent_type = atm_storage::AgentType::Lead;
    let mut roster_members = runtime
        .shared_roster_store_arc()
        .load_roster(team)
        .expect("existing roster")
        .members;
    roster_members.push(lead);
    roster_members.extend(recipients.iter().map(|recipient| {
        let address: atm_core::address::AgentAddress = recipient.parse().expect("recipient");
        bare_member(team, address.agent().as_str())
    }));
    runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: roster_members,
            refreshed_at: None,
        })
        .expect("roster with escalation lead");
    for recipient in recipients {
        let recipient = recipient
            .parse::<atm_core::address::AgentAddress>()
            .expect("recipient address");
        store
            .add_escalation_recipient(
                &atm_storage::EscalationScope::Team(team.clone()),
                &recipient,
                IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("timestamp"),
            )
            .expect("escalation recipient");
    }
}

type RealTaskPumpFixture = (
    tempfile::TempDir,
    LocalServiceRuntime,
    Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    HerdrQueueWakePump,
    atm_storage::MemberKey,
    Vec<TaskId>,
    Arc<Mutex<IsoTimestamp>>,
);

fn build_real_task_pump(task_names: &[&str]) -> RealTaskPumpFixture {
    let root = tempfile::tempdir().expect("temporary root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "lifecycle-team".parse().expect("team");
    let key = atm_storage::MemberKey::new(team.clone(), "worker".parse().expect("agent"));
    let mut lead = bare_member(&team, atm_storage::roles::ROLE_TEAM_LEAD);
    lead.agent_type = atm_storage::AgentType::Lead;
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: vec![herdr_member(&team, "worker"), bare_member(&team, "sender"), lead],
            refreshed_at: None,
        })
        .expect("lifecycle roster");
    let tasks: Vec<TaskId> = task_names
        .iter()
        .map(|name| name.parse().expect("task id"))
        .collect();
    for task in &tasks {
        let message_id = queue_task_message(
            root.path(),
            &assembly.service_runtime,
            &team,
            key.agent().as_str(),
            task.clone(),
        );
        acknowledge_task_assignment(
            root.path(),
            &assembly.service_runtime,
            &key,
            message_id,
        );
    }
    clear_pending_markers(root.path(), &assembly.service_runtime, &key);
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    queue_idle_result(&fake, &key);
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("timestamp"),
    ));
    let pump = pump_with_clock(
        assembly.service_runtime.clone(),
        fake.clone(),
        RuntimeHealth::default(),
        now.clone(),
    )
    .with_daemon_home(root.path().join("home"));
    (
        root,
        assembly.service_runtime,
        fake,
        pump,
        key,
        tasks,
        now,
    )
}

fn acknowledge_task_assignment(
    root: &std::path::Path,
    runtime: &LocalServiceRuntime,
    member: &atm_storage::MemberKey,
    message_id: AtmMessageId,
) {
    let home = root.join("home");
    ack_mail_with_runtime(
        AckRequest {
            home_dir: home.clone(),
            current_dir: home,
            caller_identity: member.agent().clone(),
            caller_chat_id: None,
            caller_team: member.team().clone(),
            activity_observation: None,
            message_id,
            reply_body: "assignment received by fixture".to_owned(),
        },
        &NullObservability,
        runtime,
    )
    .expect("acknowledge task assignment");
}

fn close_real_task(
    root: &std::path::Path,
    runtime: &LocalServiceRuntime,
    member: &atm_storage::MemberKey,
    task: &TaskId,
    outcome: atm_storage::TaskCloseOutcome,
) {
    let home = root.join("home");
    let mut request = WriteRequest::new(
        home.clone(),
        home,
        "sender".parse().expect("sender"),
        &member.to_string(),
        member.team().clone(),
        SendMessageSource::Inline("task closed by lifecycle fixture".to_owned()),
        None,
        false,
        Some(task.clone()),
        false,
    )
    .expect("close request")
    .with_nudge_mode(NudgeMode::Deferred);
    request.task_op = Some(atm_storage::TaskOp::Close {
        outcome,
        reason: None,
    });
    write_mail_with_runtime(request, &NullObservability, runtime).expect("close task");
    clear_pending_markers(root, runtime, member);
}

#[tokio::test]
async fn idle_member_with_queued_task_is_nudged_once_per_interval() {
    let (_root, _runtime, fake, pump, _store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 1);
    *now.lock().expect("clock") = IsoTimestamp::from_str("2030-01-01T00:00:30Z").expect("timestamp");
    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 1);
    *now.lock().expect("clock") = IsoTimestamp::from_str("2030-01-01T00:01:01Z").expect("timestamp");
    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 2);
}

#[tokio::test]
async fn active_member_is_never_prompted() {
    let (_root, runtime, fake, pump, _store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Working], false);
    for second in 0..200 {
        *now.lock().expect("clock") = IsoTimestamp::from_str(&format!(
            "2030-01-01T00:{:02}:{:02}Z",
            (second / 60) % 60,
            second % 60
        ))
        .expect("timestamp");
        if second > 0 {
            queue_status_result(&fake, &keys, HerdrAgentStatus::Working);
        }
        pump.tick_once().await;
    }
    assert!(prompt_texts(&fake).is_empty(), "active members are never prompted");
    assert_eq!(pump.stats().task_reminders, 0);
    assert_eq!(pump.stats().blocked_escalations, 0);
    assert!(
        daemon_mail_for(&runtime, keys[0].team(), keys[0].agent().as_str())
            .await
            .is_empty(),
        "the Herdr-backed active member receives no mail"
    );

    let root = tempfile::tempdir().expect("temporary root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "active-tmux".parse().expect("team");
    let agent = "tmux-worker";
    let mut member = herdr_member(&team, agent);
    member.metadata_json.clear();
    member.recipient_pane_id = Some(atm_core::types::PaneId::from_cli("%17").expect("pane"));
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: vec![member],
            refreshed_at: None,
        })
        .expect("tmux roster");
    let key = atm_storage::MemberKey::new(team.clone(), agent.parse().expect("agent"));
    let rows = task_rows(
        &team,
        &[agent.to_owned()],
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("timestamp"),
    );
    let store = Arc::new(atm_storage::DummyTaskStore::with_rows(rows, false));
    let reader: Arc<dyn atm_core::boundary::AsyncTaskLedgerReader + Send + Sync> = store.clone();
    let writer: Arc<dyn atm_core::boundary::TaskStore + Send + Sync> = store;
    let runtime = assembly
        .service_runtime
        .with_async_task_ledger_reader(reader)
        .with_task_store(writer);
    runtime.apply_roster_runtime_observations(
        &team,
        &[atm_core::protocol::RosterRuntimeObservationUpdate::observed(
            key.agent().clone(),
            RuntimeMemberState::Active,
            atm_core::protocol::RuntimeObservationSource::Heartbeat,
            IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("timestamp"),
            None,
        )],
    );
    let tmux_fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    let clock = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("timestamp"),
    ));
    let tmux_pump = pump_with_clock(
        runtime.clone(),
        tmux_fake.clone(),
        RuntimeHealth::default(),
        clock,
    );
    for _ in 0..200 {
        tmux_pump.tick_once().await;
    }
    assert!(prompt_texts(&tmux_fake).is_empty());
    assert_eq!(tmux_pump.stats().task_reminders, 0);
    assert!(daemon_mail_for(&runtime, &team, agent).await.is_empty());
}

#[tokio::test]
async fn poll_failure_produces_no_episode() {
    let (_root, runtime, _seeded_fake, _seeded_pump, _store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    let pump = pump_with_clock(
        runtime.clone(),
        fake.clone(),
        RuntimeHealth::default(),
        now,
    );
    let roster_before = runtime
        .shared_roster_store_arc()
        .load_roster(keys[0].team())
        .expect("roster before failures");
    for _ in 0..20 {
        fake.queue_list_result(Err(atm_herdr::HerdrError::AgentNotReady));
        pump.tick_once().await;
    }
    assert_eq!(pump.stats().blocked_escalations, 0);
    assert!(prompt_texts(&fake).is_empty());
    assert!(
        daemon_mail_for(&runtime, keys[0].team(), keys[0].agent().as_str())
            .await
            .is_empty()
    );
    assert_eq!(
        runtime
            .shared_roster_store_arc()
            .load_roster(keys[0].team())
            .expect("roster after failures"),
        roster_before,
        "poll failures do not mutate the durable roster"
    );
}

#[tokio::test]
async fn breaker_open_produces_no_escalation_mail() {
    let (_root, runtime, fake, pump, _store, keys, _now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    fake.queue_prompt_result(Err(atm_herdr::HerdrError::ServerUnavailable {
        message: String::new(),
        retry_after: None,
        io_error_kind: None,
    }));
    pump.tick_once().await;
    assert_eq!(pump.stats().breaker_open, 1);
    assert_eq!(pump.stats().blocked_escalations, 0);
    assert!(
        daemon_mail_for(&runtime, keys[0].team(), keys[0].agent().as_str())
            .await
            .is_empty(),
        "opening the Herdr breaker cannot synthesize escalation mail"
    );
}

#[tokio::test]
async fn escalation_mail_does_not_consume_prompt_budget() {
    let mut statuses = vec![HerdrAgentStatus::Idle; 16];
    statuses.push(HerdrAgentStatus::Blocked);
    let (_root, runtime, fake, pump, store, keys, _now) =
        build_task_only_pump(statuses, false);
    install_escalation_targets(
        &runtime,
        store.as_ref(),
        &keys,
        &["observer@ax5-task-only"],
    );
    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 16);
    assert_eq!(pump.stats().task_reminders, 16);
    assert_eq!(pump.stats().blocked_escalations, 1);
    assert_eq!(
        daemon_mail_for(
            &runtime,
            keys[16].team(),
            atm_storage::roles::ROLE_TEAM_LEAD,
        )
            .await
            .len(),
        1
    );
    assert_eq!(
        daemon_mail_for(&runtime, keys[16].team(), "observer")
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn member_turning_active_between_dispose_and_emit_is_not_prompted() {
    let (_root, runtime, _seeded_fake, _seeded_pump, _store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Working], false);
    let inner = runtime
        .async_task_ledger_reader()
        .expect("task ledger reader");
    let hook_runtime = runtime.clone();
    let hook_key = keys[0].clone();
    let hooked = Arc::new(
        atm_storage::testing::InMemoryTaskLedgerReader::delegating_with_open_tasks_hook(
        inner,
        move || {
            hook_runtime.apply_roster_runtime_observations(
                hook_key.team(),
                &[atm_core::protocol::RosterRuntimeObservationUpdate::observed(
                    hook_key.agent().clone(),
                    RuntimeMemberState::Active,
                    atm_core::protocol::RuntimeObservationSource::Heartbeat,
                    IsoTimestamp::from_str("2030-01-01T00:00:01Z").expect("timestamp"),
                    None,
                )],
            );
        },
    ));
    let hooked_runtime = runtime.with_async_task_ledger_reader(hooked);
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    let pump = pump_with_clock(
        hooked_runtime.clone(),
        fake.clone(),
        RuntimeHealth::default(),
        now,
    );
    pump.tick_once().await;
    assert!(prompt_texts(&fake).is_empty());
    assert_eq!(pump.stats().task_reminders, 0);
    assert_eq!(
        hooked_runtime
            .roster_ephemeral_state(keys[0].team(), keys[0].agent())
            .expect("runtime state")
            .runtime
            .state,
        RuntimeMemberState::Active
    );
}

#[tokio::test]
async fn blocked_member_gets_one_message_zero_nudges_per_episode() {
    let (_root, runtime, fake, pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Blocked], false);
    install_escalation_targets(
        &runtime,
        store.as_ref(),
        &keys,
        &["observer@ax5-task-only"],
    );
    for second in 0..50 {
        *now.lock().expect("clock") = IsoTimestamp::from_str(&format!(
            "2030-01-01T00:00:{second:02}Z"
        ))
        .expect("timestamp");
        pump.tick_once().await;
        if second < 49 {
            queue_status_result(&fake, &keys, HerdrAgentStatus::Blocked);
        }
    }
    assert!(prompt_texts(&fake).is_empty(), "blocked members receive no task nudges");
    assert_eq!(pump.stats().task_reminders, 0);
    assert_eq!(
        daemon_mail_for(
            &runtime,
            keys[0].team(),
            atm_storage::roles::ROLE_TEAM_LEAD,
        )
        .await
        .len(),
        1
    );
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "observer").await.len(), 1);

    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("timestamp");
    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:01:01Z").expect("timestamp");
    queue_status_result(&fake, &keys, HerdrAgentStatus::Blocked);
    pump.tick_once().await;
    assert_eq!(
        daemon_mail_for(
            &runtime,
            keys[0].team(),
            atm_storage::roles::ROLE_TEAM_LEAD,
        )
        .await
        .len(),
        2
    );
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "observer").await.len(), 2);
}

#[tokio::test]
async fn offline_member_gets_one_message_zero_nudges_per_episode() {
    let (root, runtime, _seeded_fake, _seeded_pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: keys[0].team().clone(),
            members: vec![bare_member(keys[0].team(), keys[0].agent().as_str())],
            refreshed_at: None,
        })
        .expect("bare CLI roster");
    install_escalation_targets(
        &runtime,
        store.as_ref(),
        &keys,
        &["observer@ax5-task-only"],
    );
    let router = crate::storage_and_nudge_router::StorageAndNudgeRouter::new(
        runtime.clone(),
        Arc::new(NullObservability),
        Arc::new(NoEmitterSelector),
        root.path().join("home"),
    );
    let observed_at = *now.lock().expect("clock");
    crate::CanonicalWriteHandler::dispatch(
        &router,
        atm_core::api::ApiRequest::new(atm_core::protocol::RequestEnvelope::Heartbeat(
            atm_core::protocol::TeamMemberHeartbeatRequest {
                team: keys[0].team().clone(),
                member: keys[0].agent().clone(),
                pid: 73,
                observed_at,
                activity: atm_core::protocol::HeartbeatActivity::SessionEnded,
                session_id: None,
            },
        )),
        atm_core::AuthenticatedIngress::Local,
        RequestDeadline::after(Duration::from_secs(1)),
    )
    .await
    .expect("offline heartbeat ingress");
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    let pump = pump_with_selector(
        runtime.clone(),
        fake.clone(),
        RuntimeHealth::default(),
        now,
        Arc::new(NoEmitterSelector),
    )
    .with_daemon_home(root.path().join("home"));
    for _ in 0..50 {
        pump.tick_once().await;
    }
    assert!(fake.calls().is_empty(), "the offline member has no Herdr backend");
    assert_eq!(pump.stats().task_reminders, 0);
    assert_eq!(
        daemon_mail_for(
            &runtime,
            keys[0].team(),
            atm_storage::roles::ROLE_TEAM_LEAD,
        )
        .await
        .len(),
        1
    );
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "observer").await.len(), 1);
}

#[tokio::test]
async fn sustained_listing_absence_becomes_offline_after_two_reminder_intervals_once() {
    let (_root, runtime, fake, pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    install_escalation_targets(
        &runtime,
        store.as_ref(),
        &keys,
        &["observer@ax5-task-only"],
    );

    pump.tick_once().await;
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:00:01Z").expect("timestamp");
    fake.queue_list_result(Ok(HerdrListOutcome { agents: Vec::new() }));
    pump.tick_once().await;
    assert_eq!(
        runtime
            .roster_ephemeral_state(keys[0].team(), keys[0].agent())
            .expect("first absent observation")
            .runtime
            .state,
        RuntimeMemberState::Unknown
    );
    assert!(
        daemon_mail_for(
            &runtime,
            keys[0].team(),
            atm_storage::roles::ROLE_TEAM_LEAD,
        )
        .await
        .is_empty()
    );

    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:02:00Z").expect("timestamp");
    fake.queue_list_result(Ok(HerdrListOutcome { agents: Vec::new() }));
    pump.tick_once().await;
    assert_eq!(
        runtime
            .roster_ephemeral_state(keys[0].team(), keys[0].agent())
            .expect("pre-threshold observation")
            .runtime
            .state,
        RuntimeMemberState::Unknown
    );

    for timestamp in ["2030-01-01T00:02:01Z", "2030-01-01T00:03:01Z"] {
        *now.lock().expect("clock") = IsoTimestamp::from_str(timestamp).expect("timestamp");
        fake.queue_list_result(Ok(HerdrListOutcome { agents: Vec::new() }));
        pump.tick_once().await;
    }

    let observation = runtime
        .roster_ephemeral_state(keys[0].team(), keys[0].agent())
        .expect("member observation")
        .runtime;
    assert_eq!(observation.state, RuntimeMemberState::Offline);
    assert_eq!(pump.stats().task_reminders, 0);
    assert_eq!(
        daemon_mail_for(
            &runtime,
            keys[0].team(),
            atm_storage::roles::ROLE_TEAM_LEAD,
        )
        .await
        .len(),
        1
    );
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "observer").await.len(), 1);
}

#[tokio::test]
async fn daemon_restart_does_not_reescalate_ongoing_episode() {
    let (root, runtime, fake, pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Blocked], false);
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2026-01-01T00:00:00Z").expect("timestamp");
    install_escalation_targets(
        &runtime,
        store.as_ref(),
        &keys,
        &["observer@ax5-task-only"],
    );
    pump.tick_once().await;
    assert_eq!(
        daemon_mail_for(
            &runtime,
            keys[0].team(),
            atm_storage::roles::ROLE_TEAM_LEAD,
        )
        .await
        .len(),
        1
    );
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "observer").await.len(), 1);

    let durable_reader = runtime.async_mailbox_reader().expect("durable reader");
    let counting_reader = Arc::new(atm_storage::testing::InMemoryMailboxReader::delegating(
        durable_reader.clone(),
    ));
    let restarted_runtime = runtime
        .clone()
        .with_async_mailbox_reader(counting_reader.clone());
    let restarted_fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    for _ in 0..50 {
        queue_status_result(&restarted_fake, &keys, HerdrAgentStatus::Blocked);
    }
    let restarted = pump_with_clock(
        restarted_runtime,
        restarted_fake.clone(),
        RuntimeHealth::default(),
        now,
    )
    .with_daemon_home(root.path().join("home"));
    for _ in 0..50 {
        restarted.tick_once().await;
    }
    assert!(prompt_texts(&fake).is_empty());
    assert!(prompt_texts(&restarted_fake).is_empty());
    assert_eq!(counting_reader.list_call_count(), 2, "one read per target at restart");
    assert_eq!(
        daemon_mail_for(
            &runtime,
            keys[0].team(),
            atm_storage::roles::ROLE_TEAM_LEAD,
        )
        .await
        .len(),
        1
    );
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "observer").await.len(), 1);
}

#[tokio::test]
async fn recovered_then_reblocked_episode_is_reported_again() {
    let (_root, runtime, fake, pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Blocked], false);
    let first_since = *now.lock().expect("clock");
    *now.lock().expect("clock") = first_since;
    install_escalation_targets(&runtime, store.as_ref(), &keys, &[]);
    pump.tick_once().await;
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:00:01Z").expect("timestamp");
    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;
    let second_since = IsoTimestamp::from_str("2030-01-01T00:00:02Z").expect("timestamp");
    *now.lock().expect("clock") = second_since;
    queue_status_result(&fake, &keys, HerdrAgentStatus::Blocked);
    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 1, "only the recovered idle tick may prompt");
    let mail = daemon_mail_for(
        &runtime,
        keys[0].team(),
        atm_storage::roles::ROLE_TEAM_LEAD,
    )
    .await;
    assert_eq!(mail.len(), 2);
    assert!(mail.iter().any(|message| message.envelope.text.contains(&first_since.to_string())));
    assert!(mail.iter().any(|message| message.envelope.text.contains(&second_since.to_string())));
}

#[tokio::test]
async fn recipients_only_episode_is_not_duplicated_after_restart() {
    let (root, runtime, _fake, pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Blocked], false);
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2026-01-01T00:00:00Z").expect("timestamp");
    let mut lead_a = bare_member(keys[0].team(), "lead-a");
    lead_a.agent_type = atm_storage::AgentType::Lead;
    let mut lead_b = bare_member(keys[0].team(), "lead-b");
    lead_b.agent_type = atm_storage::AgentType::Lead;
    runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: keys[0].team().clone(),
            members: vec![
                herdr_member(keys[0].team(), keys[0].agent().as_str()),
                lead_a,
                lead_b,
                bare_member(keys[0].team(), "observer"),
            ],
            refreshed_at: None,
        })
        .expect("ambiguous-lead roster");
    let observer = "observer@ax5-task-only"
        .parse::<atm_core::address::AgentAddress>()
        .expect("observer address");
    store
        .add_escalation_recipient(
            &atm_storage::EscalationScope::Team(keys[0].team().clone()),
            &observer,
            *now.lock().expect("clock"),
        )
        .expect("recipient");
    pump.tick_once().await;
    assert!(daemon_mail_for(&runtime, keys[0].team(), "lead-a").await.is_empty());
    assert!(daemon_mail_for(&runtime, keys[0].team(), "lead-b").await.is_empty());
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "observer").await.len(), 1);

    let durable_reader = runtime.async_mailbox_reader().expect("durable reader");
    let counting_reader = Arc::new(atm_storage::testing::InMemoryMailboxReader::delegating(
        durable_reader,
    ));
    let restarted_runtime = runtime
        .clone()
        .with_async_mailbox_reader(counting_reader.clone());
    let restarted_fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    queue_status_result(&restarted_fake, &keys, HerdrAgentStatus::Blocked);
    let restarted = pump_with_clock(
        restarted_runtime,
        restarted_fake,
        RuntimeHealth::default(),
        now,
    )
    .with_daemon_home(root.path().join("home"));
    restarted.tick_once().await;
    assert_eq!(counting_reader.list_call_count(), 1);
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "observer").await.len(), 1);
}

#[tokio::test]
async fn episode_message_summary_and_body() {
    let (_root, runtime, _fake, pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Blocked], false);
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2026-01-01T00:00:00Z").expect("timestamp");
    install_escalation_targets(&runtime, store.as_ref(), &keys, &[]);
    pump.tick_once().await;
    let mail = daemon_mail_for(
        &runtime,
        keys[0].team(),
        atm_storage::roles::ROLE_TEAM_LEAD,
    )
    .await;
    assert_eq!(mail.len(), 1);
    assert_eq!(
        mail[0].envelope.summary.as_deref(),
        Some("escalation:blocked_escalated:ax5-agent-00@ax5-task-only")
    );
    assert!(
        mail[0]
            .envelope
            .text
            .contains("since 2026-01-01T00:00:00+00:00")
    );
}

#[tokio::test]
async fn escalation_mail_is_immediate_and_never_carries_marker() {
    let (_root, runtime, _fake, pump, store, keys, _now) =
        build_task_only_pump(vec![HerdrAgentStatus::Blocked], false);
    install_escalation_targets(&runtime, store.as_ref(), &keys, &[]);

    pump.tick_once().await;

    let lead = atm_storage::MemberKey::new(
        keys[0].team().clone(),
        atm_storage::roles::ROLE_TEAM_LEAD
            .parse()
            .expect("lead agent"),
    );
    assert_eq!(
        daemon_mail_for(&runtime, keys[0].team(), lead.agent().as_str())
            .await
            .len(),
        1
    );
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .claim_next_pending(&lead)
            .expect("claim pending marker")
            .is_none(),
        "immediate escalation mail must not create a queue marker"
    );
}

#[tokio::test]
async fn tenth_reminder_escalates_once_then_silence() {
    let (_root, runtime, fake, pump, key, tasks, now) =
        build_real_task_pump(&["LIFE-TENTH"]);
    for minute in 0..10 {
        *now.lock().expect("clock") = IsoTimestamp::from_str(&format!(
            "2030-01-01T00:{minute:02}:00Z"
        ))
        .expect("timestamp");
        if minute > 0 {
            queue_idle_result(&fake, &key);
        }
        pump.tick_once().await;
    }
    queue_idle_result(&fake, &key);
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:10:00Z").expect("timestamp");
    pump.tick_once().await;
    let store = runtime.task_store().expect("task store");
    let row = store
        .load_task(key.team(), &tasks[0])
        .expect("task")
        .expect("row");
    assert_eq!(row.reminder_count, 10);
    assert_eq!(row.lead_notified_count, 1);
    assert_eq!(prompt_texts(&fake).len(), 10);
    assert_eq!(
        daemon_mail_for(&runtime, key.team(), atm_storage::roles::ROLE_TEAM_LEAD)
            .await
            .len(),
        1
    );

    for elapsed in 11..111 {
        let hour = elapsed / 60;
        let minute = elapsed % 60;
        *now.lock().expect("clock") = IsoTimestamp::from_str(&format!(
            "2030-01-01T{hour:02}:{minute:02}:00Z"
        ))
        .expect("timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
    }
    assert_eq!(prompt_texts(&fake).len(), 10);
    assert_eq!(
        daemon_mail_for(&runtime, key.team(), atm_storage::roles::ROLE_TEAM_LEAD)
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn escalation_recipient_read_error_holds_and_warns_once_per_tick() {
    let (_root, runtime, fake, pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    let task_id: TaskId = "AX5-TASK-00".parse().expect("task id");
    for minute in 0..10 {
        let at = IsoTimestamp::from_str(&format!("2030-01-01T00:{minute:02}:00Z"))
            .expect("timestamp");
        store
            .record_reminder(&keys[0], &task_id, at, atm_storage::ReminderOutcome::Emitted)
            .expect("seed reminder");
    }
    store.set_fail_escalation_recipient_reads(true);
    let warnings = WarningLayer::default();

    for tick in 0..2 {
        if tick > 0 {
            queue_idle_result(&fake, &keys[0]);
        }
        *now.lock().expect("clock") =
            IsoTimestamp::from_str(&format!("2030-01-01T00:{}:00Z", tick + 10))
                .expect("timestamp");
        pump.tick_once()
            .with_subscriber(tracing_subscriber::Registry::default().with(warnings.clone()))
            .await;
        let row = store.row(&keys[0], &task_id);
        assert_eq!(row.reminder_count, 10);
        assert_eq!(row.lead_notified_count, 0, "failed target reads are non-terminal");
        let target_warnings = warnings
            .events
            .lock()
            .expect("warning events")
            .iter()
            .filter(|(action, outcome)| action == "escalation_target_load" && outcome == "failed")
            .count();
        assert_eq!(target_warnings, tick + 1, "one target-read warning per tick");
    }
    assert!(prompt_texts(&fake).is_empty());
    assert_eq!(pump.stats().lead_notifications, 0);
    assert!(
        daemon_mail_for(&runtime, keys[0].team(), atm_storage::roles::ROLE_TEAM_LEAD)
            .await
            .is_empty()
    );
}

async fn drive_task_to_stall(
    pump: &HerdrQueueWakePump,
    fake: &Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    key: &atm_storage::MemberKey,
    now: &Arc<Mutex<IsoTimestamp>>,
) {
    for minute in 0..=10 {
        *now.lock().expect("clock") = IsoTimestamp::from_str(&format!(
            "2030-01-01T00:{minute:02}:00Z"
        ))
        .expect("timestamp");
        if minute > 0 {
            queue_idle_result(fake, key);
        }
        pump.tick_once().await;
    }
}

#[tokio::test]
async fn tenth_reminder_with_zero_leads_escalates_to_recipients_once_then_silence() {
    let (_root, runtime, fake, pump, key, tasks, now) =
        build_real_task_pump(&["LIFE-NO-LEAD"]);
    let mut roster = runtime
        .shared_roster_store_arc()
        .load_roster(key.team())
        .expect("roster");
    roster
        .members
        .retain(|member| member.agent_type != atm_storage::AgentType::Lead);
    roster.members.push(bare_member(key.team(), "observer"));
    runtime
        .shared_roster_store_arc()
        .save_roster(&roster)
        .expect("zero-lead roster");
    assert!(
        runtime
            .task_store()
            .expect("task store")
            .effective_escalation_recipients(key.team())
            .expect("escalation recipients")
            .is_empty()
    );
    runtime
        .task_store()
        .expect("task store")
        .add_escalation_recipient(
            &atm_storage::EscalationScope::Team(key.team().clone()),
            &"observer@lifecycle-team".parse().expect("recipient"),
            *now.lock().expect("clock"),
        )
        .expect("escalation recipient");

    drive_task_to_stall(&pump, &fake, &key, &now).await;
    assert_eq!(
        runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &tasks[0])
            .expect("task")
            .expect("row")
            .lead_notified_count,
        1
    );
    assert_eq!(daemon_mail_for(&runtime, key.team(), "observer").await.len(), 1);
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:11:00Z").expect("timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(daemon_mail_for(&runtime, key.team(), "observer").await.len(), 1);
}

#[tokio::test]
async fn tenth_reminder_with_no_targets_records_terminal_audit() {
    let (_root, runtime, fake, pump, key, tasks, now) =
        build_real_task_pump(&["LIFE-NO-TARGETS"]);
    let mut roster = runtime
        .shared_roster_store_arc()
        .load_roster(key.team())
        .expect("roster");
    roster
        .members
        .retain(|member| member.agent_type != atm_storage::AgentType::Lead);
    runtime
        .shared_roster_store_arc()
        .save_roster(&roster)
        .expect("zero-lead roster");
    let recipients = ["worker", "sender", atm_storage::roles::ROLE_TEAM_LEAD];

    drive_task_to_stall(&pump, &fake, &key, &now).await;
    assert_eq!(
        runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &tasks[0])
            .expect("task")
            .expect("row")
            .lead_notified_count,
        1
    );
    for recipient in recipients {
        assert!(
            daemon_stalled_mail_for(&runtime, key.team(), recipient)
                .await
                .is_empty(),
            "no escalation mail should be written to {recipient}"
        );
    }

    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:11:00Z").expect("timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 10);
    assert_eq!(
        runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &tasks[0])
            .expect("task")
            .expect("row")
            .lead_notified_count,
        1
    );
    for recipient in recipients {
        assert!(
            daemon_stalled_mail_for(&runtime, key.team(), recipient)
                .await
                .is_empty(),
            "the next tick should remain silent for {recipient}"
        );
    }
}

#[tokio::test]
async fn tenth_reminder_with_two_leads_escalates_to_each_once_then_silence() {
    let (_root, runtime, fake, pump, key, tasks, now) =
        build_real_task_pump(&["LIFE-TWO-LEADS"]);
    let mut roster = runtime
        .shared_roster_store_arc()
        .load_roster(key.team())
        .expect("roster");
    roster
        .members
        .retain(|member| member.agent_type != atm_storage::AgentType::Lead);
    for name in ["lead-a", "lead-b"] {
        let mut lead = bare_member(key.team(), name);
        lead.agent_type = atm_storage::AgentType::Lead;
        roster.members.push(lead);
    }
    runtime
        .shared_roster_store_arc()
        .save_roster(&roster)
        .expect("two-lead roster");

    drive_task_to_stall(&pump, &fake, &key, &now).await;
    assert_eq!(
        runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &tasks[0])
            .expect("task")
            .expect("row")
            .lead_notified_count,
        1
    );
    assert_eq!(daemon_mail_for(&runtime, key.team(), "lead-a").await.len(), 1);
    assert_eq!(daemon_mail_for(&runtime, key.team(), "lead-b").await.len(), 1);
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:11:00Z").expect("timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(daemon_mail_for(&runtime, key.team(), "lead-a").await.len(), 1);
    assert_eq!(daemon_mail_for(&runtime, key.team(), "lead-b").await.len(), 1);
}

#[tokio::test]
async fn close_of_stalled_task_resumes_nudging_on_next_task() {
    let (root, runtime, fake, pump, key, tasks, now) =
        build_real_task_pump(&["LIFE-FIRST", "LIFE-SECOND"]);
    for minute in 0..=10 {
        *now.lock().expect("clock") = IsoTimestamp::from_str(&format!(
            "2030-01-01T00:{minute:02}:00Z"
        ))
        .expect("timestamp");
        if minute > 0 {
            queue_idle_result(&fake, &key);
        }
        pump.tick_once().await;
    }
    close_real_task(
        root.path(),
        &runtime,
        &key,
        &tasks[0],
        atm_storage::TaskCloseOutcome::Completed,
    );
    let store = runtime.task_store().expect("task store");
    let next_before = store
        .load_task(key.team(), &tasks[1])
        .expect("next task")
        .expect("next row");
    assert_eq!(next_before.reminder_count, 0);
    assert_eq!(next_before.lead_notified_count, 0);
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:11:00Z").expect("timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    let next_after = store
        .load_task(key.team(), &tasks[1])
        .expect("next task")
        .expect("next row");
    assert_eq!(next_after.reminder_count, 1);
    assert_eq!(next_after.lead_notified_count, 0);
    assert_eq!(next_after.state, TaskState::Active);
    assert_eq!(prompt_texts(&fake).len(), 11);
}

#[tokio::test]
async fn reopen_of_stalled_task_escalates_again_at_threshold() {
    let (root, runtime, fake, pump, key, tasks, now) =
        build_real_task_pump(&["LIFE-REOPEN"]);
    for minute in 0..=10 {
        *now.lock().expect("clock") = IsoTimestamp::from_str(&format!(
            "2030-01-01T00:{minute:02}:00Z"
        ))
        .expect("timestamp");
        if minute > 0 {
            queue_idle_result(&fake, &key);
        }
        pump.tick_once().await;
    }
    assert_eq!(
        daemon_mail_for(&runtime, key.team(), atm_storage::roles::ROLE_TEAM_LEAD)
            .await
            .len(),
        1
    );
    close_real_task(
        root.path(),
        &runtime,
        &key,
        &tasks[0],
        atm_storage::TaskCloseOutcome::Completed,
    );
    let message_id = queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        tasks[0].clone(),
    );
    acknowledge_task_assignment(root.path(), &runtime, &key, message_id);
    let store = runtime.task_store().expect("task store");
    let reopened = store
        .load_task(key.team(), &tasks[0])
        .expect("reopened task")
        .expect("reopened row");
    assert_eq!(reopened.state, TaskState::Assigned);
    assert_eq!(reopened.reminder_count, 0);
    assert_eq!(reopened.lead_notified_count, 0);

    for minute in 20..=30 {
        *now.lock().expect("clock") = IsoTimestamp::from_str(&format!(
            "2030-01-01T00:{minute:02}:00Z"
        ))
        .expect("timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
    }
    let escalated_again = store
        .load_task(key.team(), &tasks[0])
        .expect("task")
        .expect("row");
    assert_eq!(escalated_again.reminder_count, 10);
    assert_eq!(escalated_again.lead_notified_count, 1);
    assert_eq!(
        daemon_mail_for(&runtime, key.team(), atm_storage::roles::ROLE_TEAM_LEAD)
            .await
            .len(),
        2
    );
}

#[tokio::test]
async fn handoff_applies_start_and_sends_receipt_to_assigner() {
    let (root, runtime, fake, pump, key, task, now) = build_task_handoff_pump();
    let message_id = task_assignment_message_id(&runtime, &key, &task).await;
    let team = key.team().clone();

    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 1, "the queued assignment wakes once");
    assert_eq!(
        runtime
            .task_store()
            .expect("store")
            .load_task(&team, &task)
            .expect("task")
            .expect("row")
            .state,
        TaskState::Active,
        "the successful queue-drain delivery completes the first task handoff"
    );
    acknowledge_task_assignment(root.path(), &runtime, &key, message_id);
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:01:01Z").expect("timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 2, "the due task reminder is emitted once");
    let store = runtime.task_store().expect("store");
    assert_eq!(
        store
            .load_task(&team, &task)
            .expect("task")
            .expect("row")
            .state,
        TaskState::Active
    );
    let events = store.list_task_events(&team, &task, None).expect("task events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event == atm_storage::TaskEventKind::Started)
            .count(),
        1,
        "the first emitted nudge starts the assigned task exactly once"
    );
    assert_eq!(
        events
            .iter()
            .find(|event| event.event == atm_storage::TaskEventKind::Started)
            .expect("started event")
            .actor,
        atm_storage::TaskActor::Daemon
    );
    let reader = runtime.async_mailbox_reader().expect("mailbox reader");
    let receipts = reader
        .list_messages(
            atm_storage::MailboxScope::new(team.clone(), "sender".parse().expect("assigner")),
            atm_storage::MessageQuery {
                team: team.clone(),
                agent: "sender".parse().expect("assigner"),
                sender: Some("atm-daemon".parse().expect("daemon")),
                task_id: Some(task.clone()),
                limit: None,
            },
            atm_storage::ReadDeadline::new(std::time::Duration::from_secs(1))
                .expect("deadline"),
        )
        .await
        .expect("read assigner mailbox");
    assert_eq!(receipts.len(), 1);
    assert_eq!(
        receipts[0].envelope.summary.as_deref(),
        Some("task_started:AX5-HANDOFF")
    );

    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:02:02Z").expect("timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        prompt_texts(&fake).len(),
        3,
        "a genuinely due active-task reminder is emitted"
    );
    let events_after = store
        .list_task_events(&team, &task, None)
        .expect("task events after retry");
    assert_eq!(
        events_after
            .iter()
            .filter(|event| event.event == atm_storage::TaskEventKind::Started)
            .count(),
        1
    );
    let receipts_after = reader
        .list_messages(
            atm_storage::MailboxScope::new(team.clone(), "sender".parse().expect("assigner")),
            atm_storage::MessageQuery {
                team,
                agent: "sender".parse().expect("assigner"),
                sender: Some("atm-daemon".parse().expect("daemon")),
                task_id: Some(task),
                limit: None,
            },
            atm_storage::ReadDeadline::new(std::time::Duration::from_secs(1))
                .expect("deadline"),
        )
        .await
        .expect("read assigner mailbox after retry");
    assert_eq!(
        receipts_after.len(),
        1,
        "an active task has no second handoff receipt"
    );
}

#[tokio::test]
async fn head_already_active_handoff_sends_no_receipt() {
    let (root, runtime, fake, pump, key, task, now) = build_task_handoff_pump();
    let message_id = task_assignment_message_id(&runtime, &key, &task).await;
    let team = key.team().clone();
    pump.tick_once().await;
    acknowledge_task_assignment(root.path(), &runtime, &key, message_id);
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:01:01Z").expect("timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    let store = runtime.task_store().expect("store");
    assert_eq!(
        store.load_task(&team, &task).expect("task").expect("row").state,
        TaskState::Active
    );
    let starts_before = store
        .list_task_events(&team, &task, None)
        .expect("events")
        .iter()
        .filter(|event| event.event == atm_storage::TaskEventKind::Started)
        .count();
    let reader = runtime.async_mailbox_reader().expect("mailbox reader");
    let receipt_query = || atm_storage::MessageQuery {
        team: team.clone(),
        agent: "sender".parse().expect("assigner"),
        sender: Some("atm-daemon".parse().expect("daemon")),
        task_id: Some(task.clone()),
        limit: None,
    };
    let receipts_before = reader
        .list_messages(
            atm_storage::MailboxScope::new(team.clone(), "sender".parse().expect("assigner")),
            receipt_query(),
            atm_storage::ReadDeadline::new(std::time::Duration::from_secs(1)).expect("deadline"),
        )
        .await
        .expect("receipts")
        .len();
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:02:02Z").expect("timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 3, "the active head still receives its due reminder");
    assert_eq!(
        store
            .list_task_events(&team, &task, None)
            .expect("events")
            .iter()
            .filter(|event| event.event == atm_storage::TaskEventKind::Started)
            .count(),
        starts_before
    );
    let receipts_after = reader
        .list_messages(
            atm_storage::MailboxScope::new(team.clone(), "sender".parse().expect("assigner")),
            receipt_query(),
            atm_storage::ReadDeadline::new(std::time::Duration::from_secs(1)).expect("deadline"),
        )
        .await
        .expect("receipts")
        .len();
    assert_eq!(receipts_after, receipts_before, "active tasks receive no second handoff receipt");
}

#[tokio::test]
async fn failed_start_write_then_active_member_still_starts_once() {
    let (root, runtime, fake, pump, key, task, now) = build_task_handoff_pump();
    let message_id = task_assignment_message_id(&runtime, &key, &task).await;
    acknowledge_task_assignment(root.path(), &runtime, &key, message_id);
    runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: key.team().clone(),
            members: vec![herdr_member(key.team(), key.agent().as_str())],
            refreshed_at: None,
        })
        .expect("temporarily remove assigner");
    pump.tick_once().await;
    let store = runtime.task_store().expect("task store");
    let failed = store
        .load_task(key.team(), &task)
        .expect("task")
        .expect("row");
    assert_eq!(failed.state, TaskState::Assigned);
    assert_eq!(failed.reminder_count, 1);
    assert_eq!(prompt_texts(&fake).len(), 1);

    runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: key.team().clone(),
            members: vec![
                herdr_member(key.team(), key.agent().as_str()),
                bare_member(key.team(), "sender"),
            ],
            refreshed_at: None,
        })
        .expect("restore assigner");
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:00:01Z").expect("timestamp");
    queue_status_result(&fake, std::slice::from_ref(&key), HerdrAgentStatus::Working);
    pump.tick_once().await;
    let started = store
        .load_task(key.team(), &task)
        .expect("task")
        .expect("row");
    assert_eq!(started.state, TaskState::Active);
    assert_eq!(started.reminder_count, 1);
    assert_eq!(prompt_texts(&fake).len(), 1, "owed start emits no second prompt");
    assert_eq!(
        store
            .list_task_events(key.team(), &task, None)
            .expect("events")
            .iter()
            .filter(|event| event.event == atm_storage::TaskEventKind::Started)
            .count(),
        1
    );
    let receipts = daemon_mail_for(&runtime, key.team(), "sender").await;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].envelope.summary.as_deref(), Some("task_started:AX5-HANDOFF"));
}

#[tokio::test]
async fn idle_member_with_open_mail_holds_mail_pending() {
    let (root, runtime, fake, _seeded_pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    queue_message(
        root.path(),
        &runtime,
        keys[0].team(),
        keys[0].agent().as_str(),
    );
    let pump = pump_with_selector(
        runtime.clone(),
        fake.clone(),
        RuntimeHealth::default(),
        now,
        Arc::new(NoEmitterSelector),
    );
    pump.tick_once().await;
    let task: TaskId = "AX5-TASK-00".parse().expect("task");
    assert!(prompt_texts(&fake).is_empty());
    assert_eq!(pump.stats().task_reminders, 0);
    assert_eq!(store.row(&keys[0], &task).reminder_count, 0);
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .list_pending_members()
            .expect("pending members")
            .contains(&keys[0]),
        "open mail remains pending when its backend is unavailable"
    );
}

async fn assert_refusal_read_failure_holds(error: atm_storage::ReadLaneError) {
    let (_root, _runtime, fake, pump, store, keys, _now) =
        build_task_only_pump_with_refusal_error(error);
    let warnings = WarningLayer::default();
    let subscriber = tracing_subscriber::Registry::default().with(warnings.clone());

    pump.tick_once().with_subscriber(subscriber).await;

    let task: TaskId = "AX5-TASK-00".parse().expect("task");
    assert!(prompt_texts(&fake).is_empty(), "an unknown refusal run must fail closed");
    assert_eq!(store.row(&keys[0], &task).reminder_count, 0);
    let refusal_warnings = warnings
        .events
        .lock()
        .expect("warning events")
        .iter()
        .filter(|(action, outcome)| action == "refusal_history_read" && outcome == "failed")
        .count();
    assert_eq!(refusal_warnings, 1, "the failed read emits one warning per tick");
}

#[tokio::test]
async fn refusal_reader_error_holds_before_disposition() {
    assert_refusal_read_failure_holds(atm_storage::ReadLaneError::Unavailable {
        message: "injected refusal reader failure".to_owned(),
    })
    .await;
}

#[tokio::test]
async fn refusal_reader_deadline_timeout_holds_before_disposition() {
    assert_refusal_read_failure_holds(atm_storage::ReadLaneError::DeadlineExpired {
        stage: "reading refusal history",
    })
    .await;
}

#[tokio::test]
async fn third_refusal_holds_and_escalates_once() {
    let (root, runtime, fake, pump, key, tasks, now) = build_real_task_pump(&[
        "REFUSE-01",
        "REFUSE-02",
        "REFUSE-03",
        "REFUSE-04",
        "REFUSE-05",
    ]);
    for task in &tasks[..3] {
        close_real_task(
            root.path(),
            &runtime,
            &key,
            task,
            atm_storage::TaskCloseOutcome::Refused,
        );
    }
    pump.tick_once().await;
    let mail = daemon_mail_for(
        &runtime,
        key.team(),
        atm_storage::roles::ROLE_TEAM_LEAD,
    )
    .await;
    assert_eq!(mail.len(), 1);
    assert_eq!(
        mail[0].envelope.summary.as_deref(),
        Some("escalation:refusals_escalated:worker@lifecycle-team")
    );
    assert!(prompt_texts(&fake).is_empty(), "refusal hold sends no Herdr prompt");

    close_real_task(
        root.path(),
        &runtime,
        &key,
        &tasks[3],
        atm_storage::TaskCloseOutcome::Refused,
    );
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        daemon_mail_for(&runtime, key.team(), atm_storage::roles::ROLE_TEAM_LEAD)
            .await
            .len(),
        1
    );
    for minute in 2..22 {
        *now.lock().expect("clock") = IsoTimestamp::from_str(&format!(
            "2030-01-01T00:{minute:02}:00Z"
        ))
        .expect("timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
    }
    assert!(prompt_texts(&fake).is_empty());
    assert_eq!(
        daemon_mail_for(&runtime, key.team(), atm_storage::roles::ROLE_TEAM_LEAD)
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn non_refused_close_releases_refusal_hold() {
    let (root, runtime, fake, pump, key, tasks, now) = build_real_task_pump(&[
        "RELEASE-01",
        "RELEASE-02",
        "RELEASE-03",
        "RELEASE-04",
        "RELEASE-05",
    ]);
    for task in &tasks[..3] {
        close_real_task(
            root.path(),
            &runtime,
            &key,
            task,
            atm_storage::TaskCloseOutcome::Refused,
        );
    }
    pump.tick_once().await;
    assert!(prompt_texts(&fake).is_empty());
    close_real_task(
        root.path(),
        &runtime,
        &key,
        &tasks[3],
        atm_storage::TaskCloseOutcome::Completed,
    );
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    let next = runtime
        .task_store()
        .expect("task store")
        .load_task(key.team(), &tasks[4])
        .expect("task")
        .expect("row");
    assert_eq!(next.state, TaskState::Active);
    assert_eq!(next.reminder_count, 1);
    assert_eq!(prompt_texts(&fake).len(), 1);
}

#[tokio::test]
async fn non_herdr_idle_member_with_task_is_nudged_through_its_backend() {
    let (_root, runtime, _seeded_fake, _seeded_pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    let mut tmux = bare_member(keys[0].team(), keys[0].agent().as_str());
    tmux.recipient_pane_id = Some(atm_core::types::PaneId::from_cli("%17").expect("pane"));
    runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: keys[0].team().clone(),
            members: vec![tmux],
            refreshed_at: None,
        })
        .expect("tmux roster");
    runtime.apply_roster_runtime_observations(
        keys[0].team(),
        &[atm_core::protocol::RosterRuntimeObservationUpdate::observed(
            keys[0].agent().clone(),
            RuntimeMemberState::Idle,
            atm_core::protocol::RuntimeObservationSource::Heartbeat,
            *now.lock().expect("clock"),
            None,
        )],
    );
    let tmux_paths = Arc::new(Mutex::new(Vec::new()));
    let tmux_fifo = BareCliFifo::default();
    let tmux_selector = Arc::new(BackendSelector {
        emitter: BackendEmitter {
            paths: tmux_paths.clone(),
            fifo: tmux_fifo,
            drops: BareCliQueueFullDrops::default(),
        },
    });
    let tmux_fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    let tmux_pump = pump_with_selector(
        runtime,
        tmux_fake.clone(),
        RuntimeHealth::default(),
        now,
        tmux_selector,
    );
    tmux_pump.tick_once().await;
    assert_eq!(tmux_paths.lock().expect("tmux paths").as_slice(), ["tmux"]);
    assert!(tmux_fake.calls().is_empty());
    assert_eq!(store.row(&keys[0], &"AX5-TASK-00".parse().expect("task")).reminder_count, 1);

    let (_root, runtime, _seeded_fake, _seeded_pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: keys[0].team().clone(),
            members: vec![bare_member(keys[0].team(), keys[0].agent().as_str())],
            refreshed_at: None,
        })
        .expect("bare CLI roster");
    runtime.apply_roster_runtime_observations(
        keys[0].team(),
        &[atm_core::protocol::RosterRuntimeObservationUpdate::observed(
            keys[0].agent().clone(),
            RuntimeMemberState::Idle,
            atm_core::protocol::RuntimeObservationSource::Heartbeat,
            *now.lock().expect("clock"),
            None,
        )],
    );
    let fifo_paths = Arc::new(Mutex::new(Vec::new()));
    let fifo = BareCliFifo::default();
    let fifo_selector = Arc::new(BackendSelector {
        emitter: BackendEmitter {
            paths: fifo_paths.clone(),
            fifo: fifo.clone(),
            drops: BareCliQueueFullDrops::default(),
        },
    });
    let fifo_fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    let fifo_pump = pump_with_selector(
        runtime,
        fifo_fake.clone(),
        RuntimeHealth::default(),
        now,
        fifo_selector,
    );
    fifo_pump.tick_once().await;
    assert_eq!(fifo_paths.lock().expect("fifo paths").as_slice(), ["fifo"]);
    assert_eq!(drain_bare_cli_messages(&fifo, &keys[0]).expect("drain FIFO").len(), 1);
    assert!(fifo_fake.calls().is_empty());
    assert_eq!(store.row(&keys[0], &"AX5-TASK-00".parse().expect("task")).reminder_count, 1);
}

#[tokio::test]
async fn member_without_dispatchable_backend_holds_and_logs_once() {
    let root = tempfile::tempdir().expect("temporary root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "no-channel-team".parse().expect("team");
    let key = atm_storage::MemberKey::new(team.clone(), "worker".parse().expect("agent"));
    let mut member = bare_member(&team, key.agent().as_str());
    member.harness = RosterHarness::ClaudeCode;
    assembly
        .nudge_template_override_store
        .disable_template_override(&team, atm_storage::BuiltInNudgeTemplateKind::Task)
        .expect("disable the final built-in dispatch path");
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: vec![member],
            refreshed_at: None,
        })
        .expect("no-channel roster");
    let assigned_at = IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("timestamp");
    let store = Arc::new(atm_storage::DummyTaskStore::with_rows(
        task_rows(&team, &[key.agent().to_string()], assigned_at),
        false,
    ));
    let reader: Arc<dyn atm_core::boundary::AsyncTaskLedgerReader + Send + Sync> = store.clone();
    let writer: Arc<dyn atm_core::boundary::TaskStore + Send + Sync> = store.clone();
    let runtime = assembly
        .service_runtime
        .with_async_task_ledger_reader(reader)
        .with_task_store(writer);
    runtime.apply_roster_runtime_observations(
        &team,
        &[atm_core::protocol::RosterRuntimeObservationUpdate::observed(
            key.agent().clone(),
            RuntimeMemberState::Idle,
            atm_core::protocol::RuntimeObservationSource::Heartbeat,
            assigned_at,
            None,
        )],
    );
    assert_eq!(
        runtime
            .roster_ephemeral_state(&team, key.agent())
            .expect("runtime state")
            .runtime
            .state,
        RuntimeMemberState::Idle
    );
    assert!(
        atm_core::nudge_dispatch::build_task_reminder_dispatch(
            &runtime,
            &key,
            &store.row(&key, &"AX5-TASK-00".parse().expect("task")),
        )
        .expect("dispatch classification")
        .is_none(),
        "the fixture reaches Hold(no delivery channel)"
    );
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    let now = Arc::new(Mutex::new(assigned_at));
    let pump = pump_with_selector(
        runtime.clone(),
        fake.clone(),
        RuntimeHealth::default(),
        now,
        Arc::new(NoEmitterSelector),
    );
    let warnings = WarningLayer::default();
    let subscriber = tracing::Dispatch::new(
        tracing_subscriber::Registry::default().with(warnings.clone()),
    );
    pump.tick_once().with_subscriber(subscriber.clone()).await;
    pump.tick_once().with_subscriber(subscriber).await;
    let no_channel_warnings = warnings
        .events
        .lock()
        .expect("warning events")
        .iter()
        .filter(|(action, outcome)| {
            action == "task_reminder_dispatch" && outcome == "no_delivery_channel"
        })
        .count();
    assert_eq!(
        no_channel_warnings,
        2,
        "one no-channel warning per tick: {:?}",
        warnings.events.lock().expect("warning events")
    );
    assert!(fake.calls().is_empty());
    assert_eq!(store.row(&key, &"AX5-TASK-00".parse().expect("task")).reminder_count, 0);
    assert!(
        daemon_mail_for(&runtime, &team, key.agent().as_str())
            .await
            .is_empty()
    );
}
