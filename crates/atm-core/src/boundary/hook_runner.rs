use async_trait::async_trait;

use super::sealed;
use crate::api::RequestDeadline;
use crate::doctor::hooks::HookDoctorReply;
use crate::error::AtmError;
use crate::hook::{HookReply, HookRequest, HookTarget, StopReply};

#[derive(Debug)]
pub struct HookOutcome {
    pub reply: HookReply,
    pub elapsed_ms: u64,
}

#[async_trait]
pub trait HookRunner: sealed::Sealed + Send + Sync {
    async fn run(
        &self,
        target: &HookTarget,
        request: &HookRequest,
        deadline: RequestDeadline,
    ) -> Result<HookOutcome, AtmError>;
    async fn doctor(
        &self,
        target: &HookTarget,
        deadline: RequestDeadline,
    ) -> Result<HookDoctorReply, AtmError>;
    async fn stop(
        &self,
        target: &HookTarget,
        request: &HookRequest,
        deadline: RequestDeadline,
    ) -> Result<StopReply, AtmError>;
}

#[cfg(any(test, feature = "test-utils"))]
#[derive(Clone, Debug)]
pub enum HookCall {
    Run(HookRequest),
    Stop(HookRequest),
    Doctor,
}

#[cfg(any(test, feature = "test-utils"))]
pub type BeforeReply = std::sync::Arc<
    dyn Fn(usize) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync,
>;

#[cfg(any(test, feature = "test-utils"))]
#[derive(Default)]
pub struct DummyHookRunner {
    pub replies: std::sync::Mutex<std::collections::VecDeque<Result<HookOutcome, AtmError>>>,
    pub stop_replies: std::sync::Mutex<std::collections::VecDeque<Result<StopReply, AtmError>>>,
    pub doctor_replies:
        std::sync::Mutex<std::collections::VecDeque<Result<HookDoctorReply, AtmError>>>,
    pub calls: std::sync::Mutex<Vec<(HookTarget, HookCall, RequestDeadline)>>,
    pub before_reply: std::sync::Mutex<Option<BeforeReply>>,
    pub panic_on_run: std::sync::atomic::AtomicBool,
}

#[cfg(any(test, feature = "test-utils"))]
impl sealed::Sealed for DummyHookRunner {}

#[cfg(any(test, feature = "test-utils"))]
impl DummyHookRunner {
    async fn record(&self, target: &HookTarget, call: HookCall, deadline: RequestDeadline) {
        let index = {
            let mut calls = self.calls.lock().expect("hook calls mutex poisoned");
            let index = calls.len();
            calls.push((target.clone(), call, deadline));
            index
        };
        let before_reply = self
            .before_reply
            .lock()
            .expect("before reply mutex poisoned")
            .clone();
        if let Some(before_reply) = before_reply {
            before_reply(index).await;
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl HookRunner for DummyHookRunner {
    async fn run(
        &self,
        target: &HookTarget,
        request: &HookRequest,
        deadline: RequestDeadline,
    ) -> Result<HookOutcome, AtmError> {
        self.record(target, HookCall::Run(request.clone()), deadline)
            .await;
        assert!(
            !self.panic_on_run.load(std::sync::atomic::Ordering::SeqCst),
            "DummyHookRunner run panic"
        );
        self.replies
            .lock()
            .expect("hook replies mutex poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                Err(AtmError::hook_malformed(
                    "DummyHookRunner run reply queue is empty",
                ))
            })
    }

    async fn doctor(
        &self,
        target: &HookTarget,
        deadline: RequestDeadline,
    ) -> Result<HookDoctorReply, AtmError> {
        self.record(target, HookCall::Doctor, deadline).await;
        self.doctor_replies
            .lock()
            .expect("doctor replies mutex poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                Err(AtmError::hook_malformed(
                    "DummyHookRunner doctor reply queue is empty",
                ))
            })
    }

    async fn stop(
        &self,
        target: &HookTarget,
        request: &HookRequest,
        deadline: RequestDeadline,
    ) -> Result<StopReply, AtmError> {
        self.record(target, HookCall::Stop(request.clone()), deadline)
            .await;
        self.stop_replies
            .lock()
            .expect("stop replies mutex poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                Err(AtmError::hook_malformed(
                    "DummyHookRunner stop reply queue is empty",
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{Context, Poll, Waker};

    use super::*;
    use crate::boundary::{HookName, PhaseId};
    use crate::hook::HookCaller;

    fn inputs() -> (HookTarget, HookRequest, RequestDeadline) {
        let target = HookTarget {
            team: "test-team".parse().unwrap(),
            phase: PhaseId::parse("bk").unwrap(),
            module_path: "/module.py".into(),
            cwd: "/work".into(),
            timeout_ms: 100,
            custom: serde_json::Map::new(),
        };
        let caller = HookCaller {
            identity: "solar".parse().unwrap(),
            team: target.team.clone(),
            host: None,
        };
        let request = HookRequest::for_cron(
            &target,
            caller,
            HookName::parse("cron.tick").unwrap(),
            &["arg".into()],
        );
        (
            target,
            request,
            RequestDeadline::after(std::time::Duration::ZERO),
        )
    }

    fn ready<F: Future>(future: F) -> F::Output {
        let mut future = Box::pin(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("controlled future was not ready"),
        }
    }

    #[test]
    fn reply_wait_is_driven_by_controlled_input_and_records_first() {
        let runner = DummyHookRunner::default();
        runner.replies.lock().unwrap().push_back(Ok(HookOutcome {
            reply: HookReply::PassThrough,
            elapsed_ms: 17,
        }));
        let released = Arc::new(AtomicBool::new(false));
        let controlled = released.clone();
        *runner.before_reply.lock().unwrap() = Some(Arc::new(move |index| {
            assert_eq!(index, 0);
            let controlled = controlled.clone();
            Box::pin(std::future::poll_fn(move |_| {
                if controlled.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }))
        }));
        let (target, request, deadline) = inputs();
        let mut future = Box::pin(runner.run(&target, &request, deadline));
        let mut context = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut context).is_pending());
        {
            let calls = runner.calls.lock().unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, target);
            assert_eq!(calls[0].2, deadline);
            assert!(matches!(&calls[0].1, HookCall::Run(recorded) if recorded == &request));
        }
        assert_eq!(runner.replies.lock().unwrap().len(), 1);
        released.store(true, Ordering::SeqCst);
        let Poll::Ready(Ok(reply)) = future.as_mut().poll(&mut context) else {
            panic!("released future did not reply")
        };
        assert_eq!(reply.reply, HookReply::PassThrough);
        assert_eq!(reply.elapsed_ms, 17);
        assert!(runner.replies.lock().unwrap().is_empty());
    }

    #[test]
    fn all_methods_record_and_consume_only_their_reply_queue() {
        let runner = DummyHookRunner::default();
        runner.stop_replies.lock().unwrap().push_back(Ok(StopReply {
            stop: true,
            reason: Some("closed".into()),
        }));
        runner
            .doctor_replies
            .lock()
            .unwrap()
            .push_back(HookDoctorReply::parse(
                br#"{"success":true,"message":"ok"}"#,
            ));
        let (target, request, deadline) = inputs();
        assert!(
            ready(runner.stop(&target, &request, deadline))
                .unwrap()
                .stop
        );
        assert!(ready(runner.doctor(&target, deadline)).unwrap().success);
        assert!(ready(runner.run(&target, &request, deadline)).is_err());
        assert!(ready(runner.stop(&target, &request, deadline)).is_err());
        assert!(ready(runner.doctor(&target, deadline)).is_err());
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 5);
        assert!(matches!(calls[0].1, HookCall::Stop(_)));
        assert!(matches!(calls[1].1, HookCall::Doctor));
        assert!(
            calls.iter().all(
                |(recorded_target, _, recorded_deadline)| recorded_target == &target
                    && *recorded_deadline == deadline
            )
        );
    }

    #[test]
    fn configured_run_panic_is_recorded_without_poisoning_call_mutex() {
        let runner = DummyHookRunner::default();
        runner.panic_on_run.store(true, Ordering::SeqCst);
        let (target, request, deadline) = inputs();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ready(
                runner.run(&target, &request, deadline)
            )))
            .is_err()
        );
        assert_eq!(runner.calls.lock().unwrap().len(), 1);
    }
}
