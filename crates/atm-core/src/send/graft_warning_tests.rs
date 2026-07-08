use std::fs;

use tempfile::tempdir;

use super::tests::{TestRuntime, install_home_env_with_atm_bin, send_request};
use crate::delivery_policy::DeliveryHarnessPath;
use crate::observability::NullObservability;
use crate::protocol::NotificationKind;
use crate::send::SendCommandOutcome;
use crate::test_support::TEST_SENDER;
use crate::types::TeamName;

pub(super) fn write_atm_nudge_shim(
    path: &std::path::Path,
    capture_path: &std::path::Path,
    exit_code: i32,
) {
    #[cfg(windows)]
    fs::write(
        path,
        format!(
            "@echo off\r\n> \"{}\" echo %1^|%ATM_INTERNAL_NUDGE_SINK%^|%ATM_POST_SEND%\r\nexit /b {}\r\n",
            capture_path.display(),
            exit_code
        ),
    )
    .expect("write atm shim");
    #[cfg(not(windows))]
    fs::write(
        path,
        format!(
            "#!/bin/sh\nprintf '%s|%s|%s\\n' \"$1\" \"$ATM_INTERNAL_NUDGE_SINK\" \"$ATM_POST_SEND\" > \"{}\"\nexit {}\n",
            capture_path.display(),
            exit_code
        ),
    )
    .expect("write atm shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("chmod");
    }
}

#[test]
fn load_send_alert_state_parse_errors_are_config_errors() {
    let tempdir = tempdir().expect("tempdir");
    let path = super::alert_state::state_path(tempdir.path());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("state dir");
    }
    fs::write(&path, "{not-json").expect("state file");

    let error = super::alert_state::load(&path).expect_err("malformed state");
    assert!(error.is_config());
}

#[test]
#[serial_test::serial(env)]
fn send_non_claude_success_delivers_original_via_outbound_boundary() {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let tempdir = tempdir().expect("tempdir");
    let home_dir = tempdir.path().join("home");
    let capture_path = tempdir.path().join("graft-nudge.txt");
    #[cfg(windows)]
    let atm_path = tempdir.path().join("atm.cmd");
    #[cfg(not(windows))]
    let atm_path = tempdir.path().join("atm");
    write_atm_nudge_shim(&atm_path, &capture_path, 0);
    let atm_bin = atm_path.display().to_string();
    let _env = install_home_env_with_atm_bin(&home_dir, atm_bin.as_str());

    let outcome = super::send_mail_with_runtime_impl(
        send_request(tempdir.path()),
        &NullObservability,
        &runtime,
        None,
    )
    .expect("send outcome");

    assert_eq!(outcome.outcome, SendCommandOutcome::Sent);
    assert!(outcome.warnings.is_empty());
    assert!(
        runtime
            .appended_messages
            .lock()
            .expect("append lock")
            .is_empty()
    );
    let deliveries = runtime
        .non_claude_deliveries
        .lock()
        .expect("non-claude deliveries lock");
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].messages.len(), 1);
    assert_eq!(deliveries[0].messages[0].from.as_str(), TEST_SENDER);
    drop(deliveries);
    let events = super::tests::read_notification_events(&home_dir);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, NotificationKind::Delivery);
    assert_eq!(
        super::tests::notification_detail(&events[0])
            .get("sender")
            .and_then(serde_json::Value::as_str),
        Some(TEST_SENDER)
    );
    assert_eq!(
        events[0].team.as_ref().map(TeamName::as_str),
        Some("test-team")
    );
    let captured = fs::read_to_string(&capture_path).expect("capture");
    assert!(captured.contains("internal-nudge"));
    assert!(captured.contains("|graft|"));
    assert!(captured.contains(format!("\"sender\":\"{TEST_SENDER}\"").as_str()));
    assert!(captured.contains("\"description\":\"hello\""));
}

#[test]
#[serial_test::serial(env)]
fn send_non_claude_warns_when_graft_post_send_delivery_fails() {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let tempdir = tempdir().expect("tempdir");
    let home_dir = tempdir.path().join("home");
    let capture_path = tempdir.path().join("graft-nudge.txt");
    #[cfg(windows)]
    let atm_path = tempdir.path().join("atm.cmd");
    #[cfg(not(windows))]
    let atm_path = tempdir.path().join("atm");
    write_atm_nudge_shim(&atm_path, &capture_path, 7);
    let atm_bin = atm_path.display().to_string();
    let _env = install_home_env_with_atm_bin(&home_dir, atm_bin.as_str());

    let outcome = super::send_mail_with_runtime_impl(
        send_request(tempdir.path()),
        &NullObservability,
        &runtime,
        None,
    )
    .expect("send outcome");

    assert_eq!(outcome.outcome, SendCommandOutcome::Sent);
    assert_eq!(outcome.warnings.len(), 1);
    assert!(
        outcome.warnings[0]
            .message
            .contains("warning: post-send emission failed")
    );
    let notification_path =
        crate::home::host_runtime_dir_from_home(&home_dir).join("notifications.jsonl");
    assert!(
        !notification_path.exists(),
        "notification log should not append when graft post-send emission fails"
    );
}
