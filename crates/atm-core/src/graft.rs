//! Thin graft-facing daemon client contracts shared by embedded host agents.
//!
//! The graft received-message notification channel is a same-host loopback
//! control plane: an embedded agent binds a loopback [`GraftReceiverListener`]
//! and the daemon's post-persistence receiver hook delivers nudges to it via
//! [`deliver_graft_post_send`]. The transport is loopback TCP plus a per-bind
//! [`LocalCapability`] token so it works identically on Unix and Windows
//! without any local-socket / named-pipe backend.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use fs2::FileExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::ack::{AckOutcome, AckRequest};
use crate::api::RequestDeadline;
use crate::boundary::{
    BuiltInPostSendDispatch, GraftNudgeTarget, PostSendBuiltInTarget, PostSendEmissionPath,
    PostSendHookEvent,
};
use crate::error::{AtmError, AtmErrorCode};
use crate::local_http::LocalCapability;
use crate::read::{ReadOutcome, ReadQuery};
use crate::schema::canonical_graft_root;
use crate::send::{SendOutcome, SendRequest};
use crate::service_runtime::RetainedServiceRuntime;
use crate::types::{AgentName, ChatId, TeamName};

pub const MAX_GRAFT_POST_SEND_FRAME_BYTES: usize = 1024 * 1024;

/// Schema version stamped into the graft receiver endpoint record.
pub const GRAFT_RECEIVER_RECORD_SCHEMA_VERSION: u8 = 2;

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

const RECEIVER_HOOK_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
const RECEIVER_HOOK_IO_DEADLINE: Duration = Duration::from_secs(3);

/// Delivers one serialized receiver event to an independently published Graft
/// endpoint. This is endpoint transport, not a daemon-side hook implementation.
/// Delivers one received-message hook through the recipient's independently
/// published Graft endpoint.
///
/// Replacement composition invokes this only from its narrow blocking seam;
/// the Tokio HTTP runtime itself remains independent of Graft.
pub fn deliver_published_receiver_hook_from_local_runtime(
    runtime: &crate::LocalServiceRuntime,
    dispatch: &BuiltInPostSendDispatch,
    deadline: RequestDeadline,
) -> Result<PostSendEmissionPath, AtmError> {
    deliver_published_receiver_hook(runtime, dispatch, deadline)
}

pub(crate) fn deliver_published_receiver_hook<R>(
    runtime: &R,
    dispatch: &BuiltInPostSendDispatch,
    deadline: RequestDeadline,
) -> Result<PostSendEmissionPath, AtmError>
where
    R: RetainedServiceRuntime + ?Sized,
{
    let PostSendBuiltInTarget::Graft(GraftNudgeTarget {
        recipient,
        recipient_team,
    }) = &dispatch.target
    else {
        return Err(AtmError::validation(
            "published receiver transport received a non-graft target",
        ));
    };
    let Some(member) = runtime.load_roster_member(recipient_team, recipient)? else {
        return Err(AtmError::new(
            AtmErrorCode::PostSendGraftUnavailable,
            "receiver endpoint is unavailable because the recipient is absent from the roster",
        ));
    };
    let root = canonical_graft_root(&member.metadata_json).ok_or_else(|| {
        AtmError::new(
            AtmErrorCode::PostSendGraftUnavailable,
            "receiver endpoint is unavailable because the recipient has no published root",
        )
    })?;
    let endpoint_record_path =
        graft_receiver_record_path_from_root(root.as_path(), recipient_team, recipient);
    match deliver_graft_post_send_with_deadline(
        &endpoint_record_path,
        &GraftPostSendRequest {
            event: dispatch.event.clone(),
        },
        deadline,
    )? {
        GraftPostSendResponse::Delivered => Ok(PostSendEmissionPath::GraftPort),
        GraftPostSendResponse::Error(error) => Err(error),
    }
}

fn remaining_hook_budget(
    deadline: RequestDeadline,
    safety_cap: Duration,
) -> Result<Duration, AtmError> {
    deadline
        .remaining()
        .map(|remaining| remaining.min(safety_cap))
        .ok_or_else(|| {
            AtmError::new(
                AtmErrorCode::PostSendGraftUnavailable,
                "received-message hook request deadline expired before endpoint delivery began",
            )
        })
}

fn deliver_graft_post_send_with_deadline(
    record_path: &Path,
    request: &GraftPostSendRequest,
    deadline: RequestDeadline,
) -> Result<GraftPostSendResponse, AtmError> {
    let record = read_receiver_record(record_path)?;
    let endpoint = record.endpoint()?;
    let connect_deadline = remaining_hook_budget(deadline, RECEIVER_HOOK_CONNECT_DEADLINE)?;
    let mut stream = TcpStream::connect_timeout(&endpoint, connect_deadline).map_err(|source| {
        AtmError::new(
            AtmErrorCode::PostSendGraftUnavailable,
            format!(
                "failed to connect to graft receiver endpoint {} within {:?}",
                endpoint, connect_deadline
            ),
        )
        .with_cause(source)
    })?;
    let io_deadline = remaining_hook_budget(deadline, RECEIVER_HOOK_IO_DEADLINE)?;
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
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush graft post-send request").with_cause(source)
    })?;
    read_graft_post_send_message(
        &mut stream,
        "failed to read graft post-send response",
        "graft post-send response exceeded the bounded payload cap",
    )
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
    pub owner_generation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_chat_id: Option<ChatId>,
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
        if self.owner_generation.parse::<Ulid>().is_err() {
            return Err(AtmError::validation(
                "graft receiver endpoint record contains an invalid owner generation",
            ));
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
    graft_receiver_record_path_from_root(home_dir, team, agent)
}

/// Absolute path of a loopback endpoint record under the canonical graft root.
///
/// The root is intentionally supplied by the caller so publishers and daemon
/// resolvers share exactly the same path construction once roster metadata has
/// selected the recipient's authoritative workspace root.
pub fn graft_receiver_record_path_from_root(
    graft_root: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> PathBuf {
    graft_root
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
        .map_err(|source| AtmError::daemon_unavailable(write_error).with_cause(source))
}

pub fn read_graft_post_send_message<T: DeserializeOwned>(
    reader: &mut impl Read,
    read_error: &'static str,
    oversize_error: &'static str,
) -> Result<T, AtmError> {
    let mut header = [0u8; 4];
    reader
        .read_exact(&mut header)
        .map_err(|source| AtmError::daemon_unavailable(read_error).with_cause(source))?;
    let payload_len = u32::from_be_bytes(header) as usize;
    if payload_len > MAX_GRAFT_POST_SEND_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(oversize_error));
    }
    let mut bytes = vec![0u8; payload_len];
    reader
        .read_exact(&mut bytes)
        .map_err(|source| AtmError::daemon_unavailable(read_error).with_cause(source))?;
    serde_json::from_slice(&bytes).map_err(|source| {
        AtmError::validation("failed to decode graft post-send message").with_cause(source)
    })
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
    owner_generation: String,
    capability: LocalCapability,
    _ownership: ReceiverOwnershipGuard,
}

/// Process-lifetime exclusive ownership of one receiver record path.
///
/// The OS releases this advisory lock when a crashed receiver exits, which is
/// why endpoint-record existence is never treated as the ownership authority.
struct ReceiverOwnershipGuard {
    lock_file: File,
}

impl ReceiverOwnershipGuard {
    fn acquire(record_path: &Path) -> Result<Self, AtmError> {
        let lock_path = receiver_ownership_lock_path(record_path);
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| {
                AtmError::daemon_unavailable_with_cause(
                    format!(
                        "failed to open graft receiver ownership lock at {}",
                        lock_path.display()
                    ),
                    source,
                )
            })?;
        match lock_file.try_lock_exclusive() {
            Ok(()) => {
                tracing::info!(record_path = %record_path.display(), action = "receiver_ownership", outcome = "acquired", "graft receiver ownership acquired");
                Ok(Self { lock_file })
            }
            Err(source) if is_lock_contention(&source) => {
                tracing::warn!(record_path = %record_path.display(), action = "receiver_ownership", outcome = "conflict", "graft receiver ownership already active");
                Err(AtmError::new(
                    AtmErrorCode::GraftReceiverAlreadyActive,
                    graft_receiver_identity(record_path),
                ))
            }
            Err(source) => Err(AtmError::daemon_unavailable_with_cause(
                format!(
                    "failed to acquire graft receiver ownership lock at {}",
                    lock_path.display()
                ),
                source,
            )),
        }
    }
}

impl Drop for ReceiverOwnershipGuard {
    fn drop(&mut self) {
        let _ = self.lock_file.unlock();
    }
}

impl GraftReceiverListener {
    /// Bind the loopback receiver and publish its endpoint record.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the loopback socket cannot be bound or the
    /// endpoint record cannot be published for the owner.
    pub fn bind(record_path: &Path, owner_chat_id: Option<ChatId>) -> Result<Self, AtmError> {
        prepare_receiver_record_parent(record_path)?;
        let ownership = ReceiverOwnershipGuard::acquire(record_path)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to bind graft receiver endpoint for {}",
                record_path.display()
            ))
            .with_cause(source)
        })?;
        listener.set_nonblocking(true).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to configure non-blocking graft receiver endpoint for {}",
                record_path.display()
            ))
            .with_cause(source)
        })?;
        let loopback = listener.local_addr().map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to resolve graft receiver endpoint address for {}",
                record_path.display()
            ))
            .with_cause(source)
        })?;
        let capability = LocalCapability::generate()?;
        let owner_generation = Ulid::new().to_string();
        let record = GraftReceiverEndpointRecord {
            schema_version: GRAFT_RECEIVER_RECORD_SCHEMA_VERSION,
            owner_generation: owner_generation.clone(),
            owner_chat_id,
            loopback,
            capability_base64url: capability.to_base64url(),
        };
        write_receiver_record(record_path, &record)?;
        Ok(Self {
            listener,
            record_path: record_path.to_path_buf(),
            owner_generation,
            capability,
            _ownership: ownership,
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
                    stream.set_nonblocking(false).map_err(|source| {
                        AtmError::daemon_unavailable(
                            "failed to configure blocking graft receiver connection",
                        )
                        .with_cause(source)
                    })?;
                    Ok(Some(stream))
                } else {
                    Ok(None)
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(source) => Err(AtmError::daemon_unavailable(format!(
                "failed while accepting graft receiver connection at {}",
                self.record_path.display()
            ))
            .with_cause(source)),
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
        stream.flush().map_err(|source| {
            AtmError::daemon_unavailable("failed to flush graft post-send response")
                .with_cause(source)
        })
    }

    /// Loopback address this receiver is bound to (test/inspection use).
    pub fn local_addr(&self) -> Result<SocketAddr, AtmError> {
        self.listener.local_addr().map_err(|source| {
            AtmError::daemon_unavailable("failed to resolve graft receiver endpoint address")
                .with_cause(source)
        })
    }
}

impl Drop for GraftReceiverListener {
    fn drop(&mut self) {
        // Only the generation that published this record may remove it. This
        // prevents an old listener from erasing a successor after a reclaim.
        if let Ok(record) = read_receiver_record(&self.record_path)
            && record.owner_generation == self.owner_generation
        {
            let _ = fs::remove_file(&self.record_path);
            tracing::info!(record_path = %self.record_path.display(), action = "receiver_record_cleanup", outcome = "removed", "graft receiver removed its owned endpoint record");
        } else {
            tracing::info!(record_path = %self.record_path.display(), action = "receiver_record_cleanup", outcome = "retained", "graft receiver retained successor or malformed endpoint record");
        }
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
    let mut stream = TcpStream::connect_timeout(&endpoint, connect_deadline).map_err(|source| {
        AtmError::new(
            AtmErrorCode::PostSendGraftUnavailable,
            format!(
                "failed to connect to graft receiver endpoint {} within {:?}",
                endpoint, connect_deadline
            ),
        )
        .with_cause(source)
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
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush graft post-send request").with_cause(source)
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
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to apply graft post-send read timeout")
                .with_cause(source)
        })?;
    stream
        .set_write_timeout(Some(io_deadline))
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to apply graft post-send write timeout")
                .with_cause(source)
        })?;
    Ok(())
}

fn prepare_receiver_record_parent(record_path: &Path) -> Result<(), AtmError> {
    if let Some(parent) = record_path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to prepare graft receiver directory {}",
                parent.display()
            ))
            .with_cause(source)
        })?;
    }
    Ok(())
}

fn receiver_ownership_lock_path(record_path: &Path) -> PathBuf {
    record_path.with_extension("lock")
}

fn is_lock_contention(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock
        || cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33))
}

fn graft_receiver_identity(record_path: &Path) -> String {
    let agent = record_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown-agent");
    let team = record_path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .unwrap_or("unknown-team");
    format!(
        "receiver already active for {agent}@{team} ({})",
        record_path.display()
    )
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
    let mut file = options.open(record_path).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to publish graft receiver endpoint record at {}",
            record_path.display()
        ))
        .with_cause(source)
    })?;
    file.write_all(&bytes).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to write graft receiver endpoint record at {}",
            record_path.display()
        ))
        .with_cause(source)
    })
}

fn read_receiver_record(record_path: &Path) -> Result<GraftReceiverEndpointRecord, AtmError> {
    let bytes = fs::read(record_path).map_err(|source| {
        AtmError::new(
            AtmErrorCode::PostSendGraftUnavailable,
            format!(
                "graft receiver endpoint record is unavailable at {}",
                record_path.display()
            ),
        )
        .with_cause(source)
    })?;
    let record: GraftReceiverEndpointRecord = serde_json::from_slice(&bytes).map_err(|source| {
        AtmError::validation(format!(
            "failed to decode graft receiver endpoint record at {}",
            record_path.display()
        ))
        .with_cause(source)
    })?;
    // Decode fail-closed: old schemas and malformed generations are never
    // returned as an apparently usable receiver record.
    record.endpoint()?;
    Ok(record)
}

/// Open unary client surface for embedded ATM consumers.
///
/// This trait is intentionally not sealed. `atm-graft` must be able to
/// implement the concrete same-host client in a separate crate without taking
/// a Rust dependency on `atm-daemon`.
#[async_trait::async_trait]
pub trait AtmGraftClient: Send + Sync {
    /// Execute one send-shaped ATM compose request.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the underlying daemon-backed send path cannot
    /// complete successfully.
    async fn send_message(&self, request: SendRequest) -> Result<SendOutcome, AtmError>;

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
        AtmGraftClient, GRAFT_RECEIVER_RECORD_SCHEMA_VERSION, GraftPostSendRequest,
        GraftPostSendResponse, GraftReceiverEndpointRecord, GraftReceiverListener,
        deliver_graft_post_send, graft_receiver_record_path_from_home, read_receiver_record,
        write_receiver_record,
    };
    use crate::ack::{AckOutcome, AckRequest};
    use crate::boundary::PostSendHookEvent;
    use crate::error::AtmError;
    use crate::read::{ReadOutcome, ReadQuery};
    use crate::schema::AtmMessageId;
    use crate::send::{SendOutcome, SendRequest};
    use crate::test_support::{TEST_LEAD, TEST_QA, TEST_TEAM};
    use crate::types::{AgentName, ChatId, TeamName};
    use std::fs;
    use std::net::TcpStream;
    use std::time::Duration;
    use tempfile::TempDir;

    #[derive(Debug)]
    struct MockGraftClient;

    #[async_trait::async_trait]
    impl AtmGraftClient for MockGraftClient {
        async fn send_message(&self, _request: SendRequest) -> Result<SendOutcome, AtmError> {
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
        let listener = GraftReceiverListener::bind(&record_path, None).expect("bind listener");

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
        let listener = GraftReceiverListener::bind(&record_path, None).expect("bind listener");
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

    #[test]
    fn receiver_record_rejects_old_schema_and_malformed_generation() {
        let tempdir = TempDir::new().expect("tempdir");
        let record_path = graft_receiver_record_path_from_home(
            tempdir.path(),
            &TeamName::from_validated(TEST_TEAM),
            &AgentName::from_validated(TEST_QA),
        );
        super::prepare_receiver_record_parent(&record_path).expect("record parent");
        let old = GraftReceiverEndpointRecord {
            schema_version: GRAFT_RECEIVER_RECORD_SCHEMA_VERSION - 1,
            owner_generation: ulid::Ulid::new().to_string(),
            owner_chat_id: None,
            loopback: "127.0.0.1:7".parse().expect("address"),
            capability_base64url: "capability".to_string(),
        };
        write_receiver_record(&record_path, &old).expect("write old record");
        assert!(read_receiver_record(&record_path).is_err());
        let malformed = GraftReceiverEndpointRecord {
            schema_version: GRAFT_RECEIVER_RECORD_SCHEMA_VERSION,
            owner_generation: "not-a-ulid".to_string(),
            ..old
        };
        write_receiver_record(&record_path, &malformed).expect("write malformed record");
        assert!(read_receiver_record(&record_path).is_err());
    }

    #[test]
    fn live_owner_conflict_preserves_record_and_owner_metadata() {
        let tempdir = TempDir::new().expect("tempdir");
        let team = TeamName::from_validated(TEST_TEAM);
        let agent = AgentName::from_validated(TEST_QA);
        let record_path = graft_receiver_record_path_from_home(tempdir.path(), &team, &agent);
        let chat_id = "chat-1".parse::<ChatId>().expect("chat id");
        let first =
            GraftReceiverListener::bind(&record_path, Some(chat_id.clone())).expect("first");
        let before = fs::read(&record_path).expect("record bytes");
        let error = match GraftReceiverListener::bind(&record_path, Some(chat_id)) {
            Ok(_) => panic!("second live owner must fail"),
            Err(error) => error,
        };
        assert_eq!(
            error.code(),
            crate::error::AtmErrorCode::GraftReceiverAlreadyActive
        );
        assert!(error.message().contains(TEST_QA));
        assert_eq!(fs::read(&record_path).expect("record bytes"), before);
        let record = read_receiver_record(&record_path).expect("record");
        assert!(record.owner_chat_id.is_some());
        drop(first);
    }

    #[test]
    fn old_owner_cleanup_cannot_remove_successor_generation() {
        let tempdir = TempDir::new().expect("tempdir");
        let record_path = graft_receiver_record_path_from_home(
            tempdir.path(),
            &TeamName::from_validated(TEST_TEAM),
            &AgentName::from_validated(TEST_QA),
        );
        let listener = GraftReceiverListener::bind(&record_path, None).expect("owner");
        let current = read_receiver_record(&record_path).expect("current record");
        let successor = GraftReceiverEndpointRecord {
            owner_generation: ulid::Ulid::new().to_string(),
            ..current
        };
        write_receiver_record(&record_path, &successor).expect("publish successor");
        drop(listener);
        assert_eq!(
            read_receiver_record(&record_path)
                .expect("successor remains")
                .owner_generation,
            successor.owner_generation
        );
    }

    #[test]
    fn distinct_receiver_identities_can_listen_concurrently() {
        let tempdir = TempDir::new().expect("tempdir");
        let team = TeamName::from_validated(TEST_TEAM);
        let first_path = graft_receiver_record_path_from_home(
            tempdir.path(),
            &team,
            &AgentName::from_validated(TEST_QA),
        );
        let second_path = graft_receiver_record_path_from_home(
            tempdir.path(),
            &team,
            &AgentName::from_validated(TEST_LEAD),
        );
        let first = GraftReceiverListener::bind(&first_path, None).expect("first");
        let second = GraftReceiverListener::bind(&second_path, None).expect("second");
        assert_ne!(
            first.local_addr().expect("first addr"),
            second.local_addr().expect("second addr")
        );
    }
}
