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
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

use fs4::fs_std::FileExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::api::RequestDeadline;
use crate::boundary::{
    BuiltInPostSendDispatch, GraftNudgeTarget, NudgeKind, PostSendBuiltInTarget,
    PostSendEmissionPath, PostSendHookEvent,
};
use crate::error::{AtmError, AtmErrorCode};
use crate::list::{ListOutcome, ListQuery};
use crate::local_http::LocalCapability;
use crate::protocol::OwnerGeneration;
use crate::read::{ReadOutcome, ReadQuery};
use crate::send::{SendOutcome, SendRequest};
use crate::service_runtime::RetainedServiceRuntime;
use crate::types::{AgentName, ChatId, TeamName};

pub const MAX_GRAFT_POST_SEND_FRAME_BYTES: usize = 1024 * 1024;

/// Interval between non-blocking accept polls in the receiver loop.
pub const GRAFT_RECEIVER_ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftPostSendRequest {
    pub event: PostSendHookEvent,
    /// The ATM nudge taxonomy kind carried over the graft channel. Missing
    /// kind is the pre-AQ2 wire shape and is interpreted as the historical
    /// immediate steer.
    #[serde(default = "default_graft_kind")]
    pub kind: NudgeKind,
    /// Canonical database-resolved `<atm …>` nudge text. The receiver must
    /// inject this text, never substitute the stored message description.
    pub rendered_nudge: String,
    /// Immutable message content associated with `rendered_nudge`.
    pub message_body: String,
}

const fn default_graft_kind() -> NudgeKind {
    NudgeKind::Steer
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GraftPostSendResponse {
    Delivered,
    Error(AtmError),
}

const RECEIVER_HOOK_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
const RECEIVER_HOOK_IO_DEADLINE: Duration = Duration::from_secs(3);
/// The outer replacement-runtime hook owner needs time to receive the blocking
/// Graft result and turn it into a durable-success warning. Socket I/O must
/// therefore end before, rather than race, the inherited absolute deadline.
const RECEIVER_HOOK_RESULT_HANDOFF_GRACE: Duration = Duration::from_millis(100);

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
        rendered_nudge,
        message_body,
    }) = &dispatch.target
    else {
        return Err(AtmError::validation(
            "published receiver transport received a non-graft target",
        ));
    };
    if runtime
        .load_roster_member(recipient_team, recipient)?
        .is_none()
    {
        return Err(AtmError::new(
            AtmErrorCode::PostSendGraftUnavailable,
            "receiver endpoint is unavailable because the recipient is absent from the roster",
        ));
    };
    let Some(lease) = runtime.graft_receiver_lease(recipient_team, recipient)? else {
        return Err(graft_receiver_not_registered_error(
            recipient_team,
            recipient,
        ));
    };
    let response = deliver_graft_post_send_with_deadline(
        lease.endpoint,
        &lease.capability,
        &GraftPostSendRequest {
            event: dispatch.event.clone(),
            kind: dispatch.kind,
            rendered_nudge: rendered_nudge.clone(),
            message_body: message_body.clone(),
        },
        deadline,
    );
    if let Err(error) = &response
        && error.code() == AtmErrorCode::PostSendGraftUnavailable
        && let Err(mark_error) = runtime.mark_graft_receiver_unreachable(
            recipient_team,
            recipient,
            &lease.owner_generation,
            chrono::Utc::now(),
        )
    {
        tracing::warn!(
            recipient = %recipient,
            recipient_team = %recipient_team,
            error_code = %mark_error.code(),
            error_message = %mark_error.message(),
            "failed to record unreachable graft receiver"
        );
    }
    match response? {
        GraftPostSendResponse::Delivered => Ok(PostSendEmissionPath::GraftPort),
        GraftPostSendResponse::Error(error) => Err(error),
    }
}

pub fn graft_receiver_not_registered_error(team: &TeamName, agent: &AgentName) -> AtmError {
    AtmError::new(
        AtmErrorCode::PostSendGraftUnavailable,
        format!("graft receiver is not registered for {agent}@{team}"),
    )
}

fn remaining_hook_budget(
    deadline: RequestDeadline,
    safety_cap: Duration,
) -> Result<Duration, AtmError> {
    deadline
        .remaining()
        .and_then(|remaining| remaining.checked_sub(RECEIVER_HOOK_RESULT_HANDOFF_GRACE))
        .filter(|remaining| !remaining.is_zero())
        .map(|remaining| remaining.min(safety_cap))
        .ok_or_else(|| {
            AtmError::new(
                AtmErrorCode::PostSendGraftUnavailable,
                "received-message hook does not retain enough request budget for endpoint delivery and result handoff",
            )
        })
}

fn deliver_graft_post_send_with_deadline(
    endpoint: SocketAddr,
    capability: &LocalCapability,
    request: &GraftPostSendRequest,
    deadline: RequestDeadline,
) -> Result<GraftPostSendResponse, AtmError> {
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
        capability_base64url: capability.to_base64url(),
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

/// Absolute path of the receiver ownership lock under the canonical graft root.
///
/// The lock retains the historical `.atm/graft/<team>/<agent>.lock` location
/// so existing same-host ownership files remain valid after endpoint
/// publication moved entirely into the daemon registry.
pub fn graft_receiver_lock_path_from_root(
    graft_root: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> std::path::PathBuf {
    graft_root
        .join(".atm")
        .join("graft")
        .join(team.as_str())
        .join(format!("{agent}.lock"))
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
    owner_generation: OwnerGeneration,
    capability: LocalCapability,
    _ownership: ReceiverOwnershipGuard,
}

/// Process-lifetime exclusive ownership of one receiver identity.
///
/// The OS releases this advisory lock when a crashed receiver exits, which is
/// why endpoint-record existence is never treated as the ownership authority.
struct ReceiverOwnershipGuard {
    lock_file: File,
}

impl ReceiverOwnershipGuard {
    fn acquire(lock_path: &Path, team: &TeamName, agent: &AgentName) -> Result<Self, AtmError> {
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(lock_path)
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
            Ok(true) => {
                tracing::info!(team = %team, agent = %agent, action = "receiver_ownership", outcome = "acquired", "graft receiver ownership acquired");
                Ok(Self { lock_file })
            }
            Ok(false) => {
                tracing::warn!(team = %team, agent = %agent, action = "receiver_ownership", outcome = "conflict", "graft receiver ownership already active");
                Err(AtmError::new(
                    AtmErrorCode::GraftReceiverAlreadyActive,
                    format!("receiver already active for {agent}@{team}"),
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
    /// Bind the loopback receiver and acquire its same-host ownership lock.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the loopback socket cannot be bound or the
    /// ownership lock or the loopback socket cannot be acquired.
    pub fn bind(
        graft_root: &Path,
        team: &TeamName,
        agent: &AgentName,
        _owner_chat_id: Option<ChatId>,
    ) -> Result<Self, AtmError> {
        let lock_path = graft_receiver_lock_path_from_root(graft_root, team, agent);
        prepare_receiver_lock_parent(&lock_path)?;
        let ownership = ReceiverOwnershipGuard::acquire(&lock_path, team, agent)?;
        remove_legacy_endpoint_file(graft_root, team, agent)?;
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to bind graft receiver endpoint for {agent}@{team}"
            ))
            .with_cause(source)
        })?;
        listener.set_nonblocking(true).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to configure non-blocking graft receiver endpoint for {agent}@{team}"
            ))
            .with_cause(source)
        })?;
        let capability = LocalCapability::generate()?;
        // A freshly minted ULID is always a valid `OwnerGeneration`; storing
        // the already-validated newtype here (RBP-F002) means every later
        // read of this listener's generation is a cheap clone instead of a
        // re-parse of a raw string on every ~1s refresh tick.
        let owner_generation = OwnerGeneration::new(Ulid::new().to_string())
            .expect("a freshly minted ULID is always a valid owner generation");
        Ok(Self {
            listener,
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
            Err(source) => Err(AtmError::daemon_unavailable(
                "failed while accepting graft receiver connection",
            )
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

    /// Return the capability used to authenticate this receiver's sender.
    pub fn capability(&self) -> &LocalCapability {
        &self.capability
    }

    /// Return the generation that owns this receiver binding.
    pub fn owner_generation(&self) -> &OwnerGeneration {
        &self.owner_generation
    }
}

/// Deliver one post-send nudge to an embedded agent's loopback receiver.
///
/// This is the shared sender used by both the CLI post-send hook and the
/// daemon dispatcher. It connects to the supplied loopback endpoint within
/// `connect_deadline`, and exchanges one capability-authenticated
/// request/response within `io_deadline`.
///
/// # Errors
///
/// Returns [`AtmError`] when the receiver cannot be reached or the
/// request/response exchange fails.
pub fn deliver_graft_post_send(
    endpoint: SocketAddr,
    capability: &LocalCapability,
    request: &GraftPostSendRequest,
    connect_deadline: Duration,
    io_deadline: Duration,
) -> Result<GraftPostSendResponse, AtmError> {
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
        capability_base64url: capability.to_base64url(),
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

fn prepare_receiver_lock_parent(lock_path: &Path) -> Result<(), AtmError> {
    if let Some(parent) = lock_path.parent() {
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

fn remove_legacy_endpoint_file(
    graft_root: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<(), AtmError> {
    let path = graft_root
        .join(".atm")
        .join("graft")
        .join(team.as_str())
        .join(format!("{agent}.json"));
    match fs::remove_file(&path) {
        Ok(()) => {
            tracing::info!(path = %path.display(), action = "legacy_endpoint_cleanup", outcome = "removed", "removed obsolete graft endpoint artifact");
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(AtmError::daemon_unavailable(format!(
            "failed to remove obsolete graft endpoint artifact at {}",
            path.display()
        ))
        .with_cause(source)),
    }
}

/// Test-only snapshot of a graft receiver endpoint record's persisted fields.
///
/// [`GraftReceiverEndpointRecord`] is private, so sibling crates' tests
/// (e.g. `atm-graft`'s bind/announce acceptance test) cannot name it
/// directly. This owned, public snapshot exists solely to let those tests
/// literally diff what [`GraftReceiverListener::bind`] wrote to disk against
/// its own in-memory accessors, rather than trusting they were populated
/// from the same local variables.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Clone, PartialEq, Eq)]
#[doc(hidden)]
pub struct GraftReceiverEndpointRecordForTest {
    pub loopback: SocketAddr,
    pub capability_base64url: String,
    pub owner_generation: String,
}

/// Read and decode the on-disk graft receiver endpoint record at `record_path`.
///
/// Test-only: exposes the record's raw persisted fields for literal equality
/// assertions in sibling-crate tests. Production code must go through
/// [`deliver_graft_post_send`] instead.
///
/// # Errors
///
/// Returns [`AtmError`] when the record cannot be read or decoded.
#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)]
pub fn read_graft_receiver_endpoint_record_for_test(
    record_path: &Path,
) -> Result<GraftReceiverEndpointRecordForTest, AtmError> {
    let record = read_receiver_record(record_path)?;
    Ok(GraftReceiverEndpointRecordForTest {
        loopback: record.loopback,
        capability_base64url: record.capability_base64url,
        owner_generation: record.owner_generation,
    })
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

    /// Execute one ATM read request through the same Tokio/Axum daemon API
    /// path used by the CLI.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the read request cannot be delivered or the
    /// daemon returns a typed failure.
    async fn read_message(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError>;

    /// Execute one bounded mailbox metadata list through the ordinary daemon API.
    async fn list_messages(&self, _query: ListQuery) -> Result<ListOutcome, AtmError> {
        Err(AtmError::new(
            AtmErrorCode::CallerContextRequestInvalid,
            "this graft client does not support mailbox list operations",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AtmGraftClient, GraftPostSendRequest, GraftPostSendResponse, GraftPostSendWireRequest,
        GraftReceiverListener, RECEIVER_HOOK_RESULT_HANDOFF_GRACE, deliver_graft_post_send,
        deliver_published_receiver_hook, graft_receiver_lock_path_from_root, remaining_hook_budget,
    };
    use crate::api::RequestDeadline;
    use crate::boundary::{
        BuiltInPostSendDispatch, GraftNudgeTarget, NudgeKind, PostSendBuiltInTarget,
        PostSendEmissionPath, PostSendHookEvent, RosterEntry, RosterHarness, RosterMemberKind,
    };
    use crate::config::AtmConfig;
    use crate::delivery_policy::DeliveryRecipientSnapshot;
    use crate::error::AtmError;
    use crate::protocol::OwnerGeneration;
    use crate::read::{ReadOutcome, ReadQuery};
    use crate::schema::{AtmMessageId, InboxMessage};
    use crate::send::{SendOutcome, SendRequest};
    use crate::service_runtime::RetainedServiceRuntime;
    use crate::test_support::{TEST_LEAD, TEST_QA, TEST_TEAM};
    use crate::types::{AgentName, ChatId, IsoTimestamp, ModelName, TeamName};
    use atm_storage::AgentType;
    use std::fs;
    use std::net::TcpStream;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tempfile::TempDir;

    #[derive(Debug)]
    struct MockGraftClient;

    #[async_trait::async_trait]
    impl AtmGraftClient for MockGraftClient {
        async fn send_message(&self, _request: SendRequest) -> Result<SendOutcome, AtmError> {
            panic!("send_message should not be called in trait object test")
        }

        async fn read_message(&self, _query: ReadQuery) -> Result<ReadOutcome, AtmError> {
            panic!("read_message should not be called in trait object test")
        }
    }

    #[test]
    fn atm_graft_client_trait_is_object_safe() {
        let client: &dyn AtmGraftClient = &MockGraftClient;
        let _ = client;
    }

    #[test]
    fn graft_hook_budget_reserves_time_for_the_outer_result_handoff() {
        let short = remaining_hook_budget(
            RequestDeadline::after(RECEIVER_HOOK_RESULT_HANDOFF_GRACE),
            Duration::from_secs(1),
        )
        .expect_err("a deadline with no handoff margin must not start socket I/O");
        assert!(short.message().contains("result handoff"));

        let budget = remaining_hook_budget(
            RequestDeadline::after(Duration::from_secs(1)),
            Duration::from_secs(1),
        )
        .expect("larger request deadline retains socket budget");
        assert!(budget < Duration::from_secs(1));
        assert!(budget <= Duration::from_secs(1) - RECEIVER_HOOK_RESULT_HANDOFF_GRACE);
    }

    fn test_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: AgentName::from_validated(TEST_LEAD),
            sender_chat_id: None,
            sender_team: TeamName::from_validated(TEST_TEAM),
            sender_host: None,
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

    fn bind_listener(
        graft_root: &Path,
        team: &TeamName,
        agent: &AgentName,
        owner_chat_id: Option<ChatId>,
    ) -> Result<GraftReceiverListener, AtmError> {
        GraftReceiverListener::bind(graft_root, team, agent, owner_chat_id)
    }

    #[test]
    fn loopback_receiver_round_trips_a_capability_authenticated_request() {
        let tempdir = TempDir::new().expect("tempdir");
        let listener = bind_listener(
            tempdir.path(),
            &TeamName::from_validated(TEST_TEAM),
            &AgentName::from_validated(TEST_QA),
            None,
        )
        .expect("bind listener");
        let request = GraftPostSendRequest {
            event: test_event(),
            kind: NudgeKind::Steer,
            rendered_nudge: "<atm>test nudge</atm>".to_string(),
            message_body: "full immutable body".to_string(),
        };
        let endpoint = listener.local_addr().expect("local addr");
        let capability = listener.capability().clone();
        let sender = std::thread::spawn({
            move || {
                deliver_graft_post_send(
                    endpoint,
                    &capability,
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
        assert_eq!(received.rendered_nudge, "<atm>test nudge</atm>");
        assert_eq!(received.message_body, "full immutable body");
        assert_eq!(received.event.description, "loopback graft transport");
        assert_eq!(received.kind, NudgeKind::Steer);
        listener
            .write_response(&mut stream, &GraftPostSendResponse::Delivered)
            .expect("write response");

        let response = sender.join().expect("join sender").expect("deliver");
        assert_eq!(response, GraftPostSendResponse::Delivered);
    }

    #[test]
    fn pre_aq2_wire_request_defaults_to_steer_kind() {
        let event = test_event();
        let old_wire = serde_json::json!({
            "capability_base64url": "capability",
            "request": {
                "event": event,
                "rendered_nudge": "<atm>legacy</atm>",
                "message_body": "legacy body"
            }
        });
        let decoded: GraftPostSendWireRequest =
            serde_json::from_value(old_wire).expect("old wire shape remains readable");
        assert_eq!(decoded.request.kind, NudgeKind::Steer);
    }

    #[test]
    fn queue_kind_is_preserved_on_the_evolved_wire_request() {
        let request = GraftPostSendRequest {
            event: test_event(),
            kind: NudgeKind::Queue,
            rendered_nudge: "<atm>queued</atm>".to_owned(),
            message_body: "queued body".to_owned(),
        };
        let wire = GraftPostSendWireRequest {
            capability_base64url: "capability".to_owned(),
            request,
        };
        let decoded: GraftPostSendWireRequest =
            serde_json::from_slice(&serde_json::to_vec(&wire).expect("encode wire"))
                .expect("decode wire");
        assert_eq!(decoded.request.kind, NudgeKind::Queue);
    }

    #[test]
    fn receiver_rejects_a_forged_capability() {
        let tempdir = TempDir::new().expect("tempdir");
        let listener = bind_listener(
            tempdir.path(),
            &TeamName::from_validated(TEST_TEAM),
            &AgentName::from_validated(TEST_QA),
            None,
        )
        .expect("bind listener");
        let endpoint = listener.local_addr().expect("local addr");

        let forger = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(endpoint).expect("connect to loopback receiver");
            let wire = super::GraftPostSendWireRequest {
                capability_base64url: "not-the-real-capability".to_string(),
                request: GraftPostSendRequest {
                    event: test_event(),
                    kind: NudgeKind::Steer,
                    rendered_nudge: "<atm>test nudge</atm>".to_string(),
                    message_body: "full immutable body".to_string(),
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
    fn bind_removes_stale_endpoint_artifact_and_keeps_ownership_lock() {
        let tempdir = TempDir::new().expect("tempdir");
        let team = TeamName::from_validated(TEST_TEAM);
        let agent = AgentName::from_validated(TEST_QA);
        let endpoint_path = tempdir
            .path()
            .join(".atm/graft")
            .join(TEST_TEAM)
            .join(format!("{TEST_QA}.json"));
        fs::create_dir_all(endpoint_path.parent().expect("endpoint parent")).expect("parent");
        fs::write(&endpoint_path, b"stale endpoint").expect("stale endpoint");
        let first = bind_listener(tempdir.path(), &team, &agent, None).expect("first");
        assert!(!endpoint_path.exists());
        let error = match bind_listener(tempdir.path(), &team, &agent, None) {
            Ok(_) => panic!("second live owner must fail"),
            Err(error) => error,
        };
        assert_eq!(
            error.code(),
            crate::error::AtmErrorCode::GraftReceiverAlreadyActive
        );
        assert!(error.message().contains(TEST_QA));
        drop(first);
        assert!(graft_receiver_lock_path_from_root(tempdir.path(), &team, &agent).exists());
    }

    #[test]
    fn distinct_receiver_identities_can_listen_concurrently() {
        let tempdir = TempDir::new().expect("tempdir");
        let team = TeamName::from_validated(TEST_TEAM);
        let first = bind_listener(
            tempdir.path(),
            &team,
            &AgentName::from_validated(TEST_QA),
            None,
        )
        .expect("first");
        let second = bind_listener(
            tempdir.path(),
            &team,
            &AgentName::from_validated(TEST_LEAD),
            None,
        )
        .expect("second");
        assert_ne!(
            first.local_addr().expect("first addr"),
            second.local_addr().expect("second addr")
        );
    }

    // ------------------------------------------------------------------
    // AQ1.7 QA-1 fixes, ported forward through AQ1.8's file-record removal:
    // AC1/AC3/AC5/AC6 coverage for deliver_published_receiver_hook against
    // the registry lease alone (there is no legacy file record left to
    // reference at all after this sprint).
    // ------------------------------------------------------------------

    fn roster_entry(team: &TeamName, agent: &AgentName) -> RosterEntry {
        RosterEntry {
            team_name: team.clone(),
            agent_name: agent.clone(),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::CodexCli,
            agent_type: AgentType::default(),
            model: ModelName::default(),
            recipient_pane_id: None,
            metadata_json: serde_json::Map::new(),
        }
    }

    fn dispatch_for(event: PostSendHookEvent) -> BuiltInPostSendDispatch {
        BuiltInPostSendDispatch {
            target: PostSendBuiltInTarget::Graft(GraftNudgeTarget {
                recipient: event.recipient.clone(),
                recipient_team: event.recipient_team.clone(),
                rendered_nudge: "<atm>test nudge</atm>".to_string(),
                message_body: "full immutable body".to_string(),
            }),
            event,
            kind: NudgeKind::Steer,
        }
    }

    /// Shared backing state for [`GraftLeaseTestRuntime`]: multiple
    /// independently constructed runtime instances can attach to the same
    /// `Arc`, modeling a daemon process restart that reattaches to the same
    /// persisted store (AC5's "daemon restart with a live receiver" cell).
    #[derive(Clone, Default)]
    struct SharedGraftLease(Arc<Mutex<Option<atm_storage::GraftReceiverLease>>>);

    impl SharedGraftLease {
        fn set(&self, lease: atm_storage::GraftReceiverLease) {
            *self.0.lock().expect("shared lease") = Some(lease);
        }

        fn get(&self) -> Option<atm_storage::GraftReceiverLease> {
            self.0.lock().expect("shared lease").clone()
        }
    }

    /// Deterministic [`RetainedServiceRuntime`] test double exercising only
    /// the graft-lease seam `deliver_published_receiver_hook` depends on: a
    /// fixed roster member and a caller-controlled lease/mark-unreachable
    /// recorder. All other trait methods use their harmless defaults.
    struct GraftLeaseTestRuntime {
        lease: SharedGraftLease,
        marked_unreachable: Mutex<Vec<(TeamName, AgentName, OwnerGeneration)>>,
    }

    impl GraftLeaseTestRuntime {
        fn with_lease(lease: Option<atm_storage::GraftReceiverLease>) -> Self {
            let shared = SharedGraftLease::default();
            if let Some(lease) = lease {
                shared.set(lease);
            }
            Self::from_shared(shared)
        }

        /// Attaches a fresh runtime instance to an existing shared lease
        /// cell, modeling a newly constructed daemon-runtime object that
        /// reattaches to already-persisted state after a restart.
        fn from_shared(lease: SharedGraftLease) -> Self {
            Self {
                lease,
                marked_unreachable: Mutex::new(Vec::new()),
            }
        }
    }

    impl crate::boundary::sealed::Sealed for GraftLeaseTestRuntime {}

    impl RetainedServiceRuntime for GraftLeaseTestRuntime {
        fn load_config(&self, _current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
            Ok(None)
        }

        fn graft_receiver_lease(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<atm_storage::GraftReceiverLease>, AtmError> {
            Ok(self.lease.get())
        }

        fn mark_graft_receiver_unreachable(
            &self,
            team: &TeamName,
            agent: &AgentName,
            owner_generation: &OwnerGeneration,
            _now: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), AtmError> {
            self.marked_unreachable
                .lock()
                .expect("marked_unreachable")
                .push((team.clone(), agent.clone(), owner_generation.clone()));
            Ok(())
        }

        fn inbox_path(
            &self,
            home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<std::path::PathBuf, AtmError> {
            Ok(home_dir.join("inbox.jsonl"))
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<IsoTimestamp>, AtmError> {
            Ok(None)
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: IsoTimestamp,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn deliver_non_claude_payloads(
            &self,
            _recipient: &DeliveryRecipientSnapshot,
            _messages: &[InboxMessage],
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn load_roster_member(
            &self,
            team: &TeamName,
            agent: &AgentName,
        ) -> Result<Option<RosterEntry>, AtmError> {
            Ok(Some(roster_entry(team, agent)))
        }

        fn load_team_roster(&self, team: &TeamName) -> Result<Vec<RosterEntry>, AtmError> {
            Ok(vec![roster_entry(
                team,
                &AgentName::from_validated(TEST_QA),
            )])
        }
    }

    fn lease_for(listener: &GraftReceiverListener) -> atm_storage::GraftReceiverLease {
        atm_storage::GraftReceiverLease {
            endpoint: listener.local_addr().expect("endpoint"),
            capability: listener.capability().clone(),
            owner_generation: OwnerGeneration::new(listener.owner_generation().to_string())
                .expect("owner generation"),
            registered_at: chrono::Utc::now(),
            last_seen_at: chrono::Utc::now(),
            unreachable_since: None,
        }
    }

    /// Runs one `deliver_published_receiver_hook` call concurrently with a
    /// background thread that serves exactly one accepted connection with
    /// `Delivered`, then joins that thread before returning. Borrows
    /// `listener` rather than consuming it, so the same live listener can
    /// serve multiple sequential deliveries (AC5's "receiver stays alive
    /// across a daemon restart" matrix cell).
    fn deliver_through_listener(
        runtime: &GraftLeaseTestRuntime,
        listener: &GraftReceiverListener,
    ) -> Result<PostSendEmissionPath, AtmError> {
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let mut stream = loop {
                    if let Some(stream) = listener.poll_accept().expect("poll accept") {
                        break stream;
                    }
                    std::thread::yield_now();
                };
                listener
                    .read_request(&mut stream, Duration::from_secs(3))
                    .expect("read request");
                listener
                    .write_response(&mut stream, &GraftPostSendResponse::Delivered)
                    .expect("write response");
            });
            deliver_published_receiver_hook(
                runtime,
                &dispatch_for(test_event()),
                RequestDeadline::after(Duration::from_secs(3)),
            )
        })
    }

    // AC1 (PR #1048): delivery through a registered lease succeeds via the
    // registry alone. AQ1.8 retires the legacy file record entirely, so
    // there is no file left to reference — the registry is the only path.
    #[test]
    fn ac1_delivery_succeeds_through_the_registry_lease_alone() {
        let tempdir = TempDir::new().expect("tempdir");
        let team = TeamName::from_validated(TEST_TEAM);
        let agent = AgentName::from_validated(TEST_QA);
        let listener = bind_listener(tempdir.path(), &team, &agent, None).expect("bind listener");

        let runtime = GraftLeaseTestRuntime::with_lease(Some(lease_for(&listener)));
        let outcome = deliver_through_listener(&runtime, &listener)
            .expect("delivery must succeed via the registry lease alone");
        assert_eq!(outcome, PostSendEmissionPath::GraftPort);
    }

    // AC3 (PR #1048): a dead endpoint records `mark_graft_receiver_unreachable`
    // and surfaces today's delivery error unchanged (no new error shape).
    #[test]
    fn ac3_connect_failure_marks_the_lease_unreachable_and_surfaces_todays_delivery_error() {
        let team = TeamName::from_validated(TEST_TEAM);
        let agent = AgentName::from_validated(TEST_QA);
        // A bound-then-dropped loopback listener yields a port nothing is
        // listening on, deterministically producing a connect failure
        // without depending on any particular unused-port convention.
        let dead_port = {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind probe");
            listener.local_addr().expect("probe addr")
        };
        let owner_generation =
            OwnerGeneration::new("01J00000000000000000000030").expect("owner generation");
        let lease = atm_storage::GraftReceiverLease {
            endpoint: dead_port,
            capability: crate::local_http::LocalCapability::generate().expect("capability"),
            owner_generation: owner_generation.clone(),
            registered_at: chrono::Utc::now(),
            last_seen_at: chrono::Utc::now(),
            unreachable_since: None,
        };
        let runtime = GraftLeaseTestRuntime::with_lease(Some(lease));

        let error = deliver_published_receiver_hook(
            &runtime,
            &dispatch_for(test_event()),
            RequestDeadline::after(Duration::from_secs(3)),
        )
        .expect_err("a dead endpoint must surface today's connect-failure error");
        assert_eq!(
            error.code(),
            crate::error::AtmErrorCode::PostSendGraftUnavailable
        );

        let marked = runtime
            .marked_unreachable
            .lock()
            .expect("marked_unreachable");
        assert_eq!(
            marked.as_slice(),
            [(team, agent, owner_generation)],
            "the connect failure must record exactly one mark_graft_receiver_unreachable call"
        );
    }

    // AC6 (PR #1048, closes I11): a lease that exists but whose `last_seen_at`
    // is far beyond `ACTIVE_LEASE_WINDOW` is still dialed — staleness never
    // gates delivery, only an absent lease does.
    #[test]
    fn ac6_present_but_expired_lease_is_dialed_not_refused() {
        let tempdir = TempDir::new().expect("tempdir");
        let team = TeamName::from_validated(TEST_TEAM);
        let agent = AgentName::from_validated(TEST_QA);
        let listener = bind_listener(tempdir.path(), &team, &agent, None).expect("bind listener");

        // Far beyond ACTIVE_LEASE_WINDOW (15s); the receiver is actually
        // alive (simulated clock skew), so delivery must still succeed.
        let stale_lease = atm_storage::GraftReceiverLease {
            registered_at: chrono::Utc::now() - chrono::Duration::hours(2),
            last_seen_at: chrono::Utc::now() - chrono::Duration::hours(2),
            unreachable_since: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
            ..lease_for(&listener)
        };
        let runtime = GraftLeaseTestRuntime::with_lease(Some(stale_lease));

        let outcome = deliver_through_listener(&runtime, &listener)
            .expect("a present-but-expired lease must still be dialed, not refused");
        assert_eq!(outcome, PostSendEmissionPath::GraftPort);
        assert!(
            runtime
                .marked_unreachable
                .lock()
                .expect("marked_unreachable")
                .is_empty(),
            "a successful dial must not record an unreachable observation"
        );
    }

    // AC5 (PR #1048), matrix cell "daemon restart with a live receiver": the
    // receiver's lease is served correctly by a freshly constructed runtime
    // instance that reattaches to the same persisted state, with zero
    // receiver-side action — this is the automated in-tree half of the
    // restart matrix (AQ1.9 owns only the live multi-process evidence).
    #[test]
    fn ac5_daemon_restart_with_live_receiver_delivers_through_a_reattached_runtime() {
        let tempdir = TempDir::new().expect("tempdir");
        let team = TeamName::from_validated(TEST_TEAM);
        let agent = AgentName::from_validated(TEST_QA);
        // One listener stays bound for the whole test: only the daemon-side
        // runtime object "restarts" below, never the receiver.
        let listener = bind_listener(tempdir.path(), &team, &agent, None).expect("bind listener");

        let shared = SharedGraftLease::default();
        shared.set(lease_for(&listener));
        let before_restart = GraftLeaseTestRuntime::from_shared(shared.clone());
        deliver_through_listener(&before_restart, &listener)
            .expect("delivery must succeed before the simulated daemon restart");

        // Simulate a daemon restart: a brand-new runtime object, sharing only
        // the same persisted lease state and the still-live receiver, must
        // still resolve delivery with no receiver-side action at all.
        let after_restart = GraftLeaseTestRuntime::from_shared(shared);
        let outcome = deliver_through_listener(&after_restart, &listener)
            .expect("delivery must succeed through the reattached post-restart runtime");
        assert_eq!(outcome, PostSendEmissionPath::GraftPort);
    }

    // AC5 (PR #1048), matrix cells "receiver restart with a live daemon" and
    // "SIGKILL crash-within-window": the daemon-side runtime object never
    // restarts here (same `GraftLeaseTestRuntime` throughout); only the
    // receiver does. Dropping the first listener without any unregister call
    // mirrors what a SIGKILL leaves behind (the OS releases the flock; no
    // graceful cleanup ever runs) — the successor's registration must
    // displace it immediately, not after any window elapses, and delivery
    // must reach only the successor.
    #[test]
    fn ac5_receiver_restart_with_a_live_daemon_displaces_immediately_after_a_sigkill_style_exit() {
        let tempdir = TempDir::new().expect("tempdir");
        let team = TeamName::from_validated(TEST_TEAM);
        let agent = AgentName::from_validated(TEST_QA);
        let runtime = GraftLeaseTestRuntime::with_lease(None);

        let first = bind_listener(tempdir.path(), &team, &agent, None).expect("bind first");
        let first_lease = lease_for(&first);
        runtime.lease.set(first_lease.clone());
        // No unregister call follows: this models a SIGKILL, which leaves
        // the store's row exactly as `register` last wrote it.
        drop(first);

        let second = bind_listener(tempdir.path(), &team, &agent, None).expect(
            "the successor must be able to bind immediately: the OS released the flock \
             when the first process's file handle disappeared, exactly as it would after SIGKILL",
        );
        let second_lease = lease_for(&second);
        assert_ne!(
            second_lease.owner_generation, first_lease.owner_generation,
            "the successor must mint its own generation, never reuse the crashed one"
        );
        // The successor's registration displaces the stale entry on its very
        // first tick — no ACTIVE_LEASE_WINDOW wait, matching the daemon
        // registry's real `register` semantics (ADR-056).
        runtime.lease.set(second_lease);

        let outcome = deliver_through_listener(&runtime, &second)
            .expect("delivery must reach the successor immediately after displacement");
        assert_eq!(outcome, PostSendEmissionPath::GraftPort);
    }
}
