use std::sync::{Arc, RwLock};

use atm_core::boundary::{
    self, BuiltInPostSendDispatch, GraftNudgeTarget, MessageReceivedHookEmitter,
    PostSendBuiltInTarget, PostSendEmissionPath,
};
use atm_core::error::AtmError;

use crate::runtime::read_snapshot;
use crate::{GraftObservability, HostNudge, HostNudgeInjector, SessionSnapshot};

/// Receiver-owned Graft implementation of the post-persistence hook boundary.
pub(crate) struct GraftReceiveHook<'a> {
    pub(crate) injector: &'a dyn HostNudgeInjector,
    pub(crate) snapshot: &'a Arc<RwLock<SessionSnapshot>>,
    pub(crate) observability: &'a dyn GraftObservability,
}

impl GraftReceiveHook<'_> {
    pub(crate) fn deliver(&self, nudge: HostNudge) -> Result<(), AtmError> {
        match self.injector.inject_nudge(&nudge) {
            Ok(()) => {
                match read_snapshot(self.snapshot) {
                    Ok(snapshot) => self.observability.nudge_delivered(&snapshot, &nudge.event),
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

impl boundary::sealed::Sealed for GraftReceiveHook<'_> {}

impl MessageReceivedHookEmitter for GraftReceiveHook<'_> {
    fn emit_received_message(
        &self,
        dispatch: &BuiltInPostSendDispatch,
        _deadline: atm_core::RequestDeadline,
    ) -> Result<PostSendEmissionPath, AtmError> {
        let PostSendBuiltInTarget::Graft(GraftNudgeTarget {
            recipient,
            recipient_team,
            rendered_nudge,
            message_body,
        }) = &dispatch.target
        else {
            return Err(AtmError::validation(
                "graft receive hook received a non-graft target",
            ));
        };
        let snapshot = read_snapshot(self.snapshot)?;
        if recipient != &snapshot.agent || recipient_team != &snapshot.team {
            return Err(AtmError::validation(
                "graft receive hook received an event for a different recipient",
            ));
        }
        // The agent loop receives the canonical `<atm …>` payload followed by
        // the exact immutable message body admitted with that event. Telegram's
        // visible notice remains a separate plain-text rendering so it shows
        // sender and subject without masquerading as a dispatch.
        let notice_text = format!(
            "📬 from {}\n{}",
            dispatch.event.source_address(),
            dispatch.event.description
        );
        self.deliver(HostNudge {
            event: dispatch.event.clone(),
            kind: dispatch.kind,
            body: format!("{rendered_nudge}\n\n{message_body}"),
            notice_text,
        })?;
        Ok(PostSendEmissionPath::GraftPort)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, RwLock};

    use atm_core::boundary::{
        BuiltInPostSendDispatch, GraftNudgeTarget, MessageReceivedHookEmitter, NudgeKind,
        PostSendBuiltInTarget, PostSendHookEvent,
    };
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::test_support::{TEST_ARCH_CTM, TEST_LEAD, TEST_TEAM};

    use super::GraftReceiveHook;
    use crate::{
        GraftObservability, GraftSessionState, HostNudge, HostNudgeInjector, SessionSnapshot,
    };

    #[derive(Default)]
    struct RecordingInjector {
        nudges: Mutex<Vec<HostNudge>>,
    }

    impl HostNudgeInjector for RecordingInjector {
        fn inject_nudge(&self, nudge: &HostNudge) -> Result<(), AtmError> {
            self.nudges.lock().expect("nudges").push(nudge.clone());
            Ok(())
        }
    }

    struct FailingInjector;

    impl HostNudgeInjector for FailingInjector {
        fn inject_nudge(&self, _nudge: &HostNudge) -> Result<(), AtmError> {
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
            sender_host: None,
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
        let sink = GraftReceiveHook {
            injector: &injector,
            snapshot: &snapshot(),
            observability: &NoopObservability,
        };

        let event = request_event();
        sink.deliver(HostNudge {
            kind: atm_core::boundary::NudgeKind::Steer,
            notice_text: format!("📬 from {}\n{}", event.source_address(), event.description),
            body: event.description.clone(),
            event,
        })
        .expect("delivery");

        assert_eq!(injector.nudges.lock().expect("nudges").len(), 1);
    }

    #[test]
    fn graft_nudge_sink_injects_rendered_xml_and_full_message_body() {
        let injector = RecordingInjector::default();
        let sink = GraftReceiveHook {
            injector: &injector,
            snapshot: &snapshot(),
            observability: &NoopObservability,
        };
        let event = request_event();
        let dispatch = BuiltInPostSendDispatch {
            event,
            target: PostSendBuiltInTarget::Graft(GraftNudgeTarget {
                recipient: TEST_ARCH_CTM.parse().expect("recipient"),
                recipient_team: TEST_TEAM.parse().expect("team"),
                rendered_nudge: "<atm><action>read atm</action></atm>".to_string(),
                message_body: "full immutable body".to_string(),
            }),
            kind: NudgeKind::Steer,
        };

        sink.emit_received_message(
            &dispatch,
            atm_core::RequestDeadline::after(std::time::Duration::from_secs(1)),
        )
        .expect("delivery");

        let nudges = injector.nudges.lock().expect("nudges");
        assert_eq!(nudges.len(), 1);
        assert_eq!(
            nudges[0].body,
            "<atm><action>read atm</action></atm>\n\nfull immutable body"
        );
        assert_eq!(
            nudges[0].notice_text,
            "📬 from test-lead@test-team\nreview failing smoke lane"
        );
    }

    #[test]
    fn graft_nudge_sink_returns_typed_error_envelope() {
        let sink = GraftReceiveHook {
            injector: &FailingInjector,
            snapshot: &snapshot(),
            observability: &NoopObservability,
        };

        let event = request_event();
        let error = sink
            .deliver(HostNudge {
                kind: atm_core::boundary::NudgeKind::Steer,
                notice_text: format!("📬 from {}\n{}", event.source_address(), event.description),
                body: event.description.clone(),
                event,
            })
            .expect_err("typed error");
        assert!(
            error
                .message()
                .contains("synthetic graft receiver unavailable")
        );
    }
}
