//! Integration coverage for rebuilding a queued dispatch from durable state.

use std::fs;

use atm_core::boundary::{
    MemberKey, NudgeKind, PostSendBuiltInTarget, RosterEntry, RosterHarness, RosterMemberKind,
};
use atm_core::nudge_dispatch::rebuild_received_hook_dispatch;
use atm_core::observability::NullObservability;
use atm_core::schema::AgentType;
use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
use atm_core::types::{AgentName, ModelName, PaneId, TeamName};
use atm_runtime_test_support::open_isolated_sqlite_boundary;

fn roster_member(team: &TeamName, agent: &str, pane_id: Option<PaneId>) -> RosterEntry {
    RosterEntry {
        team_name: team.clone(),
        agent_name: agent.parse().expect("agent"),
        member_kind: RosterMemberKind::Permanent,
        harness: RosterHarness::PythonGraft,
        agent_type: AgentType::default(),
        model: ModelName::default(),
        recipient_pane_id: pane_id,
        metadata_json: serde_json::Map::new(),
    }
}

fn setup() -> (tempfile::TempDir, atm_core::LocalServiceRuntime, TeamName) {
    let root = tempfile::tempdir().expect("temp root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("sqlite runtime");
    let runtime = assembly.service_runtime;
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
    (root, runtime, team)
}

fn write_request(
    home_dir: &std::path::Path,
    team: &TeamName,
    nudge_mode: NudgeMode,
) -> WriteRequest {
    WriteRequest::new(
        home_dir.to_path_buf(),
        home_dir.to_path_buf(),
        "sender".parse::<AgentName>().expect("sender"),
        "recipient@test-team",
        team.clone(),
        SendMessageSource::Inline("rebuild me".to_owned()),
        None,
        false,
        None,
        false,
    )
    .expect("write request")
    .with_nudge_mode(nudge_mode)
}

#[test]
fn rebuild_matches_write_time_dispatch_for_a_pending_tmux_recipient() {
    let (root, runtime, team) = setup();
    let home_dir = root.path().join("home");
    fs::create_dir_all(&home_dir).expect("home dir");

    let request = write_request(&home_dir, &team, NudgeMode::Immediate);
    let outcome =
        write_mail_with_runtime(request, &NullObservability, &runtime).expect("write succeeds");
    let message_id = outcome.persisted_message_id();

    let member = MemberKey::new(team, "recipient".parse().expect("agent"));
    let rebuilt = rebuild_received_hook_dispatch(&runtime, &member, message_id, NudgeKind::Queue)
        .expect("rebuild succeeds")
        .expect("dispatch rebuilt");

    assert_eq!(rebuilt.kind, NudgeKind::Queue);
    assert!(matches!(
        rebuilt.target,
        PostSendBuiltInTarget::LocalSteer(_)
    ));
    assert_eq!(rebuilt.event.message_id, message_id);
}

#[test]
fn rebuild_returns_none_for_a_message_not_addressed_to_member() {
    let (root, runtime, team) = setup();
    let home_dir = root.path().join("home");
    fs::create_dir_all(&home_dir).expect("home dir");

    let request = write_request(&home_dir, &team, NudgeMode::Immediate);
    let outcome =
        write_mail_with_runtime(request, &NullObservability, &runtime).expect("write succeeds");
    let message_id = outcome.persisted_message_id();

    let wrong_member = MemberKey::new(team, "sender".parse().expect("agent"));
    let rebuilt =
        rebuild_received_hook_dispatch(&runtime, &wrong_member, message_id, NudgeKind::Queue)
            .expect("rebuild does not error");
    assert!(rebuilt.is_none());
}
