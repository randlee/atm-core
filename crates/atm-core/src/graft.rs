//! Thin graft-facing daemon client contracts shared by embedded host agents.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::ack::{AckOutcome, AckRequest};
use crate::boundary::PostSendHookEvent;
use crate::error::AtmError;
use crate::protocol::ProtocolErrorEnvelope;
use crate::read::{ReadOutcome, ReadQuery};
use crate::send::{SendOutcome, SendRequest};
use crate::types::{AgentName, TeamName};

pub const MAX_GRAFT_POST_SEND_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftPostSendRequest {
    pub event: PostSendHookEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GraftPostSendResponse {
    Delivered,
    Error(ProtocolErrorEnvelope),
}

pub fn graft_receiver_socket_path_from_home(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> PathBuf {
    home_dir
        .join(".atm")
        .join("graft")
        .join(team.as_str())
        .join(format!("{agent}.sock"))
}

pub fn write_graft_post_send_message<T: Serialize>(
    writer: &mut impl Write,
    value: &T,
    write_error: &'static str,
    oversize_error: &'static str,
) -> Result<(), AtmError> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_GRAFT_POST_SEND_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(oversize_error).with_recovery(
            "Reduce the graft post-send payload size before retrying same-host graft delivery.",
        ));
    }
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| writer.write_all(&bytes))
        .map_err(|source| AtmError::daemon_unavailable(write_error).with_source(source))
}

pub fn read_graft_post_send_message<T: DeserializeOwned>(
    reader: &mut impl Read,
    read_error: &'static str,
    oversize_error: &'static str,
) -> Result<T, AtmError> {
    let mut header = [0u8; 4];
    reader
        .read_exact(&mut header)
        .map_err(|source| AtmError::daemon_unavailable(read_error).with_source(source))?;
    let payload_len = u32::from_be_bytes(header) as usize;
    if payload_len > MAX_GRAFT_POST_SEND_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(oversize_error).with_recovery(
            "Reduce the graft post-send payload size before retrying same-host graft delivery.",
        ));
    }
    let mut bytes = vec![0u8; payload_len];
    reader
        .read_exact(&mut bytes)
        .map_err(|source| AtmError::daemon_unavailable(read_error).with_source(source))?;
    serde_json::from_slice(&bytes).map_err(|source| {
        AtmError::validation("failed to decode graft post-send message")
            .with_recovery(
                "Align the daemon and graft builds so both sides use the same graft post-send wire contract before retrying delivery.",
            )
            .with_source(source)
    })
}

/// Open unary client surface for embedded ATM consumers.
///
/// This trait is intentionally not sealed. `atm-graft` must be able to
/// implement the concrete same-host client in a separate crate without taking
/// a Rust dependency on `atm-daemon`.
pub trait AtmGraftClient: Send + Sync {
    /// Execute one send-shaped ATM compose request.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the underlying daemon-backed send path cannot
    /// complete successfully.
    fn send_message(&self, request: SendRequest) -> Result<SendOutcome, AtmError>;

    /// Execute one ATM read request through the same daemon-backed semantic
    /// path used by the retained CLI.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the read request cannot be delivered or the
    /// daemon returns a typed failure.
    fn read_message(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError>;

    /// Execute one send-shaped ATM acknowledgement request.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the acknowledgement request cannot be
    /// completed successfully.
    fn acknowledge_message(&self, request: AckRequest) -> Result<AckOutcome, AtmError>;
}

#[cfg(test)]
mod tests {
    use super::AtmGraftClient;
    use crate::ack::{AckOutcome, AckRequest};
    use crate::error::AtmError;
    use crate::read::{ReadOutcome, ReadQuery};
    use crate::send::{SendOutcome, SendRequest};

    #[derive(Debug)]
    struct MockGraftClient;

    impl AtmGraftClient for MockGraftClient {
        fn send_message(&self, _request: SendRequest) -> Result<SendOutcome, AtmError> {
            panic!("send_message should not be called in trait object test")
        }

        fn read_message(&self, _query: ReadQuery) -> Result<ReadOutcome, AtmError> {
            panic!("read_message should not be called in trait object test")
        }

        fn acknowledge_message(&self, _request: AckRequest) -> Result<AckOutcome, AtmError> {
            panic!("acknowledge_message should not be called in trait object test")
        }
    }

    #[test]
    fn atm_graft_client_trait_is_object_safe() {
        let client: &dyn AtmGraftClient = &MockGraftClient;
        let _ = client;
    }
}
