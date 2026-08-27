//! L2.2/L2.5 coverage: `NudgeMode::Deferred` suppresses the immediate
//! receiver dispatch and sets exactly one durable queue marker via
//! the async router's blocking marker seam; `NudgeMode::Immediate` is byte-identical to the
//! pre-AQ1 dispatch; a duplicate (idempotent) write never sets a second
//! marker.

use std::sync::{Arc, Mutex};

use atm_core::boundary::{
    MemberKey, NudgeClaim, PendingNudgeStore, PostSendBuiltInTarget, RosterHarness,
    RosterMemberKind,
};
use atm_core::error::AtmError;
use atm_core::observability::NullObservability;
use atm_core::schema::AtmMessageId;
use atm_core::send::{
    NudgeMode, SendMessageSource, WriteRequest, prepare_write_with_runtime, write_mail_with_runtime,
};
use atm_core::types::{AgentName, IsoTimestamp, ModelName, PaneId, TeamName};

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
    prepared.mark_pending_if_deferred(&runtime);
    assert_eq!(
        recording_store.mark_pending_call_count(),
        1,
        "finishing a newly persisted deferred write must set exactly one queue marker"
    );
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
    prepared.mark_pending_if_deferred(&runtime);
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
        prepared.mark_pending_if_deferred(&runtime);
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
    prepared.mark_pending_if_deferred(&runtime);

    assert_eq!(
        failing_store.mark_pending_call_count(),
        1,
        "the failing store double must exercise the marker error path"
    );
}
