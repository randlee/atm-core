use std::sync::Arc;

use crate::ack;
use crate::boundary::{self, ClientTransport};
use crate::clear;
use crate::doctor;
use crate::error::AtmError;
use crate::list;
use crate::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState, CommandEvent,
    LogTailSession, ObservabilityPort,
};
use crate::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};
use crate::read;
use crate::send;

#[derive(Clone)]
pub struct FakeClientTransport {
    handler: Arc<dyn Fn(RequestEnvelope) -> Result<ResponseEnvelope, AtmError> + Send + Sync>,
}

impl std::fmt::Debug for FakeClientTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeClientTransport")
            .finish_non_exhaustive()
    }
}

impl FakeClientTransport {
    pub fn new<F>(handler: F) -> Self
    where
        F: Fn(RequestEnvelope) -> Result<ResponseEnvelope, AtmError> + Send + Sync + 'static,
    {
        Self {
            handler: Arc::new(handler),
        }
    }
}

impl boundary::sealed::Sealed for FakeClientTransport {}

impl ClientTransport for FakeClientTransport {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        (self.handler)(request)
    }
}

pub struct LoopbackClientTransport {
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
}

impl std::fmt::Debug for LoopbackClientTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopbackClientTransport")
            .finish_non_exhaustive()
    }
}

impl LoopbackClientTransport {
    pub fn new(observability: Arc<dyn ObservabilityPort + Send + Sync>) -> Self {
        Self { observability }
    }
}

impl boundary::sealed::Sealed for LoopbackClientTransport {}

impl ClientTransport for LoopbackClientTransport {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => {
                send::send_mail(request, self.observability.as_ref())
                    .map(|outcome| ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)))
            }
            RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(request)) => {
                ack::ack_mail(request, self.observability.as_ref()).map(|outcome| {
                    ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome))
                })
            }
            RequestEnvelope::Heartbeat(_) => Err(AtmError::daemon_unavailable(
                "loopback heartbeat transport is not wired outside the daemon runtime",
            )),
            RequestEnvelope::List(query) => {
                list::list_mail(query, self.observability.as_ref()).map(ResponseEnvelope::List)
            }
            RequestEnvelope::Receive(query) => {
                read::read_mail(query, self.observability.as_ref()).map(ResponseEnvelope::Receive)
            }
            RequestEnvelope::Clear(query) => {
                clear::clear_mail(query, self.observability.as_ref()).map(ResponseEnvelope::Clear)
            }
            RequestEnvelope::Doctor(query) => {
                doctor::run_doctor(query, self.observability.as_ref()).map(ResponseEnvelope::Doctor)
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct HealthyObservability;

impl boundary::sealed::Sealed for HealthyObservability {}

impl ObservabilityPort for HealthyObservability {
    fn emit(&self, _event: CommandEvent) -> Result<(), AtmError> {
        Ok(())
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Ok(AtmLogSnapshot::default())
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Ok(LogTailSession::empty())
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        Ok(AtmObservabilityHealth {
            active_log_path: None,
            logging_state: AtmObservabilityHealthState::Healthy,
            query_state: Some(AtmObservabilityHealthState::Healthy),
            detail: None,
        })
    }
}
