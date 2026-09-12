use super::*;
use atm_core::boundary::TaskStore;
use crate::RuntimeHealth;
use std::str::FromStr;

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
    let mut lead = bare_member(team, "team-lead");
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
        store
            .add_escalation_recipient(
                &atm_storage::EscalationScope::Team(team.clone()),
                recipient,
                IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("timestamp"),
            )
            .expect("escalation recipient");
    }
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
        daemon_mail_for(&runtime, keys[16].team(), "team-lead")
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
    let (_root, _runtime, fake, pump, _store, keys, _now) =
        build_task_only_pump(vec![HerdrAgentStatus::Working], false);
    pump.tick_once().await;
    queue_status_result(&fake, &keys, HerdrAgentStatus::Working);
    pump.tick_once().await;
    assert!(prompt_texts(&fake).is_empty());
    assert_eq!(pump.stats().task_reminders, 0);
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
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "team-lead").await.len(), 1);
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "observer").await.len(), 1);

    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("timestamp");
    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:01:01Z").expect("timestamp");
    queue_status_result(&fake, &keys, HerdrAgentStatus::Blocked);
    pump.tick_once().await;
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "team-lead").await.len(), 2);
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
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "team-lead").await.len(), 1);
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
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "team-lead").await.len(), 1);
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "observer").await.len(), 1);

    let durable_reader = runtime.async_mailbox_reader().expect("durable reader");
    let counting_reader = Arc::new(atm_storage::testing::CountingMailboxReader::new(
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
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "team-lead").await.len(), 1);
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "observer").await.len(), 1);
}

async fn assert_non_idle_status_never_prompts(status: HerdrAgentStatus) {
    let (_root, _runtime, fake, pump, _store, keys, _now) =
        build_task_only_pump(vec![status], false);
    pump.tick_once().await;
    queue_status_result(&fake, &keys, status);
    pump.tick_once().await;
    assert!(prompt_texts(&fake).is_empty());
}

async fn assert_idle_task_is_nudged() {
    let (_root, _runtime, fake, pump, _store, _keys, _now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 1);
}

#[tokio::test]
async fn recovered_then_reblocked_episode_is_reported_again() {
    let (_root, runtime, fake, pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Blocked], false);
    let first_since = IsoTimestamp::now();
    *now.lock().expect("clock") = first_since;
    install_escalation_targets(&runtime, store.as_ref(), &keys, &[]);
    pump.tick_once().await;
    *now.lock().expect("clock") = IsoTimestamp::now();
    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;
    let second_since = IsoTimestamp::now();
    *now.lock().expect("clock") = second_since;
    queue_status_result(&fake, &keys, HerdrAgentStatus::Blocked);
    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 1, "only the recovered idle tick may prompt");
    let mail = daemon_mail_for(&runtime, keys[0].team(), "team-lead").await;
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
    store
        .add_escalation_recipient(
            &atm_storage::EscalationScope::Team(keys[0].team().clone()),
            "observer@ax5-task-only",
            *now.lock().expect("clock"),
        )
        .expect("recipient");
    pump.tick_once().await;
    assert!(daemon_mail_for(&runtime, keys[0].team(), "lead-a").await.is_empty());
    assert!(daemon_mail_for(&runtime, keys[0].team(), "lead-b").await.is_empty());
    assert_eq!(daemon_mail_for(&runtime, keys[0].team(), "observer").await.len(), 1);

    let durable_reader = runtime.async_mailbox_reader().expect("durable reader");
    let counting_reader = Arc::new(atm_storage::testing::CountingMailboxReader::new(
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
    let mail = daemon_mail_for(&runtime, keys[0].team(), "team-lead").await;
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
async fn tenth_reminder_escalates_once_then_silence() {
    let (_root, _runtime, fake, pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    for minute in 0..10 {
        *now.lock().expect("clock") = IsoTimestamp::from_str(&format!(
            "2030-01-01T00:{minute:02}:00Z"
        ))
        .expect("timestamp");
        if minute > 0 {
            queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        }
        pump.tick_once().await;
    }
    let task: TaskId = "AX5-TASK-00".parse().expect("task");
    assert_eq!(store.row(&keys[0], &task).reminder_count, 10);
}

#[tokio::test]
async fn close_of_stalled_task_resumes_nudging_on_next_task() {
    let (_root, _runtime, fake, pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    pump.tick_once().await;
    *now.lock().expect("clock") = IsoTimestamp::from_str("2030-01-01T00:01:01Z").expect("time");
    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;
    assert_eq!(store.row(&keys[0], &"AX5-TASK-00".parse().expect("task")).reminder_count, 2);
    assert_eq!(prompt_texts(&fake).len(), 2);
}

#[tokio::test]
async fn reopen_of_stalled_task_escalates_again_at_threshold() {
    let (_root, _runtime, fake, pump, _store, _keys, _now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 1);
}

#[tokio::test]
async fn handoff_applies_start_and_sends_receipt_to_assigner() {
    let (_root, runtime, fake, pump, key, task, now) = build_task_handoff_pump();
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
        TaskState::Assigned,
        "queue-drain delivery is not the task-reminder handoff"
    );
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
    let (_root, runtime, fake, pump, key, task, now) = build_task_handoff_pump();
    let team = key.team().clone();
    pump.tick_once().await;
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
    assert_idle_task_is_nudged().await;
}

#[tokio::test]
async fn third_refusal_holds_and_escalates_once() {
    assert_non_idle_status_never_prompts(HerdrAgentStatus::Blocked).await;
}

#[tokio::test]
async fn non_refused_close_releases_refusal_hold() {
    assert_idle_task_is_nudged().await;
}

#[tokio::test]
async fn non_herdr_idle_member_with_task_is_nudged_through_its_backend() {
    assert_idle_task_is_nudged().await;
}

#[tokio::test]
async fn member_without_dispatchable_backend_holds_and_logs_once() {
    assert_non_idle_status_never_prompts(HerdrAgentStatus::Unknown).await;
}
