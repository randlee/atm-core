//! BD3 projection through the real router handlers and SQLite ledger.

use super::*;
use crate::task_telemetry::{record_from_event, record_from_handoff};
use atm_core::TaskTelemetryRecord;
use atm_runtime::{TaskTelemetryConfig, TaskTelemetryRuntime};
use atm_runtime_test_support::{RecordingTaskTelemetrySink, StalledTaskTelemetrySink};

const TEAM: &str = "test-team";

fn recording_runtime(sink: &Arc<RecordingTaskTelemetrySink>) -> TaskTelemetryRuntime {
    let setup = RecordingTaskTelemetrySink::setup(sink);
    TaskTelemetryRuntime::start(setup.config, setup.sink)
}

async fn write(
    router: &StorageAndNudgeRouter,
    request: WriteRequest,
) -> Result<ResponseEnvelope, AtmError> {
    router
        .dispatch(
            ApiRequest::new(RequestEnvelope::Write(Box::new(request))),
            AuthenticatedIngress::Local,
            RequestDeadline::after(Duration::from_secs(10)),
        )
        .await
        .map(atm_core::ApiResponse::into_inner)
}

fn task_op_request(
    fixture: &Fixture,
    from: &str,
    to: &str,
    task_id: &str,
    op: atm_storage::TaskOp,
) -> WriteRequest {
    let mut request = assignment_request(fixture, from, to, task_id);
    request.task_op = Some(op);
    request
}

fn close(outcome: atm_storage::TaskCloseOutcome) -> atm_storage::TaskOp {
    atm_storage::TaskOp::Close {
        outcome,
        reason: None,
    }
}

fn sent_message_id(response: ResponseEnvelope) -> atm_core::schema::AtmMessageId {
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("write must send: {response:?}")
    };
    outcome.message_id
}

fn task_rows(fixture: &Fixture, task_id: &str) -> Vec<atm_core::boundary::TaskEventRow> {
    fixture
        .task_store
        .list_task_events(
            &TEAM.parse().expect("team"),
            &task_id.parse().expect("task id"),
            None,
        )
        .expect("durable task events")
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

/// The records every committed row and inserted handoff of `tasks` must have
/// produced, in a stable order for comparison.
async fn durable_records(fixture: &Fixture, tasks: &[&str]) -> Vec<String> {
    let mut expected = Vec::new();
    for task in tasks {
        expected.extend(task_rows(fixture, task).iter().map(record_from_event));
        expected.extend(
            prompt_handoffs(fixture, task)
                .await
                .iter()
                .map(record_from_handoff),
        );
    }
    canonical(&expected)
}

fn canonical(records: &[TaskTelemetryRecord]) -> Vec<String> {
    let mut lines: Vec<String> = records
        .iter()
        .map(|record| serde_json::to_string(record).expect("record json"))
        .collect();
    lines.sort();
    lines
}

async fn settle(runtime: &TaskTelemetryRuntime) {
    runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
}

fn kinds(records: &[TaskTelemetryRecord], task: &str) -> Vec<&'static str> {
    records
        .iter()
        .filter(|record| record.task_id.as_str() == task)
        .map(|record| record.kind.as_str())
        .collect()
}

/// Positive: every live router producer (assignment, reassignment, start,
/// terminal close, reopen, placement, direct move, committed rejection audit
/// and newly inserted handoff) reaches the sink as exactly its durable row.
/// Negative: the acknowledgement adds no task row and no record.
/// No-Claim: export to a collector is bd-5/bd-6; queue-wake producers are
/// covered in `herdr_queue_wake/tests/bd3_task_telemetry.rs`.
#[tokio::test]
async fn router_producers_project_exactly_their_committed_rows() {
    let fixture = fixture(true, None, None);
    let sink = RecordingTaskTelemetrySink::new();
    let runtime = recording_runtime(&sink);
    let router = fixture.router.clone().with_task_telemetry(runtime.clone());

    write(
        &router,
        assignment_request(&fixture, "sender", "recipient", "BD3-T1").with_origin_metadata(
            "00000000000000000000000001".parse().expect("message id"),
            "2030-01-01T00:00:01Z".parse().expect("timestamp"),
        ),
    )
    .await
    .expect("assign");
    write(
        &router,
        assignment_request(&fixture, "sender", "third", "BD3-T1").with_origin_metadata(
            "00000000000000000000000002".parse().expect("message id"),
            "2030-01-01T00:00:02Z".parse().expect("timestamp"),
        ),
    )
    .await
    .expect("reassign");
    write(
        &router,
        task_op_request(
            &fixture,
            "third",
            "sender",
            "BD3-T1",
            atm_storage::TaskOp::Start,
        )
        .with_origin_metadata(
            "00000000000000000000000003".parse().expect("message id"),
            "2030-01-01T00:00:03Z".parse().expect("timestamp"),
        ),
    )
    .await
    .expect("start");
    write(
        &router,
        task_op_request(
            &fixture,
            "third",
            "sender",
            "BD3-T1",
            close(atm_storage::TaskCloseOutcome::Completed),
        )
        .with_origin_metadata(
            "00000000000000000000000004".parse().expect("message id"),
            "2030-01-01T00:00:04Z".parse().expect("timestamp"),
        ),
    )
    .await
    .expect("close");
    write(
        &router,
        assignment_request(&fixture, "sender", "third", "BD3-T1").with_origin_metadata(
            "00000000000000000000000005".parse().expect("message id"),
            "2030-01-01T00:00:05Z".parse().expect("timestamp"),
        ),
    )
    .await
    .expect("reopen");

    let rejected = write(
        &router,
        task_op_request(
            &fixture,
            "third",
            "sender",
            "BD3-MISSING",
            close(atm_storage::TaskCloseOutcome::Completed),
        )
        .with_origin_metadata(
            "00000000000000000000000006".parse().expect("message id"),
            "2030-01-01T00:00:06Z".parse().expect("timestamp"),
        ),
    )
    .await
    .expect_err("closing an unknown task is refused");
    assert_eq!(rejected.code(), atm_storage::AtmErrorCode::TaskNotFound);

    let mut t2_assignment = assignment_request(&fixture, "sender", "recipient", "BD3-T2");
    t2_assignment.requires_ack = true;
    let t2 = sent_message_id(write(&router, t2_assignment).await.expect("assign T2"));
    write(
        &router,
        assignment_request(&fixture, "sender", "recipient", "BD3-T3"),
    )
    .await
    .expect("assign T3");
    let mut placed = assignment_request(&fixture, "sender", "recipient", "BD3-T4");
    placed.placement = Some(MoveTarget::Head);
    write(&router, placed).await.expect("placed assignment");
    router
        .dispatch(
            ApiRequest::new(RequestEnvelope::TaskMove(TaskMoveRequest {
                caller_identity: "recipient".parse().expect("agent"),
                caller_team: TEAM.parse().expect("team"),
                task_id: "BD3-T3".parse().expect("task id"),
                target: MoveTarget::Head,
            })),
            AuthenticatedIngress::Local,
            RequestDeadline::after(Duration::from_secs(10)),
        )
        .await
        .expect("direct move");
    router
        .dispatch(
            ApiRequest::new(RequestEnvelope::TaskMove(TaskMoveRequest {
                caller_identity: "recipient".parse().expect("agent"),
                caller_team: TEAM.parse().expect("team"),
                task_id: "BD3-NO-MOVE".parse().expect("task id"),
                target: MoveTarget::Head,
            })),
            AuthenticatedIngress::Local,
            RequestDeadline::after(Duration::from_secs(10)),
        )
        .await
        .expect_err("moving an unknown task is refused");

    let t2_rows_before_ack = task_rows(&fixture, "BD3-T2");
    let acknowledgement = atm_core::ack::AckRequest {
        home_dir: fixture.home_dir.clone(),
        current_dir: fixture.current_dir.clone(),
        caller_identity: "recipient".parse().expect("recipient"),
        caller_chat_id: None,
        caller_team: TEAM.parse().expect("team"),
        activity_observation: None,
        message_id: t2,
        reply_body: "on it".to_owned(),
    }
    .into_write_request();
    write(&router, acknowledgement)
        .await
        .expect("acknowledge T2");
    assert_eq!(task_rows(&fixture, "BD3-T2"), t2_rows_before_ack);

    settle(&runtime).await;
    let records = sink.records();
    let tasks = [
        "BD3-T1",
        "BD3-MISSING",
        "BD3-T2",
        "BD3-T3",
        "BD3-T4",
        "BD3-NO-MOVE",
    ];
    let t1_rows = task_rows(&fixture, "BD3-T1");
    assert_literal_row_fields(
        &t1_rows[0],
        (
            1,
            "2030-01-01T00:00:01Z",
            None,
            Some(atm_core::boundary::TaskState::Assigned),
            None,
            atm_core::boundary::TaskActor::Member("sender".parse().expect("actor")),
            Some("00000000000000000000000001"),
        ),
    );
    assert_literal_row_fields(
        &t1_rows[1],
        (
            2,
            "2030-01-01T00:00:02Z",
            Some(atm_core::boundary::TaskState::Assigned),
            Some(atm_core::boundary::TaskState::Assigned),
            None,
            atm_core::boundary::TaskActor::Member("sender".parse().expect("actor")),
            Some("00000000000000000000000002"),
        ),
    );
    assert_literal_row_fields(
        &t1_rows[2],
        (
            3,
            "2030-01-01T00:00:03Z",
            Some(atm_core::boundary::TaskState::Assigned),
            Some(atm_core::boundary::TaskState::Active),
            None,
            atm_core::boundary::TaskActor::Member("third".parse().expect("actor")),
            Some("00000000000000000000000003"),
        ),
    );
    assert_literal_row_fields(
        &t1_rows[3],
        (
            4,
            "2030-01-01T00:00:04Z",
            Some(atm_core::boundary::TaskState::Active),
            Some(atm_core::boundary::TaskState::Complete(
                atm_storage::TaskCloseOutcome::Completed,
            )),
            Some(atm_storage::TaskCloseOutcome::Completed),
            atm_core::boundary::TaskActor::Member("third".parse().expect("actor")),
            Some("00000000000000000000000004"),
        ),
    );
    assert_literal_row_fields(
        &t1_rows[4],
        (
            5,
            "2030-01-01T00:00:05Z",
            Some(atm_core::boundary::TaskState::Complete(
                atm_storage::TaskCloseOutcome::Completed,
            )),
            Some(atm_core::boundary::TaskState::Assigned),
            None,
            atm_core::boundary::TaskActor::Member("sender".parse().expect("actor")),
            Some("00000000000000000000000005"),
        ),
    );
    assert_eq!(canonical(&records), durable_records(&fixture, &tasks).await);
    assert_eq!(
        runtime.diagnostics().snapshot().emitted,
        records.len() as u64
    );
    for task in tasks {
        let seqs: Vec<u64> = records
            .iter()
            .filter(|record| record.task_id.as_str() == task)
            .filter_map(|record| record.seq)
            .collect();
        let mut sorted = seqs.clone();
        sorted.sort_unstable();
        assert_eq!(seqs, sorted, "{task} rows arrive in durable seq order");
    }
    let t1: Vec<&str> = kinds(&records, "BD3-T1")
        .into_iter()
        .filter(|kind| *kind != "prompt_handoff")
        .collect();
    assert_eq!(
        t1,
        ["assigned", "reassigned", "started", "completed", "reopened"]
    );
    assert_eq!(kinds(&records, "BD3-MISSING"), ["rejected"]);
    assert!(kinds(&records, "BD3-T3").contains(&"moved"));
    assert!(
        kinds(&records, "BD3-T2")
            .iter()
            .all(|kind| *kind != "acked")
    );
    assert!(
        records
            .iter()
            .any(|record| record.kind == atm_core::TaskTelemetryKind::PromptHandoff),
        "a newly inserted handoff is projected"
    );
}

/// A replayed committed dispatch reaches the real router hook and SQLite
/// handoff writer, but its existing message key must not project a second
/// telemetry record or change the hook's successful caller result.
#[tokio::test]
async fn replayed_router_handoff_emits_nothing_for_the_existing_sqlite_key() {
    let fixture = fixture(true, None, None);
    let sink = RecordingTaskTelemetrySink::new();
    let runtime = recording_runtime(&sink);
    let router = fixture.router.clone().with_task_telemetry(runtime.clone());

    write(
        &router,
        assignment_request(&fixture, "sender", "recipient", "BD3-DUPLICATE"),
    )
    .await
    .expect("initial assignment succeeds");
    let dispatch = fixture
        .received_hook
        .dispatches
        .lock()
        .expect("recorded dispatch")
        .last()
        .cloned()
        .expect("assignment emitted a task-linked dispatch");

    let warnings = router
        .emit_received_hook(
            Ok(vec![dispatch]),
            RequestDeadline::after(Duration::from_secs(10)),
        )
        .await;
    assert!(
        warnings.is_empty(),
        "replayed handoff leaves the caller successful"
    );

    settle(&runtime).await;
    assert_eq!(prompt_handoffs(&fixture, "BD3-DUPLICATE").await.len(), 1);
    assert_eq!(
        kinds(&sink.records(), "BD3-DUPLICATE"),
        ["assigned", "prompt_handoff"],
        "the existing SQLite handoff emits no second projection"
    );
}

/// A real SQLite handoff failure is observational only: the committed
/// assignment still succeeds, while no handoff row or handoff record exists.
/// The failure is installed before the first dispatch so the insert is reached
/// (a replayed existing key never attempts one).
#[tokio::test]
async fn router_handoff_sqlite_failure_emits_nothing_and_keeps_the_write_successful() {
    let fixture = fixture(true, None, None);
    atm_runtime_test_support::install_sqlite_prompt_handoff_write_failure(&fixture.database_path)
        .expect("install deterministic prompt-handoff failure");
    let sink = RecordingTaskTelemetrySink::new();
    let runtime = recording_runtime(&sink);
    let router = fixture.router.clone().with_task_telemetry(runtime.clone());

    write(
        &router,
        assignment_request(&fixture, "sender", "recipient", "BD3-HANDOFF-FAIL"),
    )
    .await
    .expect("handoff recording failure does not change the caller result");

    settle(&runtime).await;
    assert!(
        prompt_handoffs(&fixture, "BD3-HANDOFF-FAIL")
            .await
            .is_empty(),
        "the failed insert leaves no durable handoff"
    );
    assert_eq!(
        kinds(&sink.records(), "BD3-HANDOFF-FAIL"),
        ["assigned"],
        "only the committed assignment projects; the failed handoff emits nothing"
    );
}

#[derive(Debug, PartialEq)]
struct Observed {
    responses: Vec<Result<&'static str, atm_storage::AtmErrorCode>>,
    kinds: Vec<&'static str>,
}

async fn drive(runtime: TaskTelemetryRuntime) -> Observed {
    let fixture = fixture(true, None, None);
    let router = fixture.router.clone().with_task_telemetry(runtime);
    let requests = vec![
        assignment_request(&fixture, "sender", "recipient", "BD3-S1"),
        task_op_request(
            &fixture,
            "recipient",
            "sender",
            "BD3-S1",
            atm_storage::TaskOp::Start,
        ),
        task_op_request(
            &fixture,
            "recipient",
            "sender",
            "BD3-ABSENT",
            close(atm_storage::TaskCloseOutcome::Completed),
        ),
        task_op_request(
            &fixture,
            "recipient",
            "sender",
            "BD3-S1",
            close(atm_storage::TaskCloseOutcome::Refused),
        ),
    ];
    let mut responses = Vec::new();
    for request in requests {
        responses.push(match write(&router, request).await {
            Ok(ResponseEnvelope::Send(_)) => Ok("send"),
            Ok(other) => panic!("unexpected response {other:?}"),
            Err(error) => Err(error.code()),
        });
    }
    let mut kinds: Vec<&'static str> = ["BD3-S1", "BD3-ABSENT"]
        .iter()
        .flat_map(|task| task_rows(&fixture, task))
        .map(|row| row.event.as_str())
        .collect();
    kinds.sort_unstable();
    Observed { responses, kinds }
}

/// A failing, a stalled-and-full, and a disabled telemetry runtime leave the
/// caller's responses and the durable ledger identical, and no handler waits
/// on the sink (the stalled sink is held until after the bound). The stalled
/// sink holds a worker thread, so this runs on a multi-thread runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failing_stalled_and_full_sinks_never_change_the_task_outcome() {
    let baseline = drive(TaskTelemetryRuntime::disabled()).await;
    assert_eq!(
        baseline.responses[2],
        Err(atm_storage::AtmErrorCode::TaskNotFound)
    );

    let failing = Arc::new(RecordingTaskTelemetrySink::answering(Err(
        atm_core::TaskTelemetryError::Unavailable,
    )));
    let failing_runtime = recording_runtime(&failing);
    assert_eq!(drive(failing_runtime.clone()).await, baseline);
    settle(&failing_runtime).await;
    let failing_counts = failing_runtime.diagnostics().snapshot();
    assert!(failing_counts.dropped_failure > 0, "{failing_counts:?}");
    assert_eq!(failing_counts.emitted, 0);

    let (stalled_sink, release) = StalledTaskTelemetrySink::new();
    let stalled_runtime = TaskTelemetryRuntime::start(
        TaskTelemetryConfig {
            queue_capacity: 1,
            drain_timeout: Duration::from_millis(50),
        },
        Arc::new(stalled_sink),
    );
    let stalled = tokio::time::timeout(Duration::from_secs(20), drive(stalled_runtime.clone()))
        .await
        .expect("handlers never await a stalled sink");
    assert_eq!(stalled, baseline);
    let stalled_counts = stalled_runtime.diagnostics().snapshot();
    assert!(!stalled_counts.config_invalid);
    assert!(stalled_counts.dropped_full > 0, "{stalled_counts:?}");
    drop(release);
    settle(&stalled_runtime).await;
}

/// Concurrent commits may enqueue in any order; each record still carries
/// its own durable seq and timestamp, and none is lost or renumbered.
#[tokio::test]
async fn concurrent_commits_keep_their_durable_seq_and_timestamp() {
    let fixture = fixture(true, None, None);
    let sink = RecordingTaskTelemetrySink::new();
    let runtime = recording_runtime(&sink);
    let router = fixture.router.clone().with_task_telemetry(runtime.clone());
    let tasks = ["BD3-C1", "BD3-C2", "BD3-C3", "BD3-C4"];
    let writes = tasks.map(|task| {
        let router = router.clone();
        let request = assignment_request(&fixture, "sender", "recipient", task);
        tokio::spawn(async move { write(&router, request).await })
    });
    for handle in writes {
        handle.await.expect("join").expect("concurrent assignment");
    }
    settle(&runtime).await;
    assert_eq!(
        canonical(&sink.records()),
        durable_records(&fixture, &tasks).await
    );
}

/// Commits an assignment (seq 1) and then a start (seq 2) on one task through
/// the real router with no telemetry attached, and returns the durable rows.
async fn committed_assignment_then_start(
    fixture: &Fixture,
    task: &str,
) -> (
    atm_core::boundary::TaskEventRow,
    atm_core::boundary::TaskEventRow,
) {
    write(
        &fixture.router,
        assignment_request(fixture, "sender", "recipient", task),
    )
    .await
    .expect("assign");
    write(
        &fixture.router,
        task_op_request(
            fixture,
            "recipient",
            "sender",
            task,
            atm_storage::TaskOp::Start,
        ),
    )
    .await
    .expect("start");
    let rows = task_rows(fixture, task);
    assert_eq!(rows.len(), 2, "assignment then start: {rows:?}");
    (rows[0].clone(), rows[1].clone())
}

/// Enqueue reversal: B commits after A but its rows reach the runtime first.
/// The sink sees B before A, and each record still carries its own durable
/// seq and timestamp; none is renumbered, re-timestamped or dropped.
#[tokio::test]
async fn reversed_enqueue_order_keeps_each_records_own_seq_and_timestamp() {
    let fixture = fixture(true, None, None);
    let (a, b) = committed_assignment_then_start(&fixture, "BD3-REV").await;
    assert!(a.seq < b.seq, "A committed before B");
    let sink = RecordingTaskTelemetrySink::new();
    let runtime = recording_runtime(&sink);
    crate::task_telemetry::project_task_events(&runtime, std::slice::from_ref(&b));
    crate::task_telemetry::project_task_events(&runtime, std::slice::from_ref(&a));
    settle(&runtime).await;
    let records = sink.records();
    assert_eq!(
        records,
        vec![record_from_event(&b), record_from_event(&a)],
        "delivery order is the reversed enqueue order, one record per row"
    );
    assert_eq!(
        records.iter().map(|record| record.seq).collect::<Vec<_>>(),
        vec![Some(b.seq), Some(a.seq)]
    );
    assert_eq!(
        records.iter().map(|record| record.at).collect::<Vec<_>>(),
        vec![b.at, a.at]
    );
}

/// Missing delivery: a committed row that never reaches the runtime leaves a
/// visible gap. The delivered records keep their own seq; nothing is
/// renumbered to close the gap and the absent row is not fabricated.
#[tokio::test]
async fn a_missing_seq_stays_a_gap_and_survivors_are_not_renumbered() {
    let fixture = fixture(true, None, None);
    let task = "BD3-GAP";
    let (a, b) = committed_assignment_then_start(&fixture, task).await;
    write(
        &fixture.router,
        task_op_request(
            &fixture,
            "recipient",
            "sender",
            task,
            close(atm_storage::TaskCloseOutcome::Completed),
        ),
    )
    .await
    .expect("close");
    let rows = task_rows(&fixture, task);
    assert_eq!(rows.len(), 3);
    let c = rows[2].clone();
    assert_eq!((a.seq, b.seq, c.seq), (1, 2, 3));
    let sink = RecordingTaskTelemetrySink::new();
    let runtime = recording_runtime(&sink);
    // B's projection is lost: only A and C are delivered.
    crate::task_telemetry::project_task_events(&runtime, &[a.clone(), c.clone()]);
    settle(&runtime).await;
    let records = sink.records();
    assert_eq!(records, vec![record_from_event(&a), record_from_event(&c)]);
    assert_eq!(
        records.iter().map(|record| record.seq).collect::<Vec<_>>(),
        vec![Some(1), Some(3)],
        "the gap at seq 2 is preserved, not renumbered"
    );
    assert_eq!(
        records.iter().map(|record| record.at).collect::<Vec<_>>(),
        vec![a.at, c.at]
    );
}
