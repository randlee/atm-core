// Behavioral coverage for the BA.5 ephemeral queue contract.

use super::*;

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
        0,
        "the open queue item holds the task"
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
        0,
        "the queue item is still open"
    );
    close_message(root.path(), &runtime, &key, queue_message_id);

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:00:05Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        pump.stats().task_reminders,
        1,
        "the next tick observes the item as read"
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

    for (tick, timestamp) in [
        "2020-01-01T00:00:00Z",
        "2020-01-01T00:01:00Z",
        "2020-01-01T00:02:00Z",
    ]
    .into_iter()
    .enumerate()
    {
        *now.lock().expect("test clock lock") = timestamp.parse().expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            prompt_texts(&fake).len(),
            tick + 1,
            "one prompt at tick {tick}"
        );
        let (marker, attempts) = pending_state(root.path(), &key, message_id);
        assert!(
            marker.is_some(),
            "the unread item stays pending after tick {tick}"
        );
        assert_eq!(
            attempts, 0,
            "successful reminders do not consume retry attempts"
        );
    }

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2020-01-01T00:02:10Z").expect("test timestamp");
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
        IsoTimestamp::from_str("2020-01-01T00:00:00Z").expect("test timestamp"),
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

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2020-01-01T00:01:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        prompt_texts(&fake).len(),
        2,
        "read-but-unacked item is rediscovered"
    );

    add_roster_member(&runtime, key.team(), "sender");
    ack_task_assignment(root.path(), &runtime, key.team(), message_id);
    assert_eq!(pending_state(root.path(), &key, message_id), (None, 0));
    for seconds in 0..100 {
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str(&format!("2020-01-01T00:01:{:02}Z", seconds.min(59)))
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
    let (root, runtime, fake, _old_pump, health, key) =
        build_test_pump_with_agents(vec![AgentSnapshot {
            name: Some("aq27-agent".to_owned()),
            pane_id: None,
            status: HerdrAgentStatus::Blocked,
            workspace_id: None,
        }]);
    let task_id: TaskId = "BA5-BLOCKED-OPEN".parse().expect("task id");
    let message_id = queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        task_id.clone(),
    );
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, now);

    queue_idle_result(&fake, &key);
    pump.tick_once().await;

    assert_eq!(pump.stats().task_reminders_blocked, 1);
    assert!(prompt_texts(&fake).is_empty());
    assert!(pending_state(root.path(), &key, message_id).0.is_some());
    assert_eq!(
        runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &task_id)
            .expect("load task")
            .expect("task row")
            .state,
        TaskState::Assigned
    );
}
