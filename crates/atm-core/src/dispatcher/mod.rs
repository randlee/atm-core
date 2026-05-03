use crate::store::StoreError;
use crate::types::{AgentName, TeamName};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(test)]
use std::collections::VecDeque;
#[cfg(test)]
use std::sync::Mutex;

/// Qualified daemon request kinds. Transport layers must decode and dispatch
/// immediately to an injected handler rather than accumulating business logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestKind {
    Send,
    Ack,
    Read,
    Clear,
    Heartbeat,
    Doctor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestPayload {
    Send(Value),
    Ack(Value),
    Read(Value),
    Clear(Value),
    Heartbeat(Value),
    Doctor(Value),
}

impl RequestPayload {
    pub const fn kind(&self) -> RequestKind {
        match self {
            Self::Send(_) => RequestKind::Send,
            Self::Ack(_) => RequestKind::Ack,
            Self::Read(_) => RequestKind::Read,
            Self::Clear(_) => RequestKind::Clear,
            Self::Heartbeat(_) => RequestKind::Heartbeat,
            Self::Doctor(_) => RequestKind::Doctor,
        }
    }

    pub fn body(&self) -> &Value {
        match self {
            Self::Send(value)
            | Self::Ack(value)
            | Self::Read(value)
            | Self::Clear(value)
            | Self::Heartbeat(value)
            | Self::Doctor(value) => value,
        }
    }
}

/// Minimal shared request envelope for local and remote transport adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonRequest {
    pub team_name: TeamName,
    pub agent_name: AgentName,
    pub payload: RequestPayload,
}

impl DaemonRequest {
    pub const fn kind(&self) -> RequestKind {
        self.payload.kind()
    }
}

/// Minimal shared daemon response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonResponse {
    pub kind: RequestKind,
    pub payload_json: String,
}

/// Dispatcher-layer error. Transport code should surface this typed error
/// rather than embedding store/notifier/watcher logic inline.
#[derive(Debug)]
pub enum DispatchError {
    Store(StoreError),
    PayloadDecode(String),
    Handler(String),
    ResponseEncode(String),
    Unsupported(RequestKind),
}

/// Thin request-dispatch boundary shared by Unix-domain, TCP/TLS, and
/// test-socket transports.
pub trait RequestDispatcher: Send + Sync {
    fn dispatch(&self, request: DaemonRequest) -> Result<DaemonResponse, DispatchError>;
}

#[cfg(test)]
#[derive(Default)]
pub struct TestSocketDispatcher {
    requests: Mutex<Vec<DaemonRequest>>,
    responses: Mutex<VecDeque<Result<DaemonResponse, DispatchError>>>,
}

#[cfg(test)]
impl TestSocketDispatcher {
    pub fn queue_response(&self, response: Result<DaemonResponse, DispatchError>) {
        self.responses
            .lock()
            .expect("test dispatcher responses lock")
            .push_back(response);
    }

    pub fn requests(&self) -> Vec<DaemonRequest> {
        self.requests
            .lock()
            .expect("test dispatcher requests lock")
            .clone()
    }
}

#[cfg(test)]
impl RequestDispatcher for TestSocketDispatcher {
    fn dispatch(&self, request: DaemonRequest) -> Result<DaemonResponse, DispatchError> {
        let request_kind = request.kind();
        self.requests
            .lock()
            .expect("test dispatcher requests lock")
            .push(request);
        self.responses
            .lock()
            .expect("test dispatcher responses lock")
            .pop_front()
            .unwrap_or_else(|| Err(DispatchError::Unsupported(request_kind)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_dispatcher_records_requests_and_returns_queued_response() {
        let dispatcher = TestSocketDispatcher::default();
        let team_name: TeamName = "atm-dev".parse().expect("team");
        let agent_name: AgentName = "arch-ctm".parse().expect("agent");
        dispatcher.queue_response(Ok(DaemonResponse {
            kind: RequestKind::Heartbeat,
            payload_json: "{\"ok\":true}".to_string(),
        }));

        let response = dispatcher
            .dispatch(DaemonRequest {
                team_name: team_name.clone(),
                agent_name: agent_name.clone(),
                payload: RequestPayload::Heartbeat(serde_json::json!({"pid": 42})),
            })
            .expect("queued response");

        assert_eq!(response.kind, RequestKind::Heartbeat);
        assert_eq!(
            dispatcher.requests(),
            vec![DaemonRequest {
                team_name,
                agent_name,
                payload: RequestPayload::Heartbeat(serde_json::json!({"pid": 42})),
            }]
        );
    }
}
