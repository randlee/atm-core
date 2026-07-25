use std::sync::Arc;

use crate::api::{ApiRequest, ApiResponse, DaemonApiClient};
use crate::boundary;
use crate::clear;
use crate::doctor;
use crate::error::AtmError;
use crate::list;
use crate::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState, CommandEvent,
    LogTailSession, ObservabilityPort,
};
use crate::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
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

impl DaemonApiClient for FakeClientTransport {
    fn execute(&self, request: ApiRequest) -> Result<ApiResponse, AtmError> {
        (self.handler)(request.into_inner()).map(ApiResponse::new)
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

impl DaemonApiClient for LoopbackClientTransport {
    fn execute(&self, request: ApiRequest) -> Result<ApiResponse, AtmError> {
        let response = match request.into_inner() {
            RequestEnvelope::Write(request) => {
                send::write_mail(*request, self.observability.as_ref()).map(|outcome| match outcome
                {
                    send::WriteOutcome::Sent(outcome) => {
                        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome))
                    }
                    send::WriteOutcome::Acknowledged(outcome) => {
                        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome))
                    }
                })
            }
            RequestEnvelope::CompatibilityPreflight(_) => Err(AtmError::daemon_unavailable(
                "loopback compatibility preflight is not wired outside the daemon runtime",
            )),
            RequestEnvelope::Heartbeat(_) => Err(AtmError::daemon_unavailable(
                "loopback heartbeat transport is not wired outside the daemon runtime",
            )),
            RequestEnvelope::List(query) => {
                list::list_mail(query, self.observability.as_ref()).map(ResponseEnvelope::List)
            }
            RequestEnvelope::Peek(query) => read::peek_mail(query, self.observability.as_ref())
                .map(|outcome| ResponseEnvelope::Peek(Box::new(outcome))),
            RequestEnvelope::Receive(query) => read::read_mail(query, self.observability.as_ref())
                .map(|outcome| ResponseEnvelope::Receive(Box::new(outcome))),
            RequestEnvelope::Clear(query) => {
                clear::clear_mail(query, self.observability.as_ref()).map(ResponseEnvelope::Clear)
            }
            RequestEnvelope::Doctor(query) => {
                doctor::run_doctor(query, self.observability.as_ref())
                    .map(|report| ResponseEnvelope::Doctor(Box::new(report)))
            }
            RequestEnvelope::PeerSync(_) => Err(AtmError::daemon_unavailable(
                "peer sync requires the daemon HTTPS transport",
            )),
            RequestEnvelope::ReloadRuntimeView => Err(AtmError::daemon_unavailable(
                "runtime reload requires the running daemon control plane",
            )),
        };
        response.map(ApiResponse::new)
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
            maintenance: None,
            diagnostic: None,
            detail: None,
        })
    }
}
