//! L2.2/L2.5 coverage: `NudgeMode::Deferred` suppresses the immediate
//! receiver dispatch and sets exactly one durable queue marker via
//! the async router's blocking marker seam; `NudgeMode::Immediate` is byte-identical to the
//! pre-AQ1 dispatch; a duplicate (idempotent) write never sets a second
//! marker.

use std::sync::{Arc, Mutex};

use atm_core::boundary::{
    MemberKey, NudgeClaim, NudgeKind, PendingNudgeStore, PostSendBuiltInTarget, PostSendHookEvent,
    RosterHarness, RosterMemberKind, built_in_nudge_template_kind_from_post_send_event,
};
use atm_core::error::AtmError;
use atm_core::observability::NullObservability;
use atm_core::schema::AtmMessageId;
use atm_core::send::{
    NudgeMode, SendMessageSource, WriteRequest, prepare_write_with_runtime, write_mail_with_runtime,
};
use atm_core::types::{AgentName, IsoTimestamp, ModelName, PaneId, TaskId, TeamName};

/// Records every `mark_pending` call; every other method is a trivial no-op
/// since this suite never exercises the durable claim/requeue lifecycle.
#[derive(Default)]
struct RecordingPendingNudgeStore {
    mark_pending_calls: Mutex<Vec<(MemberKey, AtmMessageId)>>,
    fail_mark_pending: bool,
}

impl RecordingPendingNudgeStore {
    fn mark_pending_call_count(&self) -> usize {
        self.mark_pending_calls
            .lock()
            .expect("mark_pending calls lock")
            .len()
    }
}

impl atm_storage::contract::sealed::Sealed for RecordingPendingNudgeStore {}

impl PendingNudgeStore for RecordingPendingNudgeStore {
    fn mark_pending(
        &self,
        member: &MemberKey,
        msg: &AtmMessageId,
        _at: IsoTimestamp,
    ) -> Result<bool, AtmError> {
        self.mark_pending_calls
            .lock()
            .expect("mark_pending calls lock")
            .push((member.clone(), *msg));
        if self.fail_mark_pending {
            return Err(AtmError::mailbox_write(
                "pending nudge test store rejected marker",
            ));
        }
        Ok(true)
    }

    fn claim_next_pending(&self, _member: &MemberKey) -> Result<Option<NudgeClaim>, AtmError> {
        Ok(None)
    }

    fn requeue_pending(&self, _member: &MemberKey, _claim: &NudgeClaim) -> Result<(), AtmError> {
        Ok(())
    }

    fn release_pending(&self, _member: &MemberKey, _claim: &NudgeClaim) -> Result<(), AtmError> {
        Ok(())
    }

    fn clear_pending_on_read(
        &self,
        _member: &MemberKey,
        _msg: &AtmMessageId,
    ) -> Result<(), AtmError> {
        Ok(())
    }

    fn clear_pending_on_handoff(
        &self,
        _member: &MemberKey,
        _msg: &AtmMessageId,
    ) -> Result<(), AtmError> {
        Ok(())
    }

    fn list_pending_members(&self) -> Result<Vec<MemberKey>, AtmError> {
        Ok(Vec::new())
    }
}

fn setup() -> (
    tempfile::TempDir,
    atm_core::LocalServiceRuntime,
    Arc<RecordingPendingNudgeStore>,
    TeamName,
) {
    setup_with_store(false)
}

fn setup_with_store(
    fail_mark_pending: bool,
) -> (
    tempfile::TempDir,
    atm_core::LocalServiceRuntime,
    Arc<RecordingPendingNudgeStore>,
    TeamName,
) {
    let root = tempfile::tempdir().expect("temp root");
    let assembly = atm_runtime_test_support::open_isolated_sqlite_boundary(root.path())
        .expect("sqlite runtime");
    let recording_store = Arc::new(RecordingPendingNudgeStore {
        fail_mark_pending,
        ..RecordingPendingNudgeStore::default()
    });
    let runtime = assembly
        .service_runtime
        .with_pending_nudge_store(recording_store.clone());
    let team: TeamName = "test-team".parse().expect("team");
    runtime
        .shared_roster_store_arc()
        .save_roster(&atm_storage::RosterSnapshot {
            team_name: team.clone(),
            members: vec![
                roster_member(&team, "sender", None),
                roster_member(
                    &team,
                    "recipient",
                    Some(PaneId::from_cli("%9").expect("pane")),
                ),
            ],
            refreshed_at: None,
        })
        .expect("seed roster");
    (root, runtime, recording_store, team)
}

fn roster_member(
    team: &TeamName,
    agent: &str,
    pane_id: Option<PaneId>,
) -> atm_core::boundary::RosterEntry {
    atm_core::boundary::RosterEntry {
        team_name: team.clone(),
        agent_name: agent.parse().expect("agent"),
        member_kind: RosterMemberKind::Permanent,
        harness: RosterHarness::PythonGraft,
        agent_type: atm_core::schema::AgentType::default(),
        model: ModelName::default(),
        recipient_pane_id: pane_id,
        metadata_json: serde_json::Map::new(),
    }
}

fn write_request(
    home_dir: &std::path::Path,
    team: &TeamName,
    nudge_mode: NudgeMode,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
) -> WriteRequest {
    WriteRequest::new(
        home_dir.to_path_buf(),
        home_dir.to_path_buf(),
        "sender".parse::<AgentName>().expect("sender"),
        "recipient@test-team",
        team.clone(),
        SendMessageSource::Inline("nudge mode fixture".to_owned()),
        None,
        false,
        None,
        false,
    )
    .expect("write request")
    .with_nudge_mode(nudge_mode)
    .with_origin_metadata(message_id, timestamp)
}

fn task_write_request(
    home_dir: &std::path::Path,
    team: &TeamName,
    nudge_mode: NudgeMode,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
) -> WriteRequest {
    let mut request = write_request(home_dir, team, nudge_mode, message_id, timestamp);
    request.task_id = Some("task-ax1".parse::<TaskId>().expect("task id"));
    request
}

#[test]
fn deferred_write_suppresses_dispatch_and_sets_exactly_one_marker() {
    let (root, runtime, recording_store, team) = setup();
    let home_dir = root.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("home dir");
    let message_id = AtmMessageId::new();
    let timestamp = IsoTimestamp::now();

    let request = write_request(&home_dir, &team, NudgeMode::Deferred, message_id, timestamp);
    let mut prepared =
        prepare_write_with_runtime(request, &NullObservability, &runtime).expect("prepare write");
    let dispatches = prepared
        .build_received_hook_dispatches(&runtime)
        .expect("dispatches");
    assert!(
        dispatches.is_empty(),
        "a deferred write must suppress its immediate receiver dispatch"
    );

    prepared
        .finish(&runtime, &NullObservability)
        .expect("finish");
    let _ = prepared.mark_pending_if_deferred(&runtime);
    assert_eq!(
        recording_store.mark_pending_call_count(),
        1,
        "finishing a newly persisted deferred write must set exactly one queue marker"
    );
}

/// AQ2.5 AC3: a deferred write to a bare-CLI recipient must retain the
/// queue-pull dispatch so the daemon can append the message to its RAM FIFO.
#[test]
fn deferred_bare_cli_write_builds_queue_pull_dispatch() {
    let (root, runtime, _recording_store, team) = setup();
    let home_dir = root.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("home dir");
    runtime
        .shared_roster_store_arc()
        .save_roster(&atm_storage::RosterSnapshot {
            team_name: team.clone(),
            members: vec![
                roster_member(&team, "sender", None),
                roster_member(&team, "recipient", None),
            ],
            refreshed_at: None,
        })
        .expect("seed bare-CLI roster");

    let request = write_request(
        &home_dir,
        &team,
        NudgeMode::Deferred,
        AtmMessageId::new(),
        IsoTimestamp::now(),
    );
    let prepared =
        prepare_write_with_runtime(request, &NullObservability, &runtime).expect("prepare write");
    let dispatches = prepared
        .build_received_hook_dispatches(&runtime)
        .expect("dispatches");

    assert_eq!(
        dispatches.len(),
        1,
        "bare-CLI deferred writes need one queue handoff"
    );
    assert!(matches!(
        dispatches[0].target,
        atm_core::boundary::PostSendBuiltInTarget::QueuePull(_)
    ));
    assert_eq!(dispatches[0].kind, atm_core::boundary::NudgeKind::Queue);
}

/// AQ2.5 AC3: an immediate (steer-kind) write to a bare-CLI recipient must
/// use the same QueuePull handoff so the Stop hook can drain it with the
/// other steer-kind FIFO entries.
#[test]
fn steer_bare_cli_write_builds_queue_pull_dispatch() {
    let (root, runtime, _recording_store, team) = setup();
    let home_dir = root.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("home dir");
    runtime
        .shared_roster_store_arc()
        .save_roster(&atm_storage::RosterSnapshot {
            team_name: team.clone(),
            members: vec![
                roster_member(&team, "sender", None),
                roster_member(&team, "recipient", None),
            ],
            refreshed_at: None,
        })
        .expect("seed bare-CLI roster");

    let request = write_request(
        &home_dir,
        &team,
        NudgeMode::Immediate,
        AtmMessageId::new(),
        IsoTimestamp::now(),
    );
    let prepared =
        prepare_write_with_runtime(request, &NullObservability, &runtime).expect("prepare write");
    let dispatches = prepared
        .build_received_hook_dispatches(&runtime)
        .expect("dispatches");

    assert_eq!(
        dispatches.len(),
        1,
        "bare-CLI steer writes need one FIFO handoff"
    );
    assert!(matches!(
        dispatches[0].target,
        atm_core::boundary::PostSendBuiltInTarget::QueuePull(_)
    ));
    assert_eq!(dispatches[0].kind, atm_core::boundary::NudgeKind::Steer);
}

#[test]
fn immediate_write_dispatch_is_unchanged_and_sets_no_marker() {
    let (root, runtime, recording_store, team) = setup();
    let home_dir = root.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("home dir");
    let message_id = AtmMessageId::new();
    let timestamp = IsoTimestamp::now();

    let request = write_request(
        &home_dir,
        &team,
        NudgeMode::Immediate,
        message_id,
        timestamp,
    );
    let mut prepared =
        prepare_write_with_runtime(request, &NullObservability, &runtime).expect("prepare write");
    let dispatches = prepared
        .build_received_hook_dispatches(&runtime)
        .expect("dispatches");
    assert_eq!(
        dispatches.len(),
        1,
        "an immediate write must retain its historical single receiver dispatch"
    );
    assert!(matches!(
        dispatches[0].target,
        PostSendBuiltInTarget::LocalSteer(_)
    ));

    prepared
        .finish(&runtime, &NullObservability)
        .expect("finish");
    let _ = prepared.mark_pending_if_deferred(&runtime);
    assert_eq!(
        recording_store.mark_pending_call_count(),
        0,
        "an immediate write must never set a durable queue marker"
    );
}

#[test]
fn duplicate_deferred_write_sets_no_second_marker() {
    let (root, runtime, recording_store, team) = setup();
    let home_dir = root.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("home dir");
    let message_id = AtmMessageId::new();
    let timestamp = IsoTimestamp::now();

    for _ in 0..2 {
        let request = write_request(&home_dir, &team, NudgeMode::Deferred, message_id, timestamp);
        let mut prepared = prepare_write_with_runtime(request, &NullObservability, &runtime)
            .expect("prepare write");
        prepared
            .build_received_hook_dispatches(&runtime)
            .expect("dispatches");
        prepared
            .finish(&runtime, &NullObservability)
            .expect("finish");
        let _ = prepared.mark_pending_if_deferred(&runtime);
    }

    assert_eq!(
        recording_store.mark_pending_call_count(),
        1,
        "an idempotent duplicate write must not set a second queue marker"
    );
}

/// QM40v2-I1 regression: the synchronous public write API
/// (`write_mail_with_runtime`) must set the durable queue marker for a
/// newly persisted `NudgeMode::Deferred` write on its own, since it has no
/// async router to schedule `mark_pending_if_deferred` separately.
#[test]
fn sync_write_mail_with_runtime_sets_marker_for_deferred_write() {
    let (root, runtime, recording_store, team) = setup();
    let home_dir = root.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("home dir");
    let message_id = AtmMessageId::new();
    let timestamp = IsoTimestamp::now();

    let request = write_request(&home_dir, &team, NudgeMode::Deferred, message_id, timestamp);
    write_mail_with_runtime(request, &NullObservability, &runtime).expect("write mail");

    assert_eq!(
        recording_store.mark_pending_call_count(),
        1,
        "the synchronous write API must set exactly one queue marker for a \
         newly persisted deferred write"
    );
}

/// Companion to the above: an immediate write through the same synchronous
/// entry point must never set a queue marker.
#[test]
fn sync_write_mail_with_runtime_sets_no_marker_for_immediate_write() {
    let (root, runtime, recording_store, team) = setup();
    let home_dir = root.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("home dir");
    let message_id = AtmMessageId::new();
    let timestamp = IsoTimestamp::now();

    let request = write_request(
        &home_dir,
        &team,
        NudgeMode::Immediate,
        message_id,
        timestamp,
    );
    write_mail_with_runtime(request, &NullObservability, &runtime).expect("write mail");

    assert_eq!(
        recording_store.mark_pending_call_count(),
        0,
        "an immediate write through the synchronous entry point must never \
         set a durable queue marker"
    );
}

#[test]
fn failing_marker_store_does_not_fail_the_deferred_write() {
    let (root, runtime, failing_store, team) = setup_with_store(true);
    let home_dir = root.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("home dir");
    let request = write_request(
        &home_dir,
        &team,
        NudgeMode::Deferred,
        AtmMessageId::new(),
        IsoTimestamp::now(),
    );
    let mut prepared =
        prepare_write_with_runtime(request, &NullObservability, &runtime).expect("prepare write");

    prepared
        .finish(&runtime, &NullObservability)
        .expect("a marker failure must not fail durable write");
    let _ = prepared.mark_pending_if_deferred(&runtime);

    assert_eq!(
        failing_store.mark_pending_call_count(),
        1,
        "the failing store double must exercise the marker error path"
    );
}

#[test]
fn task_tagged_sync_prepare_forces_deferred_mode() {
    let (root, runtime, _recording_store, team) = setup();
    let home_dir = root.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("home dir");
    let request = task_write_request(
        &home_dir,
        &team,
        NudgeMode::Immediate,
        AtmMessageId::new(),
        IsoTimestamp::now(),
    );

    let prepared =
        prepare_write_with_runtime(request, &NullObservability, &runtime).expect("prepare write");
    assert_eq!(
        prepared.outbound_request().nudge_mode,
        NudgeMode::Deferred,
        "task-tagged sync writes are always queued"
    );
}

#[tokio::test]
async fn task_tagged_async_prepare_forces_deferred_mode() {
    let (root, runtime, _recording_store, team) = setup();
    let home_dir = root.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("home dir");
    let request = task_write_request(
        &home_dir,
        &team,
        NudgeMode::Immediate,
        AtmMessageId::new(),
        IsoTimestamp::now(),
    );

    let prepared =
        atm_core::send::prepare_write_with_async_runtime(request, &NullObservability, &runtime)
            .await
            .expect("prepare async write");
    assert_eq!(
        prepared.outbound_request().nudge_mode,
        NudgeMode::Deferred,
        "task-tagged async writes are always queued"
    );
}

#[test]
fn tmux_and_herdr_delivery_families_share_the_nudge_kind_table() {
    let event = |requires_ack, task_id| PostSendHookEvent {
        sender: "sender".parse().expect("sender"),
        sender_chat_id: None,
        sender_team: "test-team".parse().expect("sender team"),
        sender_host: None,
        recipient: "recipient".parse().expect("recipient"),
        recipient_team: "test-team".parse().expect("recipient team"),
        message_id: AtmMessageId::new(),
        description: "fixture".to_owned(),
        requires_ack,
        is_ack: false,
        task_id,
        recipient_pane_id: None,
    };
    let task_id = Some("task-ax1".parse::<TaskId>().expect("task id"));
    for backend in ["tmux", "herdr"] {
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(
                &event(false, None),
                NudgeKind::Steer,
            ),
            atm_core::boundary::BuiltInNudgeTemplateKind::Delivery,
            "{backend} Delivery"
        );
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event(true, None), NudgeKind::Steer,),
            atm_core::boundary::BuiltInNudgeTemplateKind::DeliveryAck,
            "{backend} DeliveryAck"
        );
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(
                &event(false, None),
                NudgeKind::Queue,
            ),
            atm_core::boundary::BuiltInNudgeTemplateKind::Queue,
            "{backend} Queue"
        );
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event(true, None), NudgeKind::Queue,),
            atm_core::boundary::BuiltInNudgeTemplateKind::QueueAck,
            "{backend} QueueAck"
        );
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(
                &event(true, task_id.clone()),
                NudgeKind::Queue,
            ),
            atm_core::boundary::BuiltInNudgeTemplateKind::Task,
            "{backend} task"
        );
    }
}

#[test]
fn acknowledge_default_templates_match_recorded_pre_ax1_fixtures() {
    assert_eq!(
        atm_core::send::default_template(atm_core::boundary::BuiltInNudgeTemplateKind::Acknowledge),
        "<atm kind=\"ack\" from=\"{{from}}\" message-id=\"{{message_id}}\"/>"
    );
    assert_eq!(
        atm_core::send::default_template(
            atm_core::boundary::BuiltInNudgeTemplateKind::AcknowledgeTask
        ),
        "<atm kind=\"ack\" from=\"{{from}}\" message-id=\"{{message_id}}\" task-id=\"{{task_id}}\"/>"
    );
}
