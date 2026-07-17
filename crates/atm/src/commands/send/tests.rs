use std::path::PathBuf;

use super::SendCommand;
use atm_core::roles::ROLE_TEAM_LEAD;
use atm_core::send::{RemoteTargetHost, SendMessageSource};
use atm_core::test_support::EnvGuard;
use serial_test::serial;
use tempfile::TempDir;

const TEST_TEAM: &str = "test-team";

#[test]
#[serial(env)]
fn build_request_rejects_invalid_target_before_core() {
    let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(ROLE_TEAM_LEAD))]);
    let command = SendCommand {
        to: "../evil".to_string(),
        message: Some("hello".to_string()),
        team: Some(TEST_TEAM.to_string()),
        host: None,
        file: None,
        stdin: false,
        summary: None,
        requires_ack: false,
        task_id: None,
        dry_run: false,
        json: false,
    };

    let error = command
        .build_request(".".into(), ".".into())
        .expect_err("invalid target");

    assert!(error.to_string().contains("agent name"));
}

#[test]
fn build_message_source_rejects_conflicting_input_flags() {
    let stdin_and_file = SendCommand {
        to: "recipient-a@test-team".to_string(),
        message: None,
        team: None,
        host: None,
        file: Some(PathBuf::from("message.md")),
        stdin: true,
        summary: None,
        requires_ack: false,
        task_id: None,
        dry_run: false,
        json: false,
    };
    let stdin_and_message = SendCommand {
        to: "recipient-a@test-team".to_string(),
        message: Some("hello".to_string()),
        team: None,
        host: None,
        file: None,
        stdin: true,
        summary: None,
        requires_ack: false,
        task_id: None,
        dry_run: false,
        json: false,
    };

    let file_error = stdin_and_file
        .build_message_source()
        .expect_err("stdin/file conflict");
    let message_error = stdin_and_message
        .build_message_source()
        .expect_err("stdin/message conflict");

    assert!(file_error.to_string().contains("mutually exclusive"));
    assert!(message_error.to_string().contains("mutually exclusive"));
}

#[test]
fn build_message_source_requires_one_input_channel() {
    let command = SendCommand {
        to: "recipient-a@test-team".to_string(),
        message: None,
        team: None,
        host: None,
        file: None,
        stdin: false,
        summary: None,
        requires_ack: false,
        task_id: None,
        dry_run: false,
        json: false,
    };

    let error = command.build_message_source().expect_err("missing message");

    assert!(error.to_string().contains("provide message text"));
}

#[test]
#[serial(env)]
fn build_request_preserves_cli_send_options() {
    let _env = EnvGuard::set_many([
        ("ATM_IDENTITY", Some(ROLE_TEAM_LEAD)),
        ("ATM_TEAM", Some(TEST_TEAM)),
    ]);
    let command = SendCommand {
        to: "recipient-a@test-team".to_string(),
        message: Some("hello from send".to_string()),
        team: Some(TEST_TEAM.to_string()),
        host: None,
        file: None,
        stdin: false,
        summary: Some("summary".to_string()),
        requires_ack: true,
        task_id: Some("TASK-42".parse().expect("task id")),
        dry_run: true,
        json: true,
    };
    let tempdir = TempDir::new().expect("tempdir");

    let request = command
        .build_request(tempdir.path().join("home"), tempdir.path().join("cwd"))
        .expect("request");

    assert_eq!(Some(request.caller_identity.as_str()), Some(ROLE_TEAM_LEAD));
    assert_eq!(Some(request.caller_team.as_str()), Some(TEST_TEAM));
    assert_eq!(request.summary_override.as_deref(), Some("summary"));
    assert!(request.requires_ack);
    assert_eq!(
        request.task_id.as_ref().map(|value| value.as_str()),
        Some("TASK-42")
    );
    assert!(request.dry_run);
    assert_eq!(request.to.to_string(), "recipient-a@test-team");
    match request.message_source {
        SendMessageSource::Inline(message) => assert_eq!(message, "hello from send"),
        other => panic!("expected inline message source, got {other:?}"),
    }
}

#[test]
#[serial(env)]
fn build_request_uses_environment_when_overrides_are_absent() {
    let _env = EnvGuard::set_many([
        ("ATM_IDENTITY", Some("sender-a")),
        ("ATM_TEAM", Some(TEST_TEAM)),
    ]);
    let command = SendCommand {
        to: "recipient-a@test-team".to_string(),
        message: Some("hello".to_string()),
        team: None,
        host: None,
        file: None,
        stdin: false,
        summary: None,
        requires_ack: false,
        task_id: None,
        dry_run: false,
        json: false,
    };

    let request = command
        .build_request(".".into(), ".".into())
        .expect("request");

    assert_eq!(request.caller_identity.as_str(), "sender-a");
    assert_eq!(request.caller_team.as_str(), TEST_TEAM);
}

#[test]
#[serial(env)]
fn build_request_uses_environment_identity_even_with_team_override() {
    let _env = EnvGuard::set_many([
        ("ATM_IDENTITY", Some("env-sender")),
        ("ATM_TEAM", Some("env-team")),
    ]);
    let command = SendCommand {
        to: "recipient-a@test-team".to_string(),
        message: Some("hello".to_string()),
        team: Some(TEST_TEAM.to_string()),
        host: None,
        file: None,
        stdin: false,
        summary: None,
        requires_ack: false,
        task_id: None,
        dry_run: false,
        json: false,
    };

    let request = command
        .build_request(".".into(), ".".into())
        .expect("request");

    assert_eq!(request.caller_identity.as_str(), "env-sender");
    assert_eq!(request.caller_team.as_str(), TEST_TEAM);
}

#[test]
#[serial(env)]
fn build_request_parses_inline_remote_target_and_records_host() {
    let _env = EnvGuard::set_many([
        ("ATM_IDENTITY", Some("env-sender")),
        ("ATM_TEAM", Some(TEST_TEAM)),
    ]);
    let command = SendCommand {
        to: "recipient-a@test-team.localhost".to_string(),
        message: Some("hello".to_string()),
        team: None,
        host: None,
        file: None,
        stdin: false,
        summary: None,
        requires_ack: false,
        task_id: None,
        dry_run: false,
        json: false,
    };

    let request = command
        .build_request(".".into(), ".".into())
        .expect("request");

    assert_eq!(request.to.to_string(), "recipient-a@test-team");
    assert_eq!(
        request.remote_host.as_ref().map(RemoteTargetHost::as_str),
        Some("localhost")
    );
}

#[test]
#[serial(env)]
fn build_request_rejects_mixed_inline_and_explicit_host() {
    let _env = EnvGuard::set_many([
        ("ATM_IDENTITY", Some("env-sender")),
        ("ATM_TEAM", Some(TEST_TEAM)),
    ]);
    let command = SendCommand {
        to: "recipient-a@test-team.localhost".to_string(),
        message: Some("hello".to_string()),
        team: None,
        host: Some("127.0.0.1".to_string()),
        file: None,
        stdin: false,
        summary: None,
        requires_ack: false,
        task_id: None,
        dry_run: false,
        json: false,
    };

    let error = command
        .build_request(".".into(), ".".into())
        .expect_err("mixed host forms must fail");

    assert!(
        error
            .to_string()
            .contains("cannot combine inline remote host syntax")
    );
}

#[test]
#[serial(env)]
fn build_request_supports_file_with_trailing_inline_note() {
    let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(ROLE_TEAM_LEAD))]);
    let command = SendCommand {
        to: "recipient-a@test-team".to_string(),
        message: Some("note".to_string()),
        team: Some(TEST_TEAM.to_string()),
        host: None,
        file: Some(PathBuf::from("incident.md")),
        stdin: false,
        summary: None,
        requires_ack: false,
        task_id: None,
        dry_run: false,
        json: false,
    };
    let tempdir = TempDir::new().expect("tempdir");

    let request = command
        .build_request(tempdir.path().join("home"), tempdir.path().join("cwd"))
        .expect("request");

    match request.message_source {
        SendMessageSource::File { path, message } => {
            assert_eq!(path, PathBuf::from("incident.md"));
            assert_eq!(message.as_deref(), Some("note"));
        }
        other => panic!("expected file message source, got {other:?}"),
    }
}
