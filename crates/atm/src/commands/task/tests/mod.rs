#![cfg(test)]

mod task_assign;
mod task_close;
mod task_list_events;
mod task_move;
mod task_start;

use atm_core::protocol::{CompatibilityVerdict, HttpApiVersion, ReleaseVersion};
use atm_core::test_support::{EnvGuard, TEST_TEAM};
use clap::{CommandFactory, Parser};
use serial_test::serial;
use tempfile::TempDir;

use super::*;
use crate::commands::send::require_daemon_api;
use crate::commands::{Cli, Command};

#[test]
fn task_has_exactly_six_subcommands() {
    let command = Cli::command();
    let task = command.find_subcommand("task").expect("task command");
    let names: std::collections::BTreeSet<_> = task
        .get_subcommands()
        .map(clap::Command::get_name)
        .collect();
    assert_eq!(
        names,
        ["assign", "close", "events", "list", "move", "start"].into()
    );
}

#[test]
fn close_parses_positional_outcome_and_reason() {
    let cli = Cli::try_parse_from(["atm", "task", "close", "T1", "refused", "no capacity"])
        .expect("valid close");
    let Command::Task(TaskCommand {
        command: TaskSubcommand::Close(close),
    }) = cli.command
    else {
        panic!("expected task close");
    };
    assert!(matches!(close.outcome, OutcomeArg::Refused));
    assert_eq!(close.reason.as_deref(), Some("no capacity"));
    let error =
        Cli::try_parse_from(["atm", "task", "close", "T1", "bogus"]).expect_err("invalid outcome");
    let rendered = error.to_string();
    for expected in ["completed", "refused", "cancelled"] {
        assert!(rendered.contains(expected));
    }
}

#[test]
fn close_without_reason_or_report_source_is_rejected() {
    let cli = Cli::try_parse_from(["atm", "task", "close", "T1", "refused"]).expect("clap shape");
    let Command::Task(TaskCommand {
        command: TaskSubcommand::Close(close),
    }) = cli.command
    else {
        panic!("expected task close");
    };
    assert!(close.validate().is_err());
}

#[test]
fn close_with_source_and_no_reason_is_accepted() {
    for source in [
        vec!["--stdin"],
        vec!["--template", "report.j2", "--vars", "report.json"],
    ] {
        let mut args = vec!["atm", "task", "close", "T1", "completed"];
        args.extend(source);
        let cli = Cli::try_parse_from(args).expect("close report source");
        let Command::Task(TaskCommand {
            command: TaskSubcommand::Close(close),
        }) = cli.command
        else {
            panic!("expected task close");
        };
        assert!(close.reason.is_none());
        close.validate().expect("source makes close valid");
    }
}

#[test]
fn move_requires_exactly_one_target() {
    assert!(Cli::try_parse_from(["atm", "task", "move", "T1"]).is_err());
    assert!(Cli::try_parse_from(["atm", "task", "move", "T1", "--head", "--end"]).is_err());
    for target in [vec!["--head"], vec!["--end"], vec!["--before", "T2"]] {
        let mut args = vec!["atm", "task", "move", "T1"];
        args.extend(target);
        Cli::try_parse_from(args).expect("one move target");
    }
}

#[test]
fn assign_without_task_id_mints_ulid() {
    let minted = resolve_task_id(None).expect("generated task id");
    assert_eq!(minted.as_str().len(), 26);
    assert!(minted.as_str().chars().all(|character| {
        matches!(character, '0'..='9' | 'A'..='H' | 'J'..='N' | 'P'..='T' | 'V'..='Z')
    }));
    let supplied: TaskId = "T1".parse().expect("task id");
    assert_eq!(
        resolve_task_id(Some(supplied.clone())).expect("supplied task id"),
        supplied
    );
}

#[test]
#[serial(env)]
fn task_assign_template_load_failure_exits_two() {
    let _env = EnvGuard::set_many([("ATM_IDENTITY", Some("sender"))]);
    let home = TempDir::new().expect("home directory");
    let current = TempDir::new().expect("current directory");
    let command = SendCommand::for_task(TaskSendOptions {
        to: "recipient@test-team".to_string(),
        message: None,
        team: Some(TEST_TEAM.to_string()),
        actor: None,
        file: None,
        stdin: false,
        template: Some(current.path().join("missing.j2")),
        vars: None,
        task_id: Some("T1".parse().expect("task id")),
        json: false,
    });
    let error = command
        .build_request_with_mode(
            home.path().to_path_buf(),
            current.path().to_path_buf(),
            NudgeMode::Deferred,
            None,
        )
        .expect_err("missing task template");

    assert_eq!(crate::exit_code_for_error(&error), 2);
}

#[test]
fn list_and_events_accept_bounded_or_all_paging() {
    Cli::try_parse_from(["atm", "task", "list", "--all", "--json"]).expect("documented list flags");
    Cli::try_parse_from(["atm", "task", "list", "--limit", "10"]).expect("bounded list");
    assert!(Cli::try_parse_from(["atm", "task", "list", "--all", "--limit", "10"]).is_err());
    Cli::try_parse_from(["atm", "task", "events", "T1", "--all"]).expect("all events");
    Cli::try_parse_from(["atm", "task", "events", "T1", "--limit", "10"]).expect("bounded events");
    assert!(
        Cli::try_parse_from(["atm", "task", "events", "T1", "--all", "--limit", "10"]).is_err()
    );
    assert!(Cli::try_parse_from(["atm", "task", "list", "--member", "fenix"]).is_err());
}

#[test]
fn close_preflight_query_pushes_down_the_task_id() {
    let task_id: TaskId = "T1".parse().expect("task id");
    let home = TempDir::new().expect("home");
    let current = TempDir::new().expect("current");
    let query = task_list_request(
        home.path().to_path_buf(),
        current.path().to_path_buf(),
        "alice".parse().expect("agent"),
        TEST_TEAM.parse().expect("team"),
        None,
        Some(&task_id),
    )
    .expect("task query");
    assert_eq!(query.task_filter.as_ref(), Some(&task_id));
}

#[test]
fn renderer_groups_every_member_with_state_header() {
    let row = |task_id: &str, assignee: &str, position: u32| -> TaskRow {
        serde_json::from_value(serde_json::json!({
            "team": "test-team",
            "task_id": task_id,
            "assignee": assignee,
            "assigner": "lead",
            "state": "assigned",
            "position": position,
            "assignment_message_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "description": task_id,
            "assigned_at": "2026-01-02T00:00:00Z",
            "updated_at": "2026-01-02T00:00:00Z",
            "reminder_count": 0,
            "lead_notified_count": 0
        }))
        .expect("task row")
    };
    let runtime: RuntimeStatusSnapshot = serde_json::from_value(serde_json::json!({
        "liveness": "running",
        "readiness": "ready",
        "members": [
            {"team": "test-team", "member": "alice", "state": "active"},
            {"team": "test-team", "member": "bob", "state": "idle"}
        ]
    }))
    .expect("runtime snapshot");
    let output = render_task_rows(
        &[
            row("A1", "alice", 1),
            row("A2", "alice", 2),
            row("B1", "bob", 1),
        ],
        true,
        Some(&runtime),
    );
    assert_eq!(
        output,
        concat!(
            "alice (state: active)\n",
            "pos  state     task_id     assigned_at               reminders assigner\n",
            "1    assigned  A1          2026-01-02T00:00:00Z      0         lead\n",
            "2    assigned  A2          2026-01-02T00:00:00Z      0         lead\n",
            "\n",
            "bob (state: idle)\n",
            "pos  state     task_id     assigned_at               reminders assigner\n",
            "1    assigned  B1          2026-01-02T00:00:00Z      0         lead\n",
        )
    );
}

#[test]
fn require_daemon_api_refuses_older_daemon() {
    let verdict = |version: &str| CompatibilityVerdict::Compatible {
        daemon_release: ReleaseVersion::parse("1.5.14").expect("release"),
        daemon_schema_version: 1,
        daemon_http_api_version: HttpApiVersion::parse(version).expect("HTTP API"),
    };
    let min_15 = HttpApiVersion::parse("1.5.0").expect("minimum");
    let min_16 = HttpApiVersion::parse("1.6.0").expect("minimum");
    assert!(require_daemon_api(&verdict("1.4.0"), min_15.clone(), "task list").is_err());
    assert!(require_daemon_api(&verdict("1.5.0"), min_15, "task list").is_ok());
    assert!(require_daemon_api(&verdict("1.5.0"), min_16.clone(), "task move").is_err());
    assert!(require_daemon_api(&verdict("1.6.0"), min_16, "task move").is_ok());
}

#[test]
fn compatibility_helper_refuses_task_move_below_1_6_0() {
    let verdict = CompatibilityVerdict::Compatible {
        daemon_release: ReleaseVersion::parse("1.5.14").expect("release"),
        daemon_schema_version: 1,
        daemon_http_api_version: HttpApiVersion::parse("1.5.0").expect("HTTP API"),
    };
    let error = require_daemon_api(
        &verdict,
        HttpApiVersion::parse("1.6.0").expect("minimum"),
        "task move",
    )
    .expect_err("preflight must reject before request execution");
    assert!(error.message().contains("1.5.0"));
    assert!(error.message().contains("1.6.0"));
}

#[test]
fn parser_accepts_move_head_end_and_before() {
    let cases = [
        (vec!["--head"], MoveTarget::Head),
        (vec!["--end"], MoveTarget::End),
        (
            vec!["--before", "T2"],
            MoveTarget::Before {
                task_id: "T2".parse().expect("task id"),
            },
        ),
    ];
    for (flags, expected) in cases {
        let mut args = vec!["atm", "task", "move", "T1"];
        args.extend(flags);
        let cli = Cli::try_parse_from(args).expect("valid task move");
        let Command::Task(TaskCommand {
            command: TaskSubcommand::Move(command),
        }) = cli.command
        else {
            panic!("expected task move");
        };
        assert_eq!(command.target(), expected);
    }
}
