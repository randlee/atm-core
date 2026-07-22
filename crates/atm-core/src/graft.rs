//! Thin graft-facing daemon client contracts shared by embedded host agents.
//!
//! The graft post-send notification channel is a same-host loopback control
//! plane: an embedded agent binds a loopback [`GraftReceiverListener`] and both
//! the CLI post-send hook and the daemon deliver nudges to it via
//! [`deliver_graft_post_send`]. The transport is loopback TCP plus a per-bind
//! [`LocalCapability`] token so it works identically on Unix and Windows
//! without any local-socket / named-pipe backend.

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::ack::{AckOutcome, AckRequest};
use crate::boundary::PostSendHookEvent;
use crate::error::{AtmError, AtmErrorCode};
use crate::local_http::LocalCapability;
use crate::read::{ReadOutcome, ReadQuery};
use crate::send::{SendOutcome, SendRequest};
use crate::types::{AgentName, TeamName};

pub const MAX_GRAFT_POST_SEND_FRAME_BYTES: usize = 1024 * 1024;

/// Schema version stamped into the graft receiver endpoint record.
pub const GRAFT_RECEIVER_RECORD_SCHEMA_VERSION: u8 = 1;

/// Interval between non-blocking accept polls in the receiver loop.
pub const GRAFT_RECEIVER_ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftPostSendRequest {
    pub event: PostSendHookEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GraftPostSendResponse {
    Delivered,
    Error(AtmError),
}

/// Length-prefixed wire request carrying the caller's loopback capability.
///
/// The capability authenticates the sender against the exact receiver bind so
/// an unrelated loopback process cannot inject nudges into another agent's
/// receiver even if it discovers the port.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftPostSendWireRequest {
    pub capability_base64url: String,
    pub request: GraftPostSendRequest,
}

/// Owner-readable publication describing where an embedded agent listens for
/// post-send nudges.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftReceiverEndpointRecord {
    pub schema_version: u8,
    pub loopback: SocketAddr,
    pub capability_base64url: String,
}

impl GraftReceiverEndpointRecord {
    fn endpoint(&self) -> Result<SocketAddr, AtmError> {
        if self.schema_version != GRAFT_RECEIVER_RECORD_SCHEMA_VERSION {
            return Err(AtmError::validation(format!(
                "unsupported graft receiver endpoint record schema version {}",
                self.schema_version
            )));
        }
        if !self.loopback.ip().is_loopback() {
            return Err(AtmError::validation(
                "graft receiver endpoint record contains a non-loopback address",
            ));
        }
        Ok(self.loopback)
    }
}

/// Absolute path of the loopback endpoint record for an embedded agent.
pub fn graft_receiver_record_path_from_home(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> PathBuf {
    home_dir
        .join(".atm")
        .join("graft")
        .join(team.as_str())
        .join(format!("{agent}.json"))
}

pub fn write_graft_post_send_message<T: Serialize>(
    writer: &mut impl Write,
    value: &T,
    write_error: &'static str,
    oversize_error: &'static str,
) -> Result<(), AtmError> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_GRAFT_POST_SEND_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(oversize_error));
    }
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| writer.write_all(&bytes))
        .map_err(|_source| AtmError::daemon_unavailable(write_error))
}

pub fn read_graft_post_send_message<T: DeserializeOwned>(
    reader: &mut impl Read,
    read_error: &'static str,
    oversize_error: &'static str,
) -> Result<T, AtmError> {
    let mut header = [0u8; 4];
    reader
        .read_exact(&mut header)
        .map_err(|_source| AtmError::daemon_unavailable(read_error))?;
    let payload_len = u32::from_be_bytes(header) as usize;
    if payload_len > MAX_GRAFT_POST_SEND_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(oversize_error));
    }
    let mut bytes = vec![0u8; payload_len];
    reader
        .read_exact(&mut bytes)
        .map_err(|_source| AtmError::daemon_unavailable(read_error))?;
    serde_json::from_slice(&bytes)
        .map_err(|_source| AtmError::validation("failed to decode graft post-send message"))
}

/// Loopback receiver for graft post-send nudges.
///
/// The listener binds `127.0.0.1:0`, publishes an owner-readable endpoint
/// record, and validates a per-bind [`LocalCapability`] on every accepted
/// connection. It is a same-host control plane only: connections whose peer is
/// not loopback are dropped without being served.
pub struct GraftReceiverListener {
    listener: TcpListener,
    record_path: PathBuf,
    capability: LocalCapability,
}

impl GraftReceiverListener {
    /// Bind the loopback receiver and publish its endpoint record.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the loopback socket cannot be bound or the
    /// endpoint record cannot be published for the owner.
    pub fn bind(record_path: &Path) -> Result<Self, AtmError> {
        prepare_receiver_record_parent(record_path)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to bind graft receiver endpoint for {}",
                record_path.display()
            ))
        })?;
        listener.set_nonblocking(true).map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to configure non-blocking graft receiver endpoint for {}",
                record_path.display()
            ))
        })?;
        let loopback = listener.local_addr().map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to resolve graft receiver endpoint address for {}",
                record_path.display()
            ))
        })?;
        let capability = LocalCapability::generate()?;
        let record = GraftReceiverEndpointRecord {
            schema_version: GRAFT_RECEIVER_RECORD_SCHEMA_VERSION,
            loopback,
            capability_base64url: capability.to_base64url(),
        };
        write_receiver_record(record_path, &record)?;
        Ok(Self {
            listener,
            record_path: record_path.to_path_buf(),
            capability,
        })
    }

    /// Poll for one pending loopback connection without blocking.
    ///
    /// Returns `Ok(None)` when no connection is pending or when a non-loopback
    /// peer is rejected, so the receive loop can re-check its stop signal.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] on a hard accept failure that is not a would-block.
    pub fn poll_accept(&self) -> Result<Option<TcpStream>, AtmError> {
        match self.listener.accept() {
            Ok((stream, peer)) => {
                if peer.ip().is_loopback() {
                    // BSD-derived platforms (including macOS) inherit the
                    // listener's non-blocking flag on accepted sockets. Force
                    // the served connection back to blocking so the
                    // timeout-bounded request/response reads behave uniformly.
                    stream.set_nonblocking(false).map_err(|_source| {
                        AtmError::daemon_unavailable(
                            "failed to configure blocking graft receiver connection",
                        )
                    })?;
                    Ok(Some(stream))
                } else {
                    Ok(None)
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(_source) => Err(AtmError::daemon_unavailable(format!(
                "failed while accepting graft receiver connection at {}",
                self.record_path.display()
            ))),
        }
    }

    /// Read one capability-authenticated request from an accepted connection.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the deadline cannot be applied, the frame
    /// cannot be read, or the presented capability is invalid.
    pub fn read_request(
        &self,
        stream: &mut TcpStream,
        io_deadline: Duration,
    ) -> Result<GraftPostSendRequest, AtmError> {
        apply_stream_deadlines(stream, io_deadline)?;
        let wire: GraftPostSendWireRequest = read_graft_post_send_message(
            stream,
            "failed to read graft post-send request",
            "graft post-send request exceeded the bounded payload cap",
        )?;
        if !self.capability.matches_header(&wire.capability_base64url) {
            return Err(AtmError::validation(
                "graft post-send request presented an invalid local capability",
            ));
        }
        Ok(wire.request)
    }

    /// Write one response frame and flush it to the caller.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the frame cannot be written or flushed.
    pub fn write_response(
        &self,
        stream: &mut TcpStream,
        response: &GraftPostSendResponse,
    ) -> Result<(), AtmError> {
        write_graft_post_send_message(
            stream,
            response,
            "failed to write graft post-send response",
            "graft post-send response exceeded the bounded payload cap",
        )?;
        stream.flush().map_err(|_source| {
            AtmError::daemon_unavailable("failed to flush graft post-send response")
        })
    }

    /// Loopback address this receiver is bound to (test/inspection use).
    pub fn local_addr(&self) -> Result<SocketAddr, AtmError> {
        self.listener.local_addr().map_err(|_source| {
            AtmError::daemon_unavailable("failed to resolve graft receiver endpoint address")
        })
    }
}

impl Drop for GraftReceiverListener {
    fn drop(&mut self) {
        // Best-effort removal of the published record so a stale endpoint does
        // not advertise a closed socket to future senders.
        let _ = fs::remove_file(&self.record_path);
    }
}

/// Deliver one post-send nudge to an embedded agent's loopback receiver.
///
/// This is the shared sender used by both the CLI post-send hook and the
/// daemon dispatcher. It reads the receiver's endpoint record, connects to the
/// advertised loopback port within `connect_deadline`, and exchanges one
/// capability-authenticated request/response within `io_deadline`.
///
/// # Errors
///
/// Returns [`AtmError`] when the record is missing/invalid, the receiver cannot
/// be reached, or the request/response exchange fails.
pub fn deliver_graft_post_send(
    record_path: &Path,
    request: &GraftPostSendRequest,
    connect_deadline: Duration,
    io_deadline: Duration,
) -> Result<GraftPostSendResponse, AtmError> {
    let record = read_receiver_record(record_path)?;
    let endpoint = record.endpoint()?;
    let mut stream =
        TcpStream::connect_timeout(&endpoint, connect_deadline).map_err(|_source| {
            AtmError::new(
                AtmErrorCode::PostSendGraftUnavailable,
                format!(
                    "failed to connect to graft receiver endpoint {} within {:?}",
                    endpoint, connect_deadline
                ),
            )
        })?;
    apply_stream_deadlines(&stream, io_deadline)?;
    let wire = GraftPostSendWireRequest {
        capability_base64url: record.capability_base64url.clone(),
        request: request.clone(),
    };
    write_graft_post_send_message(
        &mut stream,
        &wire,
        "failed to write graft post-send request",
        "graft post-send request exceeded the bounded payload cap",
    )?;
    stream.flush().map_err(|_source| {
        AtmError::daemon_unavailable("failed to flush graft post-send request")
    })?;
    read_graft_post_send_message(
        &mut stream,
        "failed to read graft post-send response",
        "graft post-send response exceeded the bounded payload cap",
    )
}

fn apply_stream_deadlines(stream: &TcpStream, io_deadline: Duration) -> Result<(), AtmError> {
    stream
        .set_read_timeout(Some(io_deadline))
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to apply graft post-send read timeout")
        })?;
    stream
        .set_write_timeout(Some(io_deadline))
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to apply graft post-send write timeout")
        })?;
    Ok(())
}

fn prepare_receiver_record_parent(record_path: &Path) -> Result<(), AtmError> {
    if let Some(parent) = record_path.parent() {
        fs::create_dir_all(parent).map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to prepare graft receiver directory {}",
                parent.display()
            ))
        })?;
    }
    if record_path.exists() {
        fs::remove_file(record_path).map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to remove stale graft receiver endpoint record {}",
                record_path.display()
            ))
        })?;
    }
    Ok(())
}

/// Restrict the endpoint record to the owner on Unix.
///
/// The record carries the capability token, so it must not be group/world
/// readable. On Windows the record inherits the user profile ACL and the
/// loopback bind plus per-connection capability remain the authenticating gate.
#[cfg(unix)]
fn apply_owner_only_record_mode(options: &mut fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn apply_owner_only_record_mode(_options: &mut fs::OpenOptions) {}

fn write_receiver_record(
    record_path: &Path,
    record: &GraftReceiverEndpointRecord,
) -> Result<(), AtmError> {
    let bytes = serde_json::to_vec(record)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    apply_owner_only_record_mode(&mut options);
    let mut file = options.open(record_path).map_err(|_source| {
        AtmError::daemon_unavailable(format!(
            "failed to publish graft receiver endpoint record at {}",
            record_path.display()
        ))
    })?;
    file.write_all(&bytes).map_err(|_source| {
        AtmError::daemon_unavailable(format!(
            "failed to write graft receiver endpoint record at {}",
            record_path.display()
        ))
    })
}

fn read_receiver_record(record_path: &Path) -> Result<GraftReceiverEndpointRecord, AtmError> {
    let bytes = fs::read(record_path).map_err(|_source| {
        AtmError::new(
            AtmErrorCode::PostSendGraftUnavailable,
            format!(
                "graft receiver endpoint record is unavailable at {}",
                record_path.display()
            ),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|_source| {
        AtmError::validation(format!(
            "failed to decode graft receiver endpoint record at {}",
            record_path.display()
        ))
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
    use super::{
        AtmGraftClient, GraftPostSendRequest, GraftPostSendResponse, GraftReceiverListener,
        deliver_graft_post_send, graft_receiver_record_path_from_home,
    };
    use crate::ack::{AckOutcome, AckRequest};
    use crate::boundary::PostSendHookEvent;
    use crate::error::AtmError;
    use crate::read::{ReadOutcome, ReadQuery};
    use crate::schema::AtmMessageId;
    use crate::send::{SendOutcome, SendRequest};
    use crate::test_support::{TEST_LEAD, TEST_QA, TEST_TEAM};
    use crate::types::{AgentName, TeamName};
    use std::net::TcpStream;
    use std::time::Duration;
    use tempfile::TempDir;

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

    fn test_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: AgentName::from_validated(TEST_LEAD),
            sender_chat_id: None,
            sender_team: TeamName::from_validated(TEST_TEAM),
            recipient: AgentName::from_validated(TEST_QA),
            recipient_team: TeamName::from_validated(TEST_TEAM),
            message_id: AtmMessageId::new(),
            description: "loopback graft transport".to_string(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            recipient_pane_id: None,
        }
    }

    #[test]
    fn loopback_receiver_round_trips_a_capability_authenticated_request() {
        let tempdir = TempDir::new().expect("tempdir");
        let record_path = graft_receiver_record_path_from_home(
            tempdir.path(),
            &TeamName::from_validated(TEST_TEAM),
            &AgentName::from_validated(TEST_QA),
        );
        let listener = GraftReceiverListener::bind(&record_path).expect("bind listener");

        let request = GraftPostSendRequest {
            event: test_event(),
        };
        let sender = std::thread::spawn({
            let record_path = record_path.clone();
            move || {
                deliver_graft_post_send(
                    &record_path,
                    &request,
                    Duration::from_secs(1),
                    Duration::from_secs(3),
                )
            }
        });

        let mut stream = loop {
            if let Some(stream) = listener.poll_accept().expect("poll accept") {
                break stream;
            }
            std::thread::yield_now();
        };
        let received = listener
            .read_request(&mut stream, Duration::from_secs(3))
            .expect("read request");
        assert_eq!(received.event.description, "loopback graft transport");
        listener
            .write_response(&mut stream, &GraftPostSendResponse::Delivered)
            .expect("write response");

        let response = sender.join().expect("join sender").expect("deliver");
        assert_eq!(response, GraftPostSendResponse::Delivered);
    }

    #[test]
    fn receiver_rejects_a_forged_capability() {
        let tempdir = TempDir::new().expect("tempdir");
        let record_path = graft_receiver_record_path_from_home(
            tempdir.path(),
            &TeamName::from_validated(TEST_TEAM),
            &AgentName::from_validated(TEST_QA),
        );
        let listener = GraftReceiverListener::bind(&record_path).expect("bind listener");
        let endpoint = listener.local_addr().expect("local addr");

        let forger = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(endpoint).expect("connect to loopback receiver");
            let wire = super::GraftPostSendWireRequest {
                capability_base64url: "not-the-real-capability".to_string(),
                request: GraftPostSendRequest {
                    event: test_event(),
                },
            };
            super::write_graft_post_send_message(&mut stream, &wire, "write", "oversized")
        });

        let mut stream = loop {
            if let Some(stream) = listener.poll_accept().expect("poll accept") {
                break stream;
            }
            std::thread::yield_now();
        };
        let error = listener
            .read_request(&mut stream, Duration::from_secs(3))
            .expect_err("forged capability must be rejected");
        assert!(
            error.message().contains("invalid local capability"),
            "{error:?}"
        );
        let _ = forger.join().expect("join forger");
    }
}
