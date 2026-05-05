//! Shared protocol DTOs for the core transport boundary family.

use std::env;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ack::{AckOutcome, AckRequest};
use crate::clear::{ClearOutcome, ClearQuery};
use crate::doctor::{DoctorQuery, DoctorReport};
use crate::error::AtmError;
use crate::home;
use crate::read::{ReadOutcome, ReadQuery};
use crate::send::{SendOutcome, SendRequest};
use crate::types::{AgentName, TeamName};

/// Shared protocol send-shaped request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SendRequestEnvelope {
    Compose(SendRequest),
    Acknowledge(AckRequest),
}

/// Shared protocol send-shaped response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SendResponseEnvelope {
    Sent(SendOutcome),
    Acknowledged(AckOutcome),
}

/// Shared protocol request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestEnvelope {
    Send(SendRequestEnvelope),
    Receive(ReadQuery),
    Clear(ClearQuery),
    Doctor(DoctorQuery),
}

/// Shared protocol response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseEnvelope {
    Send(SendResponseEnvelope),
    Receive(ReadOutcome),
    Clear(ClearOutcome),
    Doctor(DoctorReport),
}

/// Raw protocol frame payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FramePayload {
    pub bytes: Vec<u8>,
}

/// Resolve the active daemon socket path for the ATM request transport.
///
/// # Errors
///
/// Returns [`AtmError`] when `ATM_HOME` cannot be resolved.
pub fn daemon_socket_path() -> Result<PathBuf, AtmError> {
    if let Some(path) = env::var_os("ATM_DAEMON_SOCKET").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    Ok(home::atm_home()?.join("atm-daemon.sock"))
}

/// Shared notification event payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotificationEvent {
    pub kind: String,
    pub detail: String,
    pub team: Option<TeamName>,
    pub agent: Option<AgentName>,
}

/// Runtime status snapshot transport payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeStatusSnapshot {
    pub status: String,
    pub detail: Option<String>,
}

/// Watch subscription request payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchSubscriptionRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
    pub agent: AgentName,
}

/// Watch event batch transport payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchEventBatch {
    pub paths: Vec<PathBuf>,
}

/// Reconcile request transport payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReconcileRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
    pub agent: AgentName,
}

/// Reconcile outcome transport payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReconcileResult {
    pub observed_paths: usize,
    pub imported_sources: usize,
}
