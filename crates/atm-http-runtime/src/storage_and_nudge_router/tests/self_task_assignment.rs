//! A lead dispatching a task to itself: the task is recorded and tracked
//! through the real router and SQLite ledger, and nothing is delivered to the
//! caller. A plain self-addressed send stays invalid.

use super::*;

const TEAM: &str = "test-team";

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

fn self_task_request(
    fixture: &Fixture,
    task_id: &str,
    op: Option<atm_storage::TaskOp>,
) -> WriteRequest {
    let mut request = assignment_request(fixture, "sender", "sender", task_id);
    request.task_op = op;
    request
}

/// Positive: `sender` assigns, starts and closes a task addressed to itself;
/// the durable events show the full history and the receiver hook builds no
/// dispatch for any of the three writes.
/// Negative: a plain self-addressed send (no task id) is still rejected.
#[tokio::test]
async fn self_addressed_task_assignment_is_tracked_without_delivery() {
    let fixture = fixture(true, None, None);

    write(&fixture.router, self_task_request(&fixture, "SELF-1", None))
        .await
        .expect("self-addressed task assignment is accepted");
    write(
        &fixture.router,
        self_task_request(&fixture, "SELF-1", Some(atm_storage::TaskOp::Start)),
    )
    .await
    .expect("the caller starts its own task");
    write(
        &fixture.router,
        self_task_request(
            &fixture,
            "SELF-1",
            Some(atm_storage::TaskOp::Close {
                outcome: atm_storage::TaskCloseOutcome::Completed,
                reason: None,
            }),
        ),
    )
    .await
    .expect("the caller closes its own task");

    let events: Vec<_> = fixture
        .task_store
        .list_task_events(
            &TEAM.parse().expect("team"),
            &"SELF-1".parse().expect("task id"),
            None,
        )
        .expect("durable task events")
        .iter()
        .map(|row| row.event)
        .collect();
    assert_eq!(
        events,
        [
            atm_storage::TaskEventKind::Assigned,
            atm_storage::TaskEventKind::Started,
            atm_storage::TaskEventKind::Completed,
        ]
    );
    assert!(
        fixture
            .received_hook
            .dispatches
            .lock()
            .expect("dispatches")
            .is_empty(),
        "no nudge or prompt is built for the caller's own task"
    );

    let mut plain = self_task_request(&fixture, "SELF-PLAIN", None);
    plain.task_id = None;
    let error = write(&fixture.router, plain)
        .await
        .expect_err("a plain self-addressed send stays invalid");
    assert_eq!(
        error.code(),
        atm_storage::AtmErrorCode::SelfAddressedSendInvalid
    );
}
