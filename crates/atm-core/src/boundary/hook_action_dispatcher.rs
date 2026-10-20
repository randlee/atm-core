use async_trait::async_trait;

use super::{HookName, PhaseId, sealed};
use crate::api::{ApiResponse, RequestDeadline};
use crate::error::AtmError;
use crate::hook::HookAction;
use crate::types::{AgentName, TeamName};

#[derive(Clone, Debug, PartialEq)]
pub struct DispatchSpec {
    pub hook: HookName,
    pub team: TeamName,
    pub phase: PhaseId,
    pub default_as: AgentName,
    pub deadline: Option<RequestDeadline>,
    pub dry_run: bool,
}

#[async_trait]
pub trait HookActionDispatcher: sealed::Sealed + Send + Sync {
    async fn dispatch_actions(
        &self,
        spec: DispatchSpec,
        actions: Vec<HookAction>,
    ) -> Result<Vec<(Option<HookName>, ApiResponse)>, AtmError>;
}

#[cfg(any(test, feature = "test-utils"))]
#[derive(Clone, Debug)]
pub struct DispatchCall {
    pub spec: DispatchSpec,
    pub actions: Vec<HookAction>,
}

#[cfg(any(test, feature = "test-utils"))]
#[derive(Default)]
pub struct RecordingHookActionDispatcher {
    pub calls: std::sync::Mutex<Vec<DispatchCall>>,
    pub fail_at: Option<usize>,
    pub panic_at: Option<usize>,
}

#[cfg(any(test, feature = "test-utils"))]
impl sealed::Sealed for RecordingHookActionDispatcher {}

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl HookActionDispatcher for RecordingHookActionDispatcher {
    async fn dispatch_actions(
        &self,
        spec: DispatchSpec,
        actions: Vec<HookAction>,
    ) -> Result<Vec<(Option<HookName>, ApiResponse)>, AtmError> {
        self.calls
            .lock()
            .expect("dispatch calls mutex poisoned")
            .push(DispatchCall {
                spec: spec.clone(),
                actions: actions.clone(),
            });
        for action in &actions {
            action.into_api_request(&spec.team, &spec.default_as)?;
        }
        if spec.dry_run {
            return Ok(Vec::new());
        }
        let mut responses = Vec::new();
        let total = actions.len();
        for (index, action) in actions.into_iter().enumerate() {
            assert_ne!(
                self.panic_at,
                Some(index),
                "RecordingHookActionDispatcher action panic"
            );
            if self.fail_at == Some(index) {
                return Err(AtmError::hook_malformed(&format!(
                    "hook action {}/{} ({}) failed after {} committed: injected dispatch failure",
                    index + 1,
                    total,
                    action.command,
                    index
                )));
            }
            responses.push((
                HookName::parse_task_hook(&action.command).ok(),
                ApiResponse::new(crate::protocol::ResponseEnvelope::RuntimeViewReloaded),
            ));
        }
        Ok(responses)
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    use super::*;

    fn ready<F: Future>(future: F) -> F::Output {
        let mut future = Box::pin(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("recording dispatcher unexpectedly blocked"),
        }
    }

    fn spec(dry_run: bool) -> DispatchSpec {
        DispatchSpec {
            hook: HookName::parse("task.close").unwrap(),
            team: "test-team".parse().unwrap(),
            phase: PhaseId::parse("bk").unwrap(),
            default_as: "solar".parse().unwrap(),
            deadline: Some(RequestDeadline::after(std::time::Duration::ZERO)),
            dry_run,
        }
    }

    fn action(task: &str) -> HookAction {
        HookAction {
            command: "task.move".into(),
            run_as: None,
            request: serde_json::json!({"caller_identity": "other", "caller_team": "test-team", "task_id": task, "target": {"to": "end"}}),
        }
    }

    #[test]
    fn dispatcher_validates_all_before_injected_failure_or_panic() {
        let dispatcher = RecordingHookActionDispatcher {
            fail_at: Some(0),
            panic_at: Some(0),
            ..Default::default()
        };
        let mut invalid = action("bk-2");
        invalid.request["caller_team"] = serde_json::json!("foreign");
        let error = ready(dispatcher.dispatch_actions(spec(false), vec![action("bk-1"), invalid]))
            .unwrap_err();
        assert!(!error.to_string().contains("injected dispatch failure"));
        assert!(error.to_string().contains("team"));
        assert_eq!(dispatcher.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn dispatcher_keeps_order_and_records_spec_and_actions() {
        let dispatcher = RecordingHookActionDispatcher::default();
        let expected_spec = spec(false);
        let actions = vec![action("bk-1"), action("bk-2")];
        let results =
            ready(dispatcher.dispatch_actions(expected_spec.clone(), actions.clone())).unwrap();
        assert_eq!(results.len(), 2);
        let task_move = Some(HookName::parse("task.move").unwrap());
        assert!(results.iter().all(|(name, response)| name == &task_move
            && matches!(
                response.clone().into_inner(),
                crate::protocol::ResponseEnvelope::RuntimeViewReloaded
            )));
        let calls = dispatcher.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].spec, expected_spec);
        assert_eq!(calls[0].actions, actions);
    }

    #[test]
    fn dry_run_validates_but_never_runs_injected_effects() {
        let dispatcher = RecordingHookActionDispatcher {
            fail_at: Some(0),
            panic_at: Some(0),
            ..Default::default()
        };
        assert!(
            ready(dispatcher.dispatch_actions(spec(true), vec![action("bk-1")]))
                .unwrap()
                .is_empty()
        );
        let mut invalid = action("bk-2");
        invalid.command = "hook.register".into();
        assert!(ready(dispatcher.dispatch_actions(spec(true), vec![invalid])).is_err());
    }

    #[test]
    fn injected_failure_preserves_action_position_and_committed_count() {
        let dispatcher = RecordingHookActionDispatcher {
            fail_at: Some(1),
            ..Default::default()
        };
        let error = ready(dispatcher.dispatch_actions(
            spec(false),
            vec![action("bk-1"), action("bk-2"), action("bk-3")],
        ))
        .unwrap_err();
        assert!(error.to_string().contains(
            "hook action 2/3 (task.move) failed after 1 committed: injected dispatch failure"
        ));
        assert_eq!(dispatcher.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn injected_panic_does_not_poison_call_recording() {
        let dispatcher = RecordingHookActionDispatcher {
            panic_at: Some(1),
            ..Default::default()
        };
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ready(
                dispatcher.dispatch_actions(spec(false), vec![action("bk-1"), action("bk-2")])
            )))
            .is_err()
        );
        assert_eq!(dispatcher.calls.lock().unwrap().len(), 1);
    }
}
