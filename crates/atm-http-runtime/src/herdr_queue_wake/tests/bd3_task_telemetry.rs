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
