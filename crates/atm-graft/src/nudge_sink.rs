use std::sync::{Arc, RwLock};

use atm_core::boundary::PostSendHookEvent;
use atm_core::protocol::ProtocolErrorEnvelope;

use crate::runtime::read_snapshot;
use crate::{GraftObservability, HostNudgeInjector, SessionSnapshot};

pub(crate) struct GraftNudgeSink<'a> {
    pub(crate) injector: &'a dyn HostNudgeInjector,
    pub(crate) snapshot: &'a Arc<RwLock<SessionSnapshot>>,
    pub(crate) observability: &'a dyn GraftObservability,
}

impl GraftNudgeSink<'_> {
    pub(crate) fn deliver(&self, event: PostSendHookEvent) -> Result<(), ProtocolErrorEnvelope> {
        match self.injector.inject_nudge(event.clone()) {
            Ok(()) => {
                if let Ok(snapshot) = read_snapshot(self.snapshot) {
                    self.observability.nudge_delivered(&snapshot, &event);
                }
                Ok(())
            }
            Err(error) => {
                if let Ok(snapshot) = read_snapshot(self.snapshot) {
                    self.observability
                        .session_error(&snapshot, "inject_nudge", &error);
                }
                Err(ProtocolErrorEnvelope::from_error(&error))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, RwLock};

    use atm_core::boundary::PostSendHookEvent;
    use atm_core::error::{AtmError, AtmErrorKind};

    use super::GraftNudgeSink;
    use crate::{GraftObservability, GraftSessionState, HostNudgeInjector, SessionSnapshot};

    #[derive(Default)]
    struct RecordingInjector {
        events: Mutex<Vec<PostSendHookEvent>>,
    }

    impl HostNudgeInjector for RecordingInjector {
        fn inject_nudge(&self, nudge: PostSendHookEvent) -> Result<(), AtmError> {
            self.events.lock().expect("events").push(nudge);
            Ok(())
        }
    }

    struct FailingInjector;

    impl HostNudgeInjector for FailingInjector {
        fn inject_nudge(&self, _nudge: PostSendHookEvent) -> Result<(), AtmError> {
            Err(AtmError::new(
                AtmErrorKind::DaemonUnavailable,
                "synthetic graft receiver unavailable",
            ))
        }
    }

    #[derive(Default)]
    struct NoopObservability;

    impl GraftObservability for NoopObservability {}

    fn request_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: "team-lead".parse().expect("sender"),
            sender_team: "atm-dev".parse().expect("team"),
            recipient: "arch-ctm".parse().expect("recipient"),
            recipient_team: "atm-dev".parse().expect("team"),
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
            team: "atm-dev".parse().expect("team"),
            agent: "arch-ctm".parse().expect("agent"),
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
                .message
                .contains("synthetic graft receiver unavailable")
        );
    }
}
