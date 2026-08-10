use tempfile::tempdir;

use super::tests::{RecordingPostSendEmitter, TestRuntime, install_home_env, send_request};
use crate::delivery_policy::DeliveryHarnessPath;
use crate::error_codes::AtmErrorCode;
use crate::observability::NullObservability;
use crate::protocol::NotificationKind;
use crate::send::SendCommandOutcome;
use crate::test_support::TEST_SENDER;
use crate::types::TeamName;

#[test]
#[serial_test::serial(env)]
fn send_non_claude_success_delivers_original_via_outbound_boundary() {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let tempdir = tempdir().expect("tempdir");
    let home_dir = tempdir.path().join("home");
    let _env = install_home_env(&home_dir);
    let post_send_emitter = RecordingPostSendEmitter::succeed();

    let outcome = super::send_mail_with_runtime_impl(
        send_request(tempdir.path()),
        &NullObservability,
        &runtime,
        Some(&post_send_emitter),
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
    let emitted = post_send_emitter.emitted();
    assert_eq!(emitted.len(), 1);
    let message_id = emitted[0].event.message_id.to_string();
    let events = super::tests::read_notification_events(&home_dir);
    let event = events
        .iter()
        .rev()
        .find(|event| {
            super::tests::notification_detail(event)
                .get("message_id")
                .and_then(serde_json::Value::as_str)
                == Some(message_id.as_str())
        })
        .expect("notification event for the sent message");
    assert_eq!(event.kind, NotificationKind::Delivery);
    assert_eq!(
        super::tests::notification_detail(event)
            .get("sender")
            .and_then(serde_json::Value::as_str),
        Some(TEST_SENDER)
    );
    assert_eq!(event.team.as_ref().map(TeamName::as_str), Some("test-team"));
    assert_eq!(emitted[0].event.sender.as_str(), TEST_SENDER);
    assert_eq!(emitted[0].event.description, "hello");
}

#[test]
#[serial_test::serial(env)]
fn send_non_claude_warns_when_graft_post_send_delivery_fails() {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let tempdir = tempdir().expect("tempdir");
    let home_dir = tempdir.path().join("home");
    let _env = install_home_env(&home_dir);
    let post_send_emitter = RecordingPostSendEmitter::fail(AtmErrorCode::PostSendGraftUnavailable);

    let outcome = super::send_mail_with_runtime_impl(
        send_request(tempdir.path()),
        &NullObservability,
        &runtime,
        Some(&post_send_emitter),
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
