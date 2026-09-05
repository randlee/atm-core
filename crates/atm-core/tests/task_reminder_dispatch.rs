use atm_core::boundary::{MemberKey, RosterEntry, RosterHarness, RosterMemberKind};
use atm_core::nudge_dispatch::build_task_reminder_dispatch;
use atm_core::schema::AtmMessageId;
use atm_core::types::{AgentName, IsoTimestamp, ModelName, TaskId, TeamName};
use atm_storage::{RosterSnapshot, TaskRow, TaskState};

fn task_row(team: &TeamName) -> TaskRow {
    TaskRow {
        team: team.clone(),
        task_id: "AX5-DISPATCH".parse::<TaskId>().expect("task id"),
        assignee: "recipient".parse::<AgentName>().expect("assignee"),
        assigner: "sender".parse::<AgentName>().expect("assigner"),
        state: TaskState::Assigned,
        assignment_message_id: AtmMessageId::new(),
        description: "remind the recipient from the durable task row".to_owned(),
        assigned_at: IsoTimestamp::now(),
        updated_at: IsoTimestamp::now(),
        last_reminded_at: None,
        reminder_count: 0,
        lead_notified_count: 0,
    }
}

fn member(team: &TeamName, backend: &str) -> RosterEntry {
    let mut metadata_json = serde_json::Map::new();
    metadata_json.insert(["backend", "Type"].concat(), serde_json::json!(backend));
    if backend == "herdr" {
        metadata_json.insert("herdrSession".to_owned(), serde_json::json!("ax5-test"));
    }
    RosterEntry {
        team_name: team.clone(),
        agent_name: "recipient".parse().expect("agent"),
        member_kind: RosterMemberKind::Permanent,
        harness: RosterHarness::CodexCli,
        agent_type: atm_core::schema::AgentType::default(),
        model: ModelName::default(),
        recipient_pane_id: None,
        metadata_json,
    }
}

#[test]
fn reminder_dispatch_renders_from_a_task_row_without_assignment_mail() {
    let root = tempfile::tempdir().expect("temporary runtime root");
    let assembly =
        atm_runtime_test_support::open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "ax5-dispatch".parse().expect("team");
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: vec![member(&team, "herdr")],
            refreshed_at: None,
        })
        .expect("roster");
    let row = task_row(&team);
    let key = MemberKey::new(team, row.assignee.clone());

    let dispatch = build_task_reminder_dispatch(&assembly.service_runtime, &key, &row)
        .expect("missing assignment mail is tolerated")
        .expect("Herdr dispatch");
    assert!(dispatch.event.sender_host.is_none());
    assert_eq!(dispatch.event.task_id, Some(row.task_id));
    assert!(dispatch.event.requires_ack);
    assert!(!dispatch.event.is_ack);
}

#[test]
fn reminder_dispatch_skips_a_non_herdr_assignee() {
    let root = tempfile::tempdir().expect("temporary runtime root");
    let assembly =
        atm_runtime_test_support::open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "ax5-dispatch".parse().expect("team");
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: vec![member(&team, "tmux")],
            refreshed_at: None,
        })
        .expect("roster");
    let row = task_row(&team);
    let key = MemberKey::new(team, row.assignee.clone());

    assert!(
        build_task_reminder_dispatch(&assembly.service_runtime, &key, &row)
            .expect("recipient lookup")
            .is_none()
    );
}
