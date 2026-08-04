use std::sync::{Arc, RwLock};

use atm_core::boundary::PostSendHookEvent;
use atm_core::error::AtmError;

use crate::runtime::read_snapshot;
use crate::{GraftObservability, HostNudgeInjector, SessionSnapshot};

pub(crate) struct GraftNudgeSink<'a> {
    pub(crate) injector: &'a dyn HostNudgeInjector,
    pub(crate) snapshot: &'a Arc<RwLock<SessionSnapshot>>,
    pub(crate) observability: &'a dyn GraftObservability,
}

impl GraftNudgeSink<'_> {
    pub(crate) fn deliver(&self, event: PostSendHookEvent) -> Result<(), AtmError> {
        match self.injector.inject_nudge(&event) {
            Ok(()) => {
                match read_snapshot(self.snapshot) {
                    Ok(snapshot) => self.observability.nudge_delivered(&snapshot, &event),
                    Err(error) => tracing::warn!(
                        subsystem = "atm_graft.nudge_sink",
                        action = "deliver",
                        outcome = "snapshot_unavailable",
                        error_code = %error.code(),
                        error_message = %error.message(),
                        "graft nudge delivery succeeded but the session snapshot could not be read for observability"
                    ),
                }
                Ok(())
            }
            Err(error) => {
                match read_snapshot(self.snapshot) {
                    Ok(snapshot) => {
                        self.observability
                            .session_error(&snapshot, "inject_nudge", &error);
                    }
                    Err(snapshot_error) => tracing::warn!(
                        subsystem = "atm_graft.nudge_sink",
                        action = "deliver",
                        outcome = "snapshot_unavailable",
                        error_code = %snapshot_error.code(),
                        error_message = %snapshot_error.message(),
                        delivery_error_code = %error.code(),
                        delivery_error_message = %error.message(),
                        "graft nudge delivery failed and the session snapshot could not be read for observability"
                    ),
                }
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, RwLock};

    use atm_core::boundary::PostSendHookEvent;
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::test_support::{TEST_ARCH_CTM, TEST_LEAD, TEST_TEAM};

    use super::GraftNudgeSink;
    use crate::{GraftObservability, GraftSessionState, HostNudgeInjector, SessionSnapshot};

    #[derive(Default)]
    struct RecordingInjector {
        events: Mutex<Vec<PostSendHookEvent>>,
    }

    impl HostNudgeInjector for RecordingInjector {
        fn inject_nudge(&self, nudge: &PostSendHookEvent) -> Result<(), AtmError> {
            self.events.lock().expect("events").push(nudge.clone());
            Ok(())
        }
    }

    struct FailingInjector;

    impl HostNudgeInjector for FailingInjector {
        fn inject_nudge(&self, _nudge: &PostSendHookEvent) -> Result<(), AtmError> {
            Err(AtmError::new(
                AtmErrorCode::DaemonUnavailable,
                "synthetic graft receiver unavailable",
            ))
        }
    }

    #[derive(Default)]
    struct NoopObservability;

    impl GraftObservability for NoopObservability {}

    fn request_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: TEST_LEAD.parse().expect("sender"),
            sender_chat_id: None,
            sender_team: TEST_TEAM.parse().expect("team"),
            authenticated_source_host: None,
            recipient: TEST_ARCH_CTM.parse().expect("recipient"),
            recipient_team: TEST_TEAM.parse().expect("team"),
            message_id: "01KX1TEST00000000000000000".parse().expect("message id"),
            description: "review failing smoke lane".to_string(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            recipient_pane_id: None,
        }
    }

    fn snapshot() -> Arc<RwLock<SessionSnapshot>> {
        Arc::new(RwLock::new(SessionSnapshot {
            team: TEST_TEAM.parse().expect("team"),
            agent: TEST_ARCH_CTM.parse().expect("agent"),
            state: GraftSessionState::Listening,
        }))
    }

    #[test]
    fn graft_nudge_sink_delivers_to_host_injector() {
        let injector = RecordingInjector::default();
        let sink = GraftNudgeSink {
            injector: &injector,
            snapshot: &snapshot(),
            observability: &NoopObservability,
        };

        sink.deliver(request_event()).expect("delivery");

        assert_eq!(injector.events.lock().expect("events").len(), 1);
    }

    #[test]
    fn graft_nudge_sink_returns_typed_error_envelope() {
        let sink = GraftNudgeSink {
            injector: &FailingInjector,
            snapshot: &snapshot(),
            observability: &NoopObservability,
        };

        let error = sink.deliver(request_event()).expect_err("typed error");
        assert!(
            error
                .message()
                .contains("synthetic graft receiver unavailable")
        );
    }
}
