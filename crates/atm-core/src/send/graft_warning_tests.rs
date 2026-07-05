use std::sync::Mutex;

use std::fs;

use tempfile::tempdir;

use super::tests::{TestRuntime, install_home_env, send_request};
use crate::boundary::GraftPostSendPort;
use crate::delivery_policy::DeliveryHarnessPath;
use crate::error::{AtmError, AtmErrorKind};
use crate::error_codes::AtmErrorCode;
use crate::observability::NullObservability;
use crate::protocol::NotificationKind;
use crate::send::SendCommandOutcome;
use crate::test_support::TEST_SENDER;
use crate::types::TeamName;

#[derive(Default)]
struct RecordingGraftPort {
    events: Mutex<Vec<crate::boundary::PostSendHookEvent>>,
}

impl crate::boundary::sealed::Sealed for RecordingGraftPort {}

impl GraftPostSendPort for RecordingGraftPort {
    fn deliver_post_send(
        &self,
        event: &crate::boundary::PostSendHookEvent,
    ) -> Result<(), AtmError> {
        self.events
            .lock()
            .expect("graft events lock")
            .push(event.clone());
        Ok(())
    }
}

struct FailingGraftPort {
    events: Mutex<Vec<crate::boundary::PostSendHookEvent>>,
}

impl crate::boundary::sealed::Sealed for FailingGraftPort {}

impl GraftPostSendPort for FailingGraftPort {
    fn deliver_post_send(
        &self,
        event: &crate::boundary::PostSendHookEvent,
    ) -> Result<(), AtmError> {
        self.events
            .lock()
            .expect("graft events lock")
            .push(event.clone());
        Err(AtmError::new_with_code(
            AtmErrorCode::PostSendGraftUnavailable,
            AtmErrorKind::DaemonUnavailable,
            "simulated graft delivery failure",
        ))
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
    let graft_port = RecordingGraftPort::default();
    let tempdir = tempdir().expect("tempdir");
    let home_dir = tempdir.path().join("home");
    let _env = install_home_env(&home_dir);

    let outcome = super::send_mail_with_runtime_impl(
        send_request(tempdir.path()),
        &NullObservability,
        &runtime,
        Some(&graft_port),
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
    let graft_events = graft_port.events.lock().expect("graft events lock");
    assert_eq!(graft_events.len(), 1);
    assert_eq!(graft_events[0].sender.as_str(), TEST_SENDER);
    assert_eq!(graft_events[0].recipient.as_str(), "recipient");
    assert_eq!(graft_events[0].recipient_team.as_str(), "test-team");
    assert_eq!(graft_events[0].message, "hello");
}

#[test]
#[serial_test::serial(env)]
fn send_non_claude_warns_when_graft_post_send_delivery_fails() {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let graft_port = FailingGraftPort {
        events: Mutex::new(Vec::new()),
    };
    let tempdir = tempdir().expect("tempdir");
    let home_dir = tempdir.path().join("home");
    let _env = install_home_env(&home_dir);

    let outcome = super::send_mail_with_runtime_impl(
        send_request(tempdir.path()),
        &NullObservability,
        &runtime,
        Some(&graft_port),
    )
    .expect("send outcome");

    assert_eq!(outcome.outcome, SendCommandOutcome::Sent);
    assert_eq!(outcome.warnings.len(), 1);
    assert!(
        outcome.warnings[0]
            .message
            .contains("warning: post-send emission failed")
    );
    assert!(
        outcome.warnings[0]
            .message
            .contains("ATM_POST_SEND_GRAFT_UNAVAILABLE")
    );
    assert_eq!(
        graft_port.events.lock().expect("graft events lock").len(),
        1
    );
}
