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
        assignment_request(&fixture, "sender", "recipient", "BD3-T1"),
    )
    .await
    .expect("assign");
    write(
        &router,
        assignment_request(&fixture, "sender", "third", "BD3-T1"),
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
        ),
    )
    .await
    .expect("close");
    write(
        &router,
        assignment_request(&fixture, "sender", "third", "BD3-T1"),
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
/// on the sink (the stalled emit timeout is far longer than the bound).
#[tokio::test]
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

    let stalled_runtime = TaskTelemetryRuntime::start(
        TaskTelemetryConfig {
            queue_capacity: 1,
            emit_timeout: Duration::from_secs(30),
            drain_timeout: Duration::from_millis(50),
        },
        Arc::new(StalledTaskTelemetrySink),
    );
    let stalled = tokio::time::timeout(Duration::from_secs(20), drive(stalled_runtime.clone()))
        .await
        .expect("handlers never await a stalled sink");
    assert_eq!(stalled, baseline);
    let stalled_counts = stalled_runtime.diagnostics().snapshot();
    assert!(!stalled_counts.config_invalid);
    assert!(stalled_counts.dropped_full > 0, "{stalled_counts:?}");
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
