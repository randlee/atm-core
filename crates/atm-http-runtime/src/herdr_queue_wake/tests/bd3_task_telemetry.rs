//! BD3 projection of the queue-wake producers through real SQLite ticks.

use super::herdr_nudge_invariant::build_real_task_pump;
use super::*;
use crate::task_telemetry::{record_from_event, record_from_handoff};
use atm_core::TaskTelemetryRecord;
use atm_runtime::TaskTelemetryRuntime;
use atm_runtime_test_support::RecordingTaskTelemetrySink;

fn canonical(records: &[TaskTelemetryRecord]) -> Vec<String> {
    let mut lines: Vec<String> = records
        .iter()
        .map(|record| serde_json::to_string(record).expect("record json"))
        .collect();
    lines.sort();
    lines
}

fn set_clock(now: &Arc<Mutex<IsoTimestamp>>, value: &str) {
    *now.lock().expect("clock") = IsoTimestamp::from_str(value).expect("timestamp");
}

type ExpectedTaskEventFields = (
    u64,
    &'static str,
    Option<atm_core::boundary::TaskState>,
    Option<atm_core::boundary::TaskState>,
    Option<atm_storage::TaskCloseOutcome>,
    atm_core::boundary::TaskActor,
    Option<&'static str>,
);

fn assert_literal_row_fields(
    row: &atm_core::boundary::TaskEventRow,
    expected: ExpectedTaskEventFields,
) {
    assert_eq!(row.seq, expected.0);
    assert_eq!(row.at, expected.1.parse().expect("literal timestamp"));
    assert_eq!(row.from_state, expected.2);
    assert_eq!(row.to_state, expected.3);
    assert_eq!(
        row.to_state
            .and_then(atm_core::boundary::TaskState::close_outcome),
        expected.4
    );
    assert_eq!(row.actor, expected.5);
    assert_eq!(
        row.message_id,
        expected.6.map(|id| id.parse().expect("literal message id"))
    );
}

/// Positive: reminders, their task-pass handoffs, the lead notification and
/// the reminder reset each reach the sink as exactly their durable rows.
/// The assignment was committed before the runtime was attached, so it is the
/// one durable row with no record.
/// No-Claim: router producers are covered in
/// `storage_and_nudge_router/tests/bd3_task_telemetry.rs`.
#[tokio::test]
async fn queue_wake_producers_project_exactly_their_committed_rows() {
    let (_root, runtime, fake, pump, key, tasks, now) = build_real_task_pump(&["BD3-PUMP"]);
    let sink = RecordingTaskTelemetrySink::new();
    let setup = RecordingTaskTelemetrySink::setup(&sink);
    let telemetry = TaskTelemetryRuntime::start(setup.config, setup.sink);
    let pump = pump.with_task_telemetry(telemetry.clone());

    for minute in 0..10 {
        set_clock(&now, &format!("2030-01-01T00:{minute:02}:00Z"));
        if minute > 0 {
            queue_idle_result(&fake, &key);
        }
        pump.tick_once().await;
    }
    queue_idle_result(&fake, &key);
    set_clock(&now, "2030-01-01T00:10:00Z");
    pump.tick_once().await;
    for at in ["2030-01-01T00:10:30Z", "2030-01-01T00:11:30Z"] {
        set_clock(&now, at);
        queue_status_result(&fake, std::slice::from_ref(&key), HerdrAgentStatus::Working);
        pump.tick_once().await;
    }

    telemetry
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    let records = sink.records();

    let store = runtime.task_store().expect("task store");
    let rows = store
        .list_task_events(key.team(), &tasks[0], None)
        .expect("task events");
    let handoffs = runtime
        .async_task_ledger_reader()
        .expect("task-ledger reader")
        .list_prompt_handoffs(
            key.team().clone(),
            tasks[0].clone(),
            atm_storage::ReadDeadline::new(Duration::from_secs(1)).expect("deadline"),
        )
        .await
        .expect("prompt handoffs");
    assert_eq!(rows[0].event, atm_storage::TaskEventKind::Assigned);
    assert_literal_row_fields(
        &rows[1],
        (
            2,
            "2030-01-01T00:00:00Z",
            Some(atm_core::boundary::TaskState::Assigned),
            Some(atm_core::boundary::TaskState::Assigned),
            None,
            atm_core::boundary::TaskActor::Daemon,
            None,
        ),
    );
    let reminders_reset = rows
        .iter()
        .find(|row| row.event == atm_storage::TaskEventKind::RemindersReset)
        .expect("literal reminders-reset row");
    assert_literal_row_fields(
        reminders_reset,
        (
            13,
            "2030-01-01T00:11:30Z",
            Some(atm_core::boundary::TaskState::Assigned),
            Some(atm_core::boundary::TaskState::Assigned),
            None,
            atm_core::boundary::TaskActor::Daemon,
            None,
        ),
    );
    let mut expected: Vec<TaskTelemetryRecord> = rows[1..].iter().map(record_from_event).collect();
    expected.extend(handoffs.iter().map(record_from_handoff));
    assert_eq!(canonical(&records), canonical(&expected));

    let count = |kind: &str| {
        records
            .iter()
            .filter(|record| record.kind.as_str() == kind)
            .count()
    };
    assert_eq!(count("reminded"), 10);
    assert_eq!(count("prompt_handoff"), 10);
    assert_eq!(count("lead_notified"), 1);
    assert_eq!(count("reminders_reset"), 1);
    assert_eq!(count("assigned"), 0);
    assert_eq!(
        telemetry.diagnostics().snapshot().emitted,
        records.len() as u64
    );
}

#[tokio::test]
async fn failed_sqlite_reminder_update_emits_nothing_and_keeps_the_prompt_result() {
    let (root, runtime, fake, pump, key, tasks, _now) = build_real_task_pump(&["BD3-FAIL"]);
    let sink = RecordingTaskTelemetrySink::new();
    let setup = RecordingTaskTelemetrySink::setup(&sink);
    let telemetry = TaskTelemetryRuntime::start(setup.config, setup.sink);
    let pump = pump.with_task_telemetry(telemetry.clone());
    atm_runtime_test_support::install_sqlite_task_update_failure(
        root.path().join("runtime").join("mail.sqlite3"),
    )
    .expect("install deterministic task update failure");

    pump.tick_once().await;

    telemetry
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    let row = runtime
        .task_store()
        .expect("task store")
        .load_task(key.team(), &tasks[0])
        .expect("load task")
        .expect("task row");
    assert_eq!(
        row.reminder_count, 0,
        "failed update leaves the durable row unchanged"
    );
    assert_eq!(
        prompt_texts(&fake).len(),
        1,
        "the caller still emitted its prompt"
    );
    let records = sink.records();
    assert!(
        records
            .iter()
            .all(|record| record.kind.as_str() != "reminded"),
        "the failed reminder update projects no reminded record"
    );
    let handoffs = runtime
        .async_task_ledger_reader()
        .expect("task-ledger reader")
        .list_prompt_handoffs(
            key.team().clone(),
            tasks[0].clone(),
            atm_storage::ReadDeadline::new(Duration::from_secs(1)).expect("deadline"),
        )
        .await
        .expect("prompt handoffs");
    assert_eq!(
        handoffs.len(),
        1,
        "the delivered prompt's handoff persists independently of the reminder row"
    );
    let expected: Vec<TaskTelemetryRecord> = handoffs.iter().map(record_from_handoff).collect();
    assert_eq!(
        canonical(&records),
        canonical(&expected),
        "records are exactly the separately committed handoff rows"
    );
}

async fn advance_to_lead_notification(
    pump: &HerdrQueueWakePump,
    fake: &Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    key: &atm_storage::MemberKey,
    now: &Arc<Mutex<IsoTimestamp>>,
) {
    for minute in 0..10 {
        set_clock(now, &format!("2030-01-01T00:{minute:02}:00Z"));
        if minute > 0 {
            queue_idle_result(fake, key);
        }
        pump.tick_once().await;
    }
}

#[tokio::test]
async fn failed_sqlite_lead_notification_update_emits_nothing() {
    let (root, runtime, fake, pump, key, tasks, now) = build_real_task_pump(&["BD3-LEAD-FAIL"]);
    advance_to_lead_notification(&pump, &fake, &key, &now).await;
    let sink = RecordingTaskTelemetrySink::new();
    let setup = RecordingTaskTelemetrySink::setup(&sink);
    let telemetry = TaskTelemetryRuntime::start(setup.config, setup.sink);
    let pump = pump.with_task_telemetry(telemetry.clone());
    atm_runtime_test_support::install_sqlite_task_update_failure(
        root.path().join("runtime").join("mail.sqlite3"),
    )
    .expect("install deterministic task update failure");

    let load = || {
        let row = runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &tasks[0])
            .expect("load task")
            .expect("task row");
        (row.reminder_count, row.lead_notified_count)
    };
    let before = load();
    set_clock(&now, "2030-01-01T00:10:00Z");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        load(),
        before,
        "failed lead notification leaves both durable counters unchanged"
    );
    telemetry
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    assert!(
        sink.records().is_empty(),
        "failed lead notification emits no telemetry"
    );
}

#[tokio::test]
async fn failed_sqlite_reminder_reset_update_emits_nothing() {
    let (root, runtime, fake, pump, key, tasks, now) = build_real_task_pump(&["BD3-RESET-FAIL"]);
    advance_to_lead_notification(&pump, &fake, &key, &now).await;
    set_clock(&now, "2030-01-01T00:10:00Z");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    let sink = RecordingTaskTelemetrySink::new();
    let setup = RecordingTaskTelemetrySink::setup(&sink);
    let telemetry = TaskTelemetryRuntime::start(setup.config, setup.sink);
    let pump = pump.with_task_telemetry(telemetry.clone());
    atm_runtime_test_support::install_sqlite_task_update_failure(
        root.path().join("runtime").join("mail.sqlite3"),
    )
    .expect("install deterministic task update failure");

    let before = runtime
        .task_store()
        .expect("task store")
        .load_task(key.team(), &tasks[0])
        .expect("load task")
        .expect("task row");
    // A reset needs two consecutive Working observations a reminder apart.
    for at in ["2030-01-01T00:10:30Z", "2030-01-01T00:11:30Z"] {
        set_clock(&now, at);
        queue_status_result(&fake, std::slice::from_ref(&key), HerdrAgentStatus::Working);
        pump.tick_once().await;
    }
    telemetry
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    let after = runtime
        .task_store()
        .expect("task store")
        .load_task(key.team(), &tasks[0])
        .expect("load task")
        .expect("task row");
    assert_eq!(
        (after.reminder_count, after.lead_notified_count),
        (before.reminder_count, before.lead_notified_count),
        "failed reset leaves the durable row unchanged"
    );
    assert!(
        sink.records().is_empty(),
        "failed reminder reset emits no telemetry"
    );
}
