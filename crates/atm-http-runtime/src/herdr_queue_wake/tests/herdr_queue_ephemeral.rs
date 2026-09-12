#![cfg(test)]

// Behavioral coverage for the BA.5 ephemeral queue contract.

use super::*;

async fn stalled_escalation_count(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &str,
) -> usize {
    runtime
        .async_mailbox_reader()
        .expect("mailbox reader")
        .list_messages(
            atm_core::boundary::MailboxScope::new(
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
            atm_core::boundary::ReadDeadline::new(std::time::Duration::from_secs(1))
                .expect("deadline"),
        )
        .await
        .expect("read escalation mail")
        .iter()
        .filter(|message| {
            message
                .envelope
                .summary
                .as_deref()
                .is_some_and(|summary| summary.starts_with("escalation:lead_notified:"))
        })
        .count()
}

async fn daemon_task_mail_count(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    task_id: &TaskId,
) -> usize {
    let sender: atm_core::types::AgentName = "sender".parse().expect("sender");
    let reader = runtime.async_mailbox_reader().expect("mailbox reader");
    let messages = reader
        .list_messages(
            atm_core::boundary::MailboxScope::new(team.clone(), sender.clone()),
            atm_storage::MessageQuery {
                team: team.clone(),
                agent: sender,
                sender: None,
                task_id: Some(task_id.clone()),
                limit: None,
            },
            atm_core::boundary::ReadDeadline::new(std::time::Duration::from_secs(1))
                .expect("read deadline"),
        )
        .await
        .expect("list assigner mailbox");
    messages.len()
}

#[tokio::test]
async fn task_prompt_waits_while_queue_item_open_across_ticks() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    clear_pending_markers(root.path(), &runtime, &key);
    let queue_message_id = queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
    let task_id: TaskId = "BA5-OPEN-TASK".parse().expect("task id");
    let task_message_id = queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        task_id,
    );
    close_message(root.path(), &runtime, &key, task_message_id);
    add_roster_member(&runtime, key.team(), "sender");
    ack_task_assignment(root.path(), &runtime, key.team(), task_message_id);
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));

    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        pump.stats().prompted,
        1,
        "the unread queue item is prompted at t0"
    );
    assert_eq!(
        pump.stats().task_reminders,
        1,
        "the delivered queue prompt counts for the open task"
    );

    for seconds in [5, 10, 55] {
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str(&format!("2030-01-01T00:00:{seconds:02}Z"))
                .expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            pump.stats().task_reminders,
            0,
            "open queue item holds at +{seconds}s"
        );
    }

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:00:58Z").expect("test timestamp");
    close_message(root.path(), &runtime, &key, queue_message_id);
    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        pump.stats().task_reminders,
        1,
        "the open task is prompted at +60s"
    );
    assert!(
        prompt_texts(&fake)
            .last()
            .is_some_and(|text| text.contains("BA5-OPEN-TASK"))
    );
}

#[tokio::test]
async fn open_mail_set_is_read_each_tick_not_cached() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    clear_pending_markers(root.path(), &runtime, &key);
    let queue_message_id = queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
    let task_id: TaskId = "BA5-CACHE-TASK".parse().expect("task id");
    let task_message_id = queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        task_id,
    );
    close_message(root.path(), &runtime, &key, task_message_id);
    add_roster_member(&runtime, key.team(), "sender");
    ack_task_assignment(root.path(), &runtime, key.team(), task_message_id);
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));

    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        pump.stats().task_reminders,
        1,
        "the delivered queue prompt counts for the open task"
    );
    close_message(root.path(), &runtime, &key, queue_message_id);

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        pump.stats().task_reminders,
        1,
        "the next due tick observes the item as read"
    );
    assert!(
        prompt_texts(&fake)
            .last()
            .is_some_and(|text| text.contains("BA5-CACHE-TASK"))
    );
}

#[tokio::test]
async fn unread_queue_item_is_reprompted_every_interval() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    clear_pending_markers(root.path(), &runtime, &key);
    let message_id = queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));
    let pending_store = runtime.pending_nudge_store().expect("pending store");

    for (tick, (timestamp, next_due)) in [
        ("2030-01-01T00:00:00Z", "2030-01-01T00:01:00+00:00"),
        ("2030-01-01T00:01:00Z", "2030-01-01T00:02:00+00:00"),
        ("2030-01-01T00:02:00Z", "2030-01-01T00:03:00+00:00"),
    ]
    .into_iter()
    .enumerate()
    {
        if tick > 0 {
            make_failed_attempt_due(pending_store.as_ref(), &key, &message_id);
        }
        *now.lock().expect("test clock lock") = timestamp.parse().expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            prompt_texts(&fake).len(),
            tick + 1,
            "one prompt at tick {tick}"
        );
        let (marker, attempts) = pending_state(root.path(), &key, message_id);
        assert_eq!(marker.as_deref(), Some(next_due));
        assert_eq!(
            attempts, 0,
            "successful reminders do not consume retry attempts"
        );
    }

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:02:10Z").expect("test timestamp");
    close_message(root.path(), &runtime, &key, message_id);
    assert_eq!(pending_state(root.path(), &key, message_id), (None, 0));
    for _ in 0..100 {
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
    }
    assert_eq!(
        prompt_texts(&fake).len(),
        3,
        "reading stops all later reminders"
    );
    assert!(
        pending_store
            .list_pending_members()
            .expect("pending members")
            .is_empty()
    );
}

#[tokio::test]
async fn requires_ack_message_reminded_until_acked() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    clear_pending_markers(root.path(), &runtime, &key);
    let message_id =
        queue_requires_ack_message(root.path(), &runtime, key.team(), key.agent().as_str());
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));

    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        prompt_texts(&fake).len(),
        1,
        "the requires-ack item is initially prompted"
    );
    assert!(acknowledgement_is_pending(root.path(), &key, message_id));
    close_message(root.path(), &runtime, &key, message_id);
    assert!(pending_state(root.path(), &key, message_id).0.is_some());

    assert_eq!(
        pending_state(root.path(), &key, message_id).0.as_deref(),
        Some("2030-01-01T00:01:00+00:00")
    );
    make_failed_attempt_due(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .as_ref(),
        &key,
        &message_id,
    );
    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        prompt_texts(&fake).len(),
        2,
        "read-but-unacked item is rediscovered"
    );
    assert_eq!(
        pending_state(root.path(), &key, message_id).0.as_deref(),
        Some("2030-01-01T00:02:00+00:00")
    );

    add_roster_member(&runtime, key.team(), "sender");
    ack_task_assignment(root.path(), &runtime, key.team(), message_id);
    assert_eq!(pending_state(root.path(), &key, message_id), (None, 0));
    for seconds in 0..100 {
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str(&format!("2030-01-01T00:01:{:02}Z", seconds.min(59)))
                .expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
    }
    assert_eq!(
        prompt_texts(&fake).len(),
        2,
        "acknowledgement stops reminders"
    );
}

#[tokio::test]
async fn bare_cli_pull_preserves_item_until_member_reads() {
    let fixture = crate::storage_and_nudge_router::tests::bare_cli_pull_fixture();
    let member = fixture.member.clone();
    let router = fixture.router;
    let response = router
        .dispatch(
            atm_core::api::ApiRequest::new(atm_core::protocol::RequestEnvelope::QueueGetNext(
                atm_core::protocol::QueueGetNextRequest {
                    team: member.team().clone(),
                    member: member.agent().clone(),
                },
            )),
            atm_core::AuthenticatedIngress::Local,
            atm_core::api::RequestDeadline::after(std::time::Duration::from_secs(1)),
        )
        .await
        .expect("authorized queue-get");
    let atm_core::protocol::ResponseEnvelope::QueueGetNext(response) = response.into_inner() else {
        panic!("expected a QueueGetNext response");
    };
    assert_eq!(response.messages.len(), 1);
    assert_eq!(response.messages[0].msg_id, fixture.message_id);
    assert!(
        fixture
            .pending_nudge_store
            .list_pending_members()
            .expect("list pending members")
            .contains(&member),
        "queue_get_next is transport only and preserves the leased marker"
    );

    let message_id_text = fixture.message_id.to_string();
    let read_query = atm_core::read::ReadQuery::new(
        fixture.home_dir,
        fixture.current_dir,
        member.agent().clone(),
        Some(&format!("{}@{}", member.agent(), member.team())),
        member.team().clone(),
        atm_core::types::ReadSelection::All,
        false,
        true,
        Some(&message_id_text),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("read query");
    atm_core::read::read_mail_with_runtime(
        read_query,
        &atm_core::observability::NullObservability,
        &fixture.runtime,
    )
    .expect("member reads pulled queue item");
    assert!(
        fixture
            .pending_nudge_store
            .list_pending_members()
            .expect("list pending members after read")
            .is_empty(),
        "the member read closes the leased marker"
    );
    assert!(
        fixture
            .pending_nudge_store
            .claim_next_pending(&member)
            .expect("claim after read")
            .is_none(),
        "a closed item cannot be prompted by later queue ticks"
    );
}

#[tokio::test]
async fn handoff_started_task_closed_without_read_acknowledges_assignment() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    clear_pending_markers(root.path(), &runtime, &key);
    let task_id: TaskId = "BA5-HANDOFF-CLOSE".parse().expect("task id");
    let assignment_id = queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        task_id.clone(),
    );
    add_roster_member(&runtime, key.team(), "sender");
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));

    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    let assigned = runtime
        .task_store()
        .expect("task store")
        .load_task(key.team(), &task_id)
        .expect("load task")
        .expect("task row");
    assert_eq!(assigned.state, TaskState::Assigned);
    assert_eq!(assigned.reminder_count, 1);

    complete_task(root.path(), &runtime, key.team(), task_id.clone());
    assert!(
        !acknowledgement_is_pending(root.path(), &key, assignment_id),
        "closing the handed-off task acknowledges its assignment"
    );
    assert_eq!(pending_state(root.path(), &key, assignment_id), (None, 0));

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        prompt_texts(&fake).len(),
        1,
        "closed assignment is not nudged again"
    );
}

#[tokio::test]
async fn mail_and_task_share_one_prompt_per_tick() {
    let (root, runtime, fake, pump, _task_store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    let key = keys[0].clone();
    let mail_id = queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
    let task_id: TaskId = "AX5-TASK-00".parse().expect("task id");

    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(pump.stats().prompted, 1);
    assert_eq!(
        pump.stats().task_reminders,
        1,
        "the mail prompt also counts for the open task"
    );
    assert_eq!(prompt_texts(&fake).len(), 1);
    assert!(!prompt_texts(&fake)[0].contains(task_id.as_str()));

    close_message(root.path(), &runtime, &key, mail_id);
    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(pump.stats().task_reminders, 1);
    assert!(prompt_texts(&fake)[1].contains(task_id.as_str()));
}

#[tokio::test]
async fn unread_assignment_counts_to_stall_and_escalates_at_ten() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    clear_pending_markers(root.path(), &runtime, &key);
    let task_id: TaskId = "BA5-UNREAD-STALL".parse().expect("task id");
    let assignment_id = queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        task_id.clone(),
    );
    add_roster_member(&runtime, key.team(), "sender");
    add_lead_roster_member(&runtime, key.team(), atm_storage::roles::ROLE_TEAM_LEAD);
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2020-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));
    let store = runtime.task_store().expect("task store");

    for minute in 0..10 {
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str(&format!("2020-01-01T00:{minute:02}:00Z"))
                .expect("test timestamp");
        if minute > 0 {
            queue_idle_result(&fake, &key);
        }
        pump.tick_once().await;
        let row = store
            .load_task(key.team(), &task_id)
            .expect("load task")
            .expect("task row");
        assert_eq!(row.reminder_count, minute + 1);
        assert_eq!(row.lead_notified_count, 0);
    }
    assert_eq!(prompt_texts(&fake).len(), 10);

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2020-01-01T00:10:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    let stalled = store
        .load_task(key.team(), &task_id)
        .expect("load task")
        .expect("task row");
    assert_eq!(stalled.reminder_count, 10);
    assert_eq!(stalled.lead_notified_count, 1);
    assert_eq!(prompt_texts(&fake).len(), 10);
    assert!(pending_state(root.path(), &key, assignment_id).0.is_some());
    assert_eq!(
        stalled_escalation_count(&runtime, key.team(), atm_storage::roles::ROLE_TEAM_LEAD,).await,
        1
    );

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2020-01-01T00:11:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    let terminal = store
        .load_task(key.team(), &task_id)
        .expect("load task")
        .expect("task row");
    assert_eq!(terminal.reminder_count, 10);
    assert_eq!(terminal.lead_notified_count, 1);
    assert_eq!(
        prompt_texts(&fake).len(),
        11,
        "ordinary mail remains deliverable"
    );
    assert_eq!(
        stalled_escalation_count(&runtime, key.team(), atm_storage::roles::ROLE_TEAM_LEAD,).await,
        1
    );
}

#[tokio::test]
async fn unrelated_mail_prompt_does_not_start_the_assigned_head_task() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    clear_pending_markers(root.path(), &runtime, &key);
    let _plain = queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
    let task_id: TaskId = "BA5-UNSEEN-HEAD".parse().expect("task id");
    let _assignment = queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        task_id.clone(),
    );
    add_roster_member(&runtime, key.team(), "sender");
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, now);

    pump.tick_once().await;

    let store = runtime.task_store().expect("task store");
    let head = store
        .load_task(key.team(), &task_id)
        .expect("load head")
        .expect("head task");
    assert_eq!(head.state, TaskState::Assigned);
    assert_eq!(
        head.reminder_count, 1,
        "plain mail still counts as a reminder"
    );
    assert!(
        store
            .list_task_events(key.team(), &task_id, Some(key.agent()))
            .expect("task events")
            .iter()
            .all(|event| event.event != atm_storage::TaskEventKind::Started)
    );
    assert_eq!(
        daemon_task_mail_count(&runtime, key.team(), &task_id).await,
        0
    );
}

#[tokio::test]
async fn prompted_task_never_started_stays_assigned_and_keeps_reminding() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    clear_pending_markers(root.path(), &runtime, &key);
    let first: TaskId = "BA5-HEAD-TASK".parse().expect("task id");
    let second: TaskId = "BA5-NONHEAD-TASK".parse().expect("task id");
    queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        first.clone(),
    );
    queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        second.clone(),
    );
    add_roster_member(&runtime, key.team(), "sender");
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));

    // build_test_pump preloads the first idle response used by this tick.
    pump.tick_once().await;
    let store = runtime.task_store().expect("task store");
    let head = store
        .load_task(key.team(), &first)
        .expect("load head")
        .expect("head task");
    let non_head = store
        .load_task(key.team(), &second)
        .expect("load non-head")
        .expect("non-head task");
    assert_eq!(head.state, TaskState::Assigned);
    assert_eq!(head.reminder_count, 1);
    assert_eq!(non_head.state, TaskState::Assigned);
    let started = store
        .list_task_events(key.team(), &first, Some(key.agent()))
        .expect("task events")
        .into_iter()
        .filter(|event| event.event == atm_storage::TaskEventKind::Started)
        .count();
    assert_eq!(started, 0, "the daemon never starts a prompted task");
    assert_eq!(
        daemon_task_mail_count(&runtime, key.team(), &first).await,
        0,
        "the daemon writes no task-linked start receipt"
    );
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .list_pending_members()
            .expect("pending members")
            .iter()
            .all(|member| member.agent().as_str() != "sender"),
        "no daemon receipt queues mail for the assigner"
    );

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:01Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        prompt_texts(&fake).len(),
        2,
        "the assigned task keeps reminding after the interval"
    );
    let reminded = store
        .load_task(key.team(), &first)
        .expect("load reminded head")
        .expect("reminded head");
    assert_eq!(reminded.state, TaskState::Assigned);
    assert_eq!(reminded.reminder_count, 2);
    assert_eq!(
        daemon_task_mail_count(&runtime, key.team(), &first).await,
        0,
        "repeated prompts add no daemon-authored task mail"
    );
    assert_eq!(
        runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &second)
            .expect("load non-head")
            .expect("non-head task")
            .state,
        TaskState::Assigned
    );
}

#[tokio::test]
async fn immediate_send_is_never_reminded() {
    let (root, runtime, fake, pump, _health, key) = build_test_pump();
    clear_pending_markers(root.path(), &runtime, &key);
    immediate_message(root.path(), &runtime, key.team(), key.agent().as_str());
    for _ in 0..200 {
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
    }
    assert!(prompt_texts(&fake).is_empty());
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .list_pending_members()
            .expect("pending members")
            .is_empty()
    );
}

#[tokio::test]
async fn failed_dispatch_backs_off_after_max_attempts_and_still_closes_on_read() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    clear_pending_markers(root.path(), &runtime, &key);
    let message_id = queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2020-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));
    let pending_store = runtime.pending_nudge_store().expect("pending store");

    for failure in 0..atm_storage::MAX_NUDGE_ATTEMPTS {
        fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentPromptStalled));
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            pump.stats().released,
            1,
            "failed attempt {failure} is released"
        );
        if failure + 1 < atm_storage::MAX_NUDGE_ATTEMPTS {
            make_failed_attempt_due(pending_store.as_ref(), &key, &message_id);
        }
    }
    let (marker, attempts) = pending_state(root.path(), &key, message_id);
    assert!(marker.is_some(), "max-attempt item remains durably marked");
    assert_eq!(attempts, 0, "max attempts reset the retry counter");
    assert_eq!(
        prompt_texts(&fake).len(),
        atm_storage::MAX_NUDGE_ATTEMPTS as usize
    );

    close_message(root.path(), &runtime, &key, message_id);
    assert_eq!(pending_state(root.path(), &key, message_id), (None, 0));
    for _ in 0..100 {
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
    }
    assert_eq!(
        prompt_texts(&fake).len(),
        atm_storage::MAX_NUDGE_ATTEMPTS as usize
    );
}

#[test]
fn queue_creates_no_task_row_and_no_state_column() {
    let (root, runtime, _fake, _pump, _health, key) = build_test_pump();
    let before = runtime
        .task_store()
        .expect("task store")
        .list_tasks(key.team(), None)
        .expect("tasks before queue");
    queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
    let after = runtime
        .task_store()
        .expect("task store")
        .list_tasks(key.team(), None)
        .expect("tasks after queue");
    assert_eq!(before, after);

    let columns = atm_runtime_test_support::inspect_mail_message_state_columns_for_test(
        root.path().join("runtime/mail.sqlite3"),
    )
    .expect("inspect mail state schema");
    assert_eq!(
        columns,
        [
            "team",
            "agent",
            "message_key",
            "read",
            "pending_ack_at",
            "acknowledged_at",
            "expires_at",
            "deleted_at",
            "updated_at",
            "nudge_pending_at",
            "nudge_attempts",
        ]
    );
}

#[tokio::test]
async fn no_mailbox_list_read_in_the_queue_pass() {
    let root = tempfile::tempdir().expect("temporary root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "aq27-team".parse().expect("team");
    let agent: atm_core::types::AgentName = "aq27-agent".parse().expect("agent");
    let key = atm_core::boundary::MemberKey::new(team.clone(), agent.clone());
    let durable_roster = assembly.service_runtime.shared_roster_store_arc();
    durable_roster
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: vec![herdr_member(&team, agent.as_str())],
            refreshed_at: None,
        })
        .expect("roster");
    let roster = atm_runtime_test_support::build_write_through_roster_for_test(durable_roster)
        .expect("write-through roster fixture");
    let list_messages_calls = Arc::new(AtomicUsize::new(0));
    let message_store = Arc::new(CountingMessageStore {
        inner: assembly.message_store_arc(),
        list_messages_calls: Arc::clone(&list_messages_calls),
    });
    let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        message_store,
        roster,
        Arc::new(NoopNudgeTemplateOverrideStore),
        Arc::new(atm_core::LocalFileNonClaudeOutbound::new()),
    )
    .with_pending_nudge_store(
        assembly
            .service_runtime
            .pending_nudge_store()
            .expect("pending store"),
    );
    let message_id = queue_message(root.path(), &runtime, &team, agent.as_str());
    assert!(pending_state(root.path(), &key, message_id).0.is_some());
    list_messages_calls.store(0, Ordering::SeqCst);
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(
        runtime.clone(),
        fake.clone(),
        crate::RuntimeHealth::default(),
        now,
    );
    for _ in 0..50 {
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
    }
    assert_eq!(prompt_texts(&fake).len(), 1);
    assert_eq!(
        list_messages_calls.load(Ordering::SeqCst),
        0,
        "the queue pass must use pending markers and never enumerate the mailbox"
    );
}

#[tokio::test]
async fn blocked_member_with_open_item_escalates_not_reminded() {
    let (root, runtime, fake, pump, task_store, keys, _now) =
        build_task_only_pump(vec![HerdrAgentStatus::Blocked], false);
    let key = keys[0].clone();
    let task_id: TaskId = "AX5-TASK-00".parse().expect("task id");
    let message_id = queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        task_id.clone(),
    );
    add_lead_roster_member(&runtime, key.team(), "sender");
    queue_status_result(&fake, &keys, HerdrAgentStatus::Blocked);
    pump.tick_once().await;

    assert_eq!(pump.stats().task_reminders, 0);
    assert_eq!(pump.stats().blocked_escalations, 1);
    assert!(prompt_texts(&fake).is_empty());
    assert!(pending_state(root.path(), &key, message_id).0.is_some());
    assert_eq!(task_store.row(&key, &task_id).state, TaskState::Assigned);
}
