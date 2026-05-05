//! Shared protocol DTOs for the core transport boundary family.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ack::{AckOutcome, AckRequest};
use crate::clear::{ClearOutcome, ClearQuery};
use crate::doctor::{DoctorQuery, DoctorReport};
use crate::read::{ReadOutcome, ReadQuery};
use crate::send::{SendOutcome, SendRequest};
use crate::types::{AgentName, TeamName};

/// Shared protocol send-shaped request envelope.
#[derive(Debug, Clone)]
pub enum SendRequestEnvelope {
    Compose(SendRequest),
    Acknowledge(AckRequest),
}

/// Shared protocol send-shaped response envelope.
#[derive(Debug, Clone, Serialize)]
pub enum SendResponseEnvelope {
    Sent(SendOutcome),
    Acknowledged(AckOutcome),
}

/// Shared protocol request envelope.
#[derive(Debug, Clone)]
pub enum RequestEnvelope {
    Send(SendRequestEnvelope),
    Receive(ReadQuery),
    Clear(ClearQuery),
    Doctor(DoctorQuery),
}

/// Shared protocol response envelope.
#[derive(Debug, Clone, Serialize)]
pub enum ResponseEnvelope {
    Send(SendResponseEnvelope),
    Receive(ReadOutcome),
    Clear(ClearOutcome),
    Doctor(DoctorReport),
}

/// Raw protocol frame payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FramePayload;

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
