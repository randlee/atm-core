//! L2.2/L2.5 coverage: `NudgeMode::Deferred` suppresses the immediate
//! receiver dispatch and sets exactly one durable queue marker via
//! the async router's blocking marker seam; `NudgeMode::Immediate` is byte-identical to the
//! pre-AQ1 dispatch; a duplicate (idempotent) write never sets a second
//! marker.

use std::sync::{Arc, Mutex};

use atm_core::boundary::{
    MemberKey, NudgeClaim, NudgeKind, PendingNudgeStore, PostSendBuiltInTarget, RosterHarness,
    RosterMemberKind, built_in_nudge_template_kind_from_post_send_event,
};
use atm_core::error::AtmError;
use atm_core::nudge_dispatch::rebuild_received_hook_dispatch;
use atm_core::observability::NullObservability;
use atm_core::schema::AtmMessageId;
use atm_core::send::{
    NudgeMode, SendMessageSource, WriteRequest, prepare_write_with_runtime, write_mail_with_runtime,
};
use atm_core::types::{AgentName, IsoTimestamp, ModelName, PaneId, TaskId, TeamName};

#[derive(Default)]
struct InMemoryAsyncStore;

impl atm_storage::contract::sealed::Sealed for InMemoryAsyncStore {}

impl atm_storage::MessageStore for InMemoryAsyncStore {
    fn save_message(&self, _message: &atm_storage::Message) -> Result<(), AtmError> {
        Ok(())
    }

    fn save_messages_atomically(&self, _messages: &[atm_storage::Message]) -> Result<(), AtmError> {
        Ok(())
    }

    fn load_message(
        &self,
        _key: &atm_storage::MessageKey,
    ) -> Result<Option<atm_storage::Message>, AtmError> {
        Ok(None)
    }

    fn list_messages(
        &self,
        _query: &atm_storage::MessageQuery,
    ) -> Result<Vec<atm_storage::Message>, AtmError> {
        Ok(Vec::new())
    }

    fn delete_message(&self, _key: &atm_storage::MessageKey) -> Result<(), AtmError> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl atm_storage::AsyncMessageStore for InMemoryAsyncStore {}

/// Minimal executor matching the core's async admission tests. This fixture's
/// in-memory async store never yields, so no Tokio runtime is needed.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(output) => return output,
            std::task::Poll::Pending => std::thread::yield_now(),
        }
    }
}

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
    let team: TeamName = "test-team".parse().expect("team");
    setup_with_roster(
        fail_mark_pending,
        vec![
            roster_member(&team, "sender", None),
            roster_member(
                &team,
                "recipient",
                Some(PaneId::from_cli("%9").expect("pane")),
            ),
        ],
        false,
    )
}

fn setup_with_roster(
    fail_mark_pending: bool,
    members: Vec<atm_core::boundary::RosterEntry>,
    register_graft: bool,
) -> (
    tempfile::TempDir,
    atm_core::LocalServiceRuntime,
    Arc<RecordingPendingNudgeStore>,
    TeamName,
) {
    let root = tempfile::tempdir().expect("temp root");
    let assembly = atm_runtime_test_support::open_isolated_sqlite_boundary(root.path())
        .expect("sqlite runtime");
    let async_store = Arc::new(InMemoryAsyncStore);
    let recording_store = Arc::new(RecordingPendingNudgeStore {
        fail_mark_pending,
        ..RecordingPendingNudgeStore::default()
    });
    let mut runtime = assembly
        .service_runtime
        .with_async_message_store(async_store)
        .with_pending_nudge_store(recording_store.clone());
    let team: TeamName = "test-team".parse().expect("team");
    if register_graft {
        let endpoint_store = atm_runtime_test_support::open_graft_receiver_endpoint_store(
            root.path().join("runtime").join("mail.sqlite3"),
        )
        .expect("graft endpoint store");
        endpoint_store
            .register(
                &atm_storage::GraftReceiverRegistration {
                    team: team.clone(),
                    agent: "graft-recipient".parse().expect("graft recipient"),
                    endpoint: "127.0.0.1:9".parse().expect("endpoint"),
                    capability: atm_core::local_http::LocalCapability::generate()
                        .expect("capability"),
                    owner_generation: atm_storage::OwnerGeneration::new(
                        "01J00000000000000000000000",
                    )
                    .expect("owner generation"),
                },
                IsoTimestamp::now().into_inner(),
            )
            .expect("register graft recipient");
        runtime = runtime.with_graft_receiver_endpoint_store(endpoint_store);
    }
    runtime
        .shared_roster_store_arc()
        .save_roster(&atm_storage::RosterSnapshot {
            team_name: team.clone(),
            members,
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

fn herdr_roster_member(team: &TeamName, agent: &str) -> atm_core::boundary::RosterEntry {
    let mut metadata = atm_core::delivery_channel::test_backend_type_metadata("herdr");
    metadata.insert("herdrSession".to_owned(), serde_json::json!("ax1-herdr"));
    atm_core::boundary::RosterEntry {
        team_name: team.clone(),
        agent_name: agent.parse().expect("agent"),
        member_kind: RosterMemberKind::Permanent,
        harness: RosterHarness::CodexCli,
        agent_type: atm_core::schema::AgentType::default(),
        model: ModelName::default(),
        recipient_pane_id: None,
        metadata_json: metadata,
    }
}

fn graft_roster_member(team: &TeamName) -> atm_core::boundary::RosterEntry {
    roster_member(team, "graft-recipient", None)
}

fn write_request(
    home_dir: &std::path::Path,
    team: &TeamName,
    nudge_mode: NudgeMode,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
) -> WriteRequest {
    write_request_for(
        home_dir,
        team,
        "recipient",
        WriteRequestOptions {
            nudge_mode,
            requires_ack: false,
            task_id: None,
        },
        message_id,
        timestamp,
    )
}

struct WriteRequestOptions {
    nudge_mode: NudgeMode,
    requires_ack: bool,
    task_id: Option<TaskId>,
}

fn write_request_for(
    home_dir: &std::path::Path,
    team: &TeamName,
    recipient: &str,
    options: WriteRequestOptions,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
) -> WriteRequest {
    WriteRequest::new(
        home_dir.to_path_buf(),
        home_dir.to_path_buf(),
        "sender".parse::<AgentName>().expect("sender"),
        &format!("{recipient}@{team}"),
        team.clone(),
        SendMessageSource::Inline("nudge mode fixture".to_owned()),
        None,
        options.requires_ack,
        options.task_id,
        false,
    )
    .expect("write request")
    .with_nudge_mode(options.nudge_mode)
    .with_origin_metadata(message_id, timestamp)
}

fn task_write_request(
    home_dir: &std::path::Path,
    team: &TeamName,
    nudge_mode: NudgeMode,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
) -> WriteRequest {
    write_request_for(
        home_dir,
        team,
        "recipient",
        WriteRequestOptions {
            nudge_mode,
            requires_ack: false,
            task_id: Some("task-ax1".parse::<TaskId>().expect("task id")),
        },
        message_id,
        timestamp,
    )
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

#[test]
fn task_tagged_async_prepare_forces_deferred_mode() {
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

    let prepared = block_on(atm_core::send::prepare_write_with_async_runtime(
        request,
        &NullObservability,
        &runtime,
    ))
    .expect("prepare async write");
    assert_eq!(
        prepared.outbound_request().nudge_mode,
        NudgeMode::Deferred,
        "task-tagged async writes are always queued"
    );
}

fn assert_local_target(dispatch: &atm_core::boundary::BuiltInPostSendDispatch, herdr: bool) {
    if herdr {
        assert!(matches!(
            dispatch.target,
            PostSendBuiltInTarget::LocalSteer(atm_core::boundary::LocalSteerTarget::Herdr(_))
        ));
    } else {
        assert!(matches!(
            dispatch.target,
            PostSendBuiltInTarget::LocalSteer(atm_core::boundary::LocalSteerTarget::Tmux(_))
        ));
    }
}

fn assert_herdr_rendered_default(
    dispatch: &atm_core::boundary::BuiltInPostSendDispatch,
    expected_kind: atm_core::boundary::BuiltInNudgeTemplateKind,
) {
    let PostSendBuiltInTarget::LocalSteer(atm_core::boundary::LocalSteerTarget::Herdr(target)) =
        &dispatch.target
    else {
        panic!("expected Herdr target");
    };
    let expected = atm_core::send::default_template(expected_kind)
        .replace("{{from}}", &dispatch.event.source_address().to_string())
        .replace("{{message_id}}", &dispatch.event.message_id.to_string())
        .replace("{{description}}", &dispatch.event.description)
        .replace(
            "{{task_id}}",
            dispatch
                .event
                .task_id
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default()
                .as_str(),
        );
    assert_eq!(target.rendered_nudge, expected);
}

fn assert_local_matrix(herdr: bool) {
    let team: TeamName = "test-team".parse().expect("team");
    let recipient = if herdr {
        herdr_roster_member(&team, "recipient")
    } else {
        roster_member(
            &team,
            "recipient",
            Some(PaneId::from_cli("%9").expect("pane")),
        )
    };
    let (root, runtime, recording_store, _) = setup_with_roster(
        false,
        vec![roster_member(&team, "sender", None), recipient],
        false,
    );
    let home_dir = root.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("home dir");

    let immediate = |requires_ack, expected_kind| {
        let mut prepared = prepare_write_with_runtime(
            write_request_for(
                &home_dir,
                &team,
                "recipient",
                WriteRequestOptions {
                    nudge_mode: NudgeMode::Immediate,
                    requires_ack,
                    task_id: None,
                },
                AtmMessageId::new(),
                IsoTimestamp::now(),
            ),
            &NullObservability,
            &runtime,
        )
        .expect("prepare immediate write");
        let dispatches = prepared
            .build_received_hook_dispatches(&runtime)
            .expect("build immediate dispatch");
        assert_eq!(dispatches.len(), 1);
        assert_local_target(&dispatches[0], herdr);
        if herdr {
            assert_herdr_rendered_default(&dispatches[0], expected_kind);
        }
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(
                &dispatches[0].event,
                dispatches[0].kind,
            ),
            expected_kind,
        );
        prepared
            .finish(&runtime, &NullObservability)
            .expect("finish immediate write");
    };
    immediate(
        false,
        atm_core::boundary::BuiltInNudgeTemplateKind::Delivery,
    );
    immediate(
        true,
        atm_core::boundary::BuiltInNudgeTemplateKind::DeliveryAck,
    );

    let queued = |requires_ack, expected_kind| {
        let message_id = AtmMessageId::new();
        let mut prepared = prepare_write_with_runtime(
            write_request_for(
                &home_dir,
                &team,
                "recipient",
                WriteRequestOptions {
                    nudge_mode: NudgeMode::Deferred,
                    requires_ack,
                    task_id: None,
                },
                message_id,
                IsoTimestamp::now(),
            ),
            &NullObservability,
            &runtime,
        )
        .expect("prepare queued write");
        assert!(
            prepared
                .build_received_hook_dispatches(&runtime)
                .expect("deferred local dispatch")
                .is_empty()
        );
        prepared
            .finish(&runtime, &NullObservability)
            .expect("finish queued write");
        prepared
            .mark_pending_if_deferred(&runtime)
            .expect("mark queued write");
        let member = MemberKey::new(team.clone(), "recipient".parse().expect("recipient"));
        let dispatch =
            rebuild_received_hook_dispatch(&runtime, &member, message_id, NudgeKind::Queue)
                .expect("rebuild queued dispatch")
                .expect("queued dispatch");
        assert_local_target(&dispatch, herdr);
        if herdr {
            assert_herdr_rendered_default(&dispatch, expected_kind);
        }
        assert_eq!(dispatch.kind, NudgeKind::Queue);
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&dispatch.event, dispatch.kind),
            expected_kind,
        );
    };
    queued(false, atm_core::boundary::BuiltInNudgeTemplateKind::Queue);
    queued(true, atm_core::boundary::BuiltInNudgeTemplateKind::QueueAck);

    let task = |mode| {
        let message_id = AtmMessageId::new();
        let mut prepared = prepare_write_with_runtime(
            write_request_for(
                &home_dir,
                &team,
                "recipient",
                WriteRequestOptions {
                    nudge_mode: mode,
                    requires_ack: false,
                    task_id: Some("task-ax1".parse().expect("task id")),
                },
                message_id,
                IsoTimestamp::now(),
            ),
            &NullObservability,
            &runtime,
        )
        .expect("prepare task write");
        assert_eq!(
            prepared.outbound_request().nudge_mode,
            NudgeMode::Deferred,
            "task writes are deferred for every local backend"
        );
        assert!(
            prepared
                .build_received_hook_dispatches(&runtime)
                .expect("deferred task dispatch")
                .is_empty()
        );
        prepared
            .finish(&runtime, &NullObservability)
            .expect("finish task write");
        prepared
            .mark_pending_if_deferred(&runtime)
            .expect("mark task write");
        let member = MemberKey::new(team.clone(), "recipient".parse().expect("recipient"));
        let dispatch =
            rebuild_received_hook_dispatch(&runtime, &member, message_id, NudgeKind::Queue)
                .expect("rebuild task dispatch")
                .expect("task dispatch");
        assert_local_target(&dispatch, herdr);
        assert_eq!(dispatch.kind, NudgeKind::Queue);
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&dispatch.event, dispatch.kind),
            atm_core::boundary::BuiltInNudgeTemplateKind::Task,
        );
    };
    task(NudgeMode::Immediate);
    task(NudgeMode::Deferred);

    assert_eq!(
        recording_store.mark_pending_call_count(),
        4,
        "every deferred local write sets one durable marker"
    );
}

#[test]
fn actual_dispatch_matrix_covers_tmux_and_herdr_members() {
    assert_local_matrix(false);
    assert_local_matrix(true);
}

fn assert_graft_task_dispatch(async_path: bool) {
    let team: TeamName = "test-team".parse().expect("team");
    let (root, runtime, _recording_store, _) = setup_with_roster(
        false,
        vec![
            roster_member(&team, "sender", None),
            graft_roster_member(&team),
        ],
        true,
    );
    let home_dir = root.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("home dir");
    let message_id = AtmMessageId::new();
    let request = write_request_for(
        &home_dir,
        &team,
        "graft-recipient",
        WriteRequestOptions {
            nudge_mode: NudgeMode::Immediate,
            requires_ack: false,
            task_id: Some("task-ax1".parse().expect("task id")),
        },
        message_id,
        IsoTimestamp::now(),
    );
    let mut prepared = if async_path {
        block_on(atm_core::send::prepare_write_with_async_runtime(
            request,
            &NullObservability,
            &runtime,
        ))
        .expect("prepare async graft task write")
    } else {
        prepare_write_with_runtime(request, &NullObservability, &runtime)
            .expect("prepare sync graft task write")
    };
    assert_eq!(
        prepared.outbound_request().nudge_mode,
        NudgeMode::Deferred,
        "graft task writes are deferred before queue handoff"
    );
    let dispatches = prepared
        .build_received_hook_dispatches(&runtime)
        .expect("build graft task dispatch");
    assert_eq!(dispatches.len(), 1);
    assert!(matches!(
        dispatches[0].target,
        PostSendBuiltInTarget::Graft(_)
    ));
    assert_eq!(dispatches[0].kind, NudgeKind::Queue);
    assert_eq!(
        built_in_nudge_template_kind_from_post_send_event(&dispatches[0].event, dispatches[0].kind,),
        atm_core::boundary::BuiltInNudgeTemplateKind::Task,
    );
    prepared
        .finish(&runtime, &NullObservability)
        .expect("finish graft task write");
    prepared
        .mark_pending_if_deferred(&runtime)
        .expect("mark graft task write");
}

#[test]
fn sync_and_async_graft_task_writes_use_the_queue_dispatch_kind() {
    assert_graft_task_dispatch(false);
    assert_graft_task_dispatch(true);
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
