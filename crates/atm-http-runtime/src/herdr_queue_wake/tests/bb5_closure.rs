use super::*;

type BbAssignmentFixture = (
    tempfile::TempDir,
    LocalServiceRuntime,
    Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    HerdrQueueWakePump,
    atm_core::boundary::MemberKey,
    Vec<TaskId>,
    Arc<Mutex<IsoTimestamp>>,
);

fn build_bb_assignment_pump(task_count: usize) -> BbAssignmentFixture {
    let root = tempfile::tempdir().expect("temporary root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "bb5-runtime".parse().expect("team");
    let key = atm_core::boundary::MemberKey::new(team.clone(), "bb5-agent".parse().expect("agent"));
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: vec![herdr_member(&team, key.agent().as_str())],
            refreshed_at: None,
        })
        .expect("roster");
    let tasks = (0..task_count)
        .map(|index| format!("BB5-{index}").parse().expect("task id"))
        .collect::<Vec<TaskId>>();
    for task in &tasks {
        queue_task_message_with_nudge(
            root.path(),
            &assembly.service_runtime,
            &team,
            key.agent().as_str(),
            task.clone(),
            NudgeMode::Immediate,
        );
    }
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    queue_idle_result(&fake, &key);
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("timestamp"),
    ));
    let pump = pump_with_clock(
        assembly.service_runtime.clone(),
        fake.clone(),
        RuntimeHealth::default(),
        Arc::clone(&now),
    );
    (root, assembly.service_runtime, fake, pump, key, tasks, now)
}

#[tokio::test]
async fn task_pass_first_prompt_is_ready_then_reminder_with_attempt() {
    let (_root, _runtime, fake, pump, key, _tasks, now) = build_bb_assignment_pump(1);
    pump.tick_once().await;
    *now.lock().expect("clock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;

    let prompts = prompt_texts(&fake);
    assert_eq!(prompts.len(), 2);
    assert!(prompts[0].contains(" ready "));
    assert!(prompts[1].contains(" reminder=\"1\" "));
}

#[tokio::test]
async fn task_pass_records_reminder_against_prompted_task_only() {
    let (_root, runtime, _fake, pump, key, tasks, _now) = build_bb_assignment_pump(2);
    pump.tick_once().await;
    let store = runtime.task_store().expect("task store");
    let first = store
        .load_task(key.team(), &tasks[0])
        .expect("load head")
        .expect("head");
    let second = store
        .load_task(key.team(), &tasks[1])
        .expect("load second")
        .expect("second");
    assert_eq!(first.reminder_count, 1);
    assert_eq!(second.reminder_count, 0);
}

#[tokio::test]
async fn pump_never_claims_an_assignment() {
    let (_root, runtime, fake, pump, key, _tasks, _now) = build_bb_assignment_pump(1);
    let pending = runtime.pending_nudge_store().expect("pending store");
    assert!(
        pending
            .claim_next_pending(&key)
            .expect("pre-pump claim")
            .is_none()
    );
    pump.tick_once().await;
    assert!(
        pending
            .claim_next_pending(&key)
            .expect("post-pump claim")
            .is_none()
    );
    assert_eq!(prompt_texts(&fake).len(), 1);
}

#[tokio::test]
async fn pump_never_claims_a_bb_assignment_and_loses_no_prompt() {
    let (_root, runtime, fake, pump, key, tasks, _now) = build_bb_assignment_pump(3);
    pump.tick_once().await;
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .claim_next_pending(&key)
            .expect("claim")
            .is_none()
    );
    let prompts = prompt_texts(&fake);
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains(&format!("task=\"{}\" ready", tasks[0])));
    assert!(!prompts[0].contains("queued="));
}

#[tokio::test]
async fn legacy_assignment_claimed_before_normalization_renders_delivery() {
    let (_root, runtime, fake, pump, key, task, _now) = build_task_handoff_pump();
    let assignment = runtime
        .task_store()
        .expect("task store")
        .load_task(key.team(), &task)
        .expect("load task")
        .expect("task row")
        .assignment_message_id;
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .mark_pending(
                &key,
                &assignment,
                IsoTimestamp::from_str("2020-01-01T00:00:00Z").expect("timestamp"),
            )
            .expect("seed pre-normalization assignment marker")
    );
    pump.tick_once().await;
    let prompts = prompt_texts(&fake);
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].starts_with("<atm from=\""), "{prompts:?}");
    assert!(!prompts[0].contains(task.as_str()));
    assert_eq!(pump.stats().task_reminders_failed, 0);
}
