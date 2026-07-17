use std::sync::Arc;

use crate::boundary::{self, ClientTransport};
use crate::error::AtmError;
use crate::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState, CommandEvent,
    LogTailSession, ObservabilityPort,
};
use crate::protocol::{RequestEnvelope, ResponseEnvelope};

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
