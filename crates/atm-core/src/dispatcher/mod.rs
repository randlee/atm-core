use crate::store::StoreError;
use crate::types::{AgentName, TeamName};

/// Qualified daemon request kinds. Transport layers must decode and dispatch
/// immediately to an injected handler rather than accumulating business logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Send,
    Ack,
    Read,
    Clear,
    Heartbeat,
    Doctor,
}

/// Minimal shared request envelope for local and remote transport adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonRequest {
    pub kind: RequestKind,
    pub team_name: TeamName,
    pub agent_name: AgentName,
    pub payload_json: String,
}

/// Minimal shared daemon response envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonResponse {
    pub kind: RequestKind,
    pub payload_json: String,
}

/// Dispatcher-layer error. Transport code should surface this typed error
/// rather than embedding store/notifier/watcher logic inline.
#[derive(Debug)]
pub enum DispatchError {
    Store(StoreError),
    InvalidPayload(String),
}

/// Thin request-dispatch boundary shared by Unix-domain, TCP/TLS, and
/// test-socket transports.
pub trait RequestDispatcher: Send + Sync {
    fn dispatch(&self, request: DaemonRequest) -> Result<DaemonResponse, DispatchError>;
}
