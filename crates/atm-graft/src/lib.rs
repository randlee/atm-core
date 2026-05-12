//! Thin embedded ATM client crate for graft-aware host agents.
//!
//! Phase U.8 intentionally keeps this crate lean:
//! - shared unary send/read/ack transport only
//! - no daemon-private API surface
//! - no client runtime loop yet
//!
//! U.9 owns the client-side session runtime and advisory receive loop.

use std::fmt;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use atm_core::ack::{AckOutcome, AckRequest};
use atm_core::boundary;
use atm_core::boundary::ClientTransport;
use atm_core::error::AtmError;
use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::send::{SendOutcome, SendRequest};
use atm_daemon_client::{DaemonBinaryPath, DaemonLocalIpcEndpoint, DaemonSupervisor};
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;

const SAME_HOST_REQUEST_DEADLINE: Duration = Duration::from_secs(3);

pub use atm_core::types::ClientSessionId;

/// Thin daemon-backed same-host client for embedded graft consumers.
#[derive(Clone)]
pub struct GraftClient {
    transport: Arc<dyn ClientTransport + Send + Sync>,
}

impl fmt::Debug for GraftClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GraftClient")
            .field("transport", &"dyn ClientTransport")
            .finish()
    }
}

impl GraftClient {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the daemon endpoint or daemon binary cannot
    /// be resolved or the same-host daemon cannot be reached or started.
    pub fn connect() -> Result<Self, AtmError> {
        let endpoint = resolve_daemon_local_ipc_endpoint()?;
        let daemon_bin = resolve_daemon_bin()?;
        let transport = Arc::new(GraftLocalIpcClientTransport::new(endpoint.clone()));
        let supervisor = DaemonSupervisor::new(endpoint, daemon_bin);
        supervisor.ensure_daemon_available(|| transport.try_connect().map(|_| ()))?;
        Ok(Self::from_transport(transport))
    }

    pub fn from_transport(transport: Arc<dyn ClientTransport + Send + Sync>) -> Self {
        Self { transport }
    }

    pub fn send_message(&self, request: SendRequest) -> Result<SendOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))? {
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => Ok(outcome),
            other => Err(unexpected_response("send", other)),
        }
    }

    pub fn read_message(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Receive(query))? {
            ResponseEnvelope::Receive(outcome) => Ok(outcome),
            other => Err(unexpected_response("read", other)),
        }
    }

    pub fn acknowledge_message(&self, request: AckRequest) -> Result<AckOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(
            request,
        )))? {
            ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => Ok(outcome),
            other => Err(unexpected_response("ack", other)),
        }
    }

    fn send_request(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        match self.transport.send(request)? {
            ResponseEnvelope::Error(error) => Err(error.into_atm_error()),
            response => Ok(response),
        }
    }
}

#[derive(Debug)]
struct GraftLocalIpcClientTransport {
    endpoint: DaemonLocalIpcEndpoint,
}

impl GraftLocalIpcClientTransport {
    fn new(endpoint: DaemonLocalIpcEndpoint) -> Self {
        Self { endpoint }
    }

    fn try_connect(&self) -> Result<LocalSocketStream, AtmError> {
        LocalSocketStream::connect(atm_core::protocol::daemon_local_ipc_name_from_path(
            self.endpoint.as_ref(),
        )?)
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to connect to daemon local IPC endpoint at {}",
                self.endpoint.display()
            ))
            .with_source(source)
        })
    }

    fn exchange(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let mut stream = self.try_connect()?;
        stream
            .set_send_timeout(Some(SAME_HOST_REQUEST_DEADLINE))
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to configure daemon local IPC write timeout")
                    .with_source(source)
            })?;
        stream
            .set_recv_timeout(Some(SAME_HOST_REQUEST_DEADLINE))
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to configure daemon local IPC read timeout")
                    .with_source(source)
            })?;
        let request_id = atm_core::protocol::next_request_id();
        let frame = atm_core::protocol::request_to_frame_payload(request_id, request)?;
        atm_core::protocol::write_frame(
            &mut stream,
            &frame,
            "failed to write daemon request frame",
        )?;
        stream.flush().map_err(|source| {
            AtmError::daemon_unavailable("failed to flush daemon request frame").with_source(source)
        })?;
        let response_frame = atm_core::protocol::read_frame(
            &mut stream,
            "failed to read daemon response frame",
            "daemon response frame exceeded the maximum supported size",
        )?
        .ok_or_else(|| {
            AtmError::daemon_unavailable(
                "daemon closed the local IPC connection before returning a response frame",
            )
            .with_recovery(
                "Retry the ATM command after the daemon reaches serving state and verify the daemon logs if the problem persists.",
            )
        })?;
        let (response_id, response) =
            atm_core::protocol::response_from_frame_payload(response_frame)?;
        if response_id != request_id {
            return Err(AtmError::daemon_unavailable(format!(
                "daemon response request_id {} did not match request_id {}",
                response_id, request_id
            ))
            .with_recovery(
                "Align the client and daemon builds so both sides use the same ATM daemon protocol contract before retrying.",
            ));
        }
        Ok(response)
    }
}

impl boundary::sealed::Sealed for GraftLocalIpcClientTransport {}

impl ClientTransport for GraftLocalIpcClientTransport {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        self.exchange(request)
    }
}

fn resolve_daemon_local_ipc_endpoint() -> Result<DaemonLocalIpcEndpoint, AtmError> {
    if let Some(value) = std::env::var_os("ATM_DAEMON_SOCKET") {
        return DaemonLocalIpcEndpoint::new(value.into());
    }
    DaemonLocalIpcEndpoint::new(atm_core::protocol::daemon_socket_path()?)
}

fn resolve_daemon_bin() -> Result<DaemonBinaryPath, AtmError> {
    if let Some(value) = std::env::var_os("ATM_DAEMON_BIN") {
        return DaemonBinaryPath::new(value.into());
    }
    let current = std::env::current_exe().map_err(|source| {
        AtmError::daemon_unavailable("failed to resolve current executable for daemon lookup")
            .with_source(source)
    })?;
    DaemonBinaryPath::new(
        current.with_file_name(format!("atm-daemon{}", std::env::consts::EXE_SUFFIX)),
    )
}

fn unexpected_response(action: &'static str, response: ResponseEnvelope) -> AtmError {
    AtmError::daemon_unavailable(format!(
        "daemon returned unexpected response for {action}: {response:?}"
    ))
    .with_recovery(
        "Align the thin client and daemon builds so both sides use the same shared ATM request and response contract before retrying.",
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use atm_core::ack::AckOutcome;
    use atm_core::read::{BucketCounts, ReadOutcome, ReadQuery};
    use atm_core::schema::AtmMessageId;
    use atm_core::send::{SendMessageSource, SendOutcome, SendRequest, WarningEntry};
    use atm_core::test_support::{
        TEST_LEAD, TEST_LEAD_ADDRESS, TEST_SENDER, TEST_SENDER_ADDRESS, TEST_TEAM,
    };
    use atm_core::transport::testing::FakeClientTransport;
    use atm_core::types::{AckActivationMode, CommandAction, ReadSelection};
    use serde_json::json;

    use super::*;

    #[test]
    fn send_uses_shared_send_envelope() {
        let transport = Arc::new(FakeClientTransport::new(|request| match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(send)) => {
                match send.message_source {
                    SendMessageSource::Inline(ref message) => assert_eq!(message, "hello"),
                    ref other => panic!("unexpected message source: {other:?}"),
                }
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(
                    SendOutcome {
                        action: CommandAction::Send,
                        team: TEST_TEAM.parse().expect("team"),
                        agent: TEST_SENDER.parse().expect("agent"),
                        sender: TEST_SENDER.parse().expect("sender"),
                        outcome: "sent".to_string(),
                        message_id: AtmMessageId::new(),
                        requires_ack: false,
                        task_id: None,
                        summary: Some("summary".to_string()),
                        message: Some("hello".to_string()),
                        warnings: vec![WarningEntry::new("warning", Some("recovery"))],
                        dry_run: false,
                    },
                )))
            }
            other => panic!("unexpected request: {other:?}"),
        }));

        let client = GraftClient::from_transport(transport);
        let outcome = client
            .send_message(
                SendRequest::new(
                    PathBuf::from("/tmp/home"),
                    PathBuf::from("/tmp/current"),
                    Some(TEST_SENDER),
                    TEST_LEAD_ADDRESS,
                    None,
                    SendMessageSource::Inline("hello".to_string()),
                    None,
                    false,
                    None,
                    false,
                )
                .expect("request"),
            )
            .expect("send");

        assert_eq!(outcome.team.as_ref(), TEST_TEAM);
        assert_eq!(outcome.agent.as_ref(), TEST_SENDER);
    }

    #[test]
    fn read_uses_shared_receive_envelope() {
        let transport = Arc::new(FakeClientTransport::new(|request| match request {
            RequestEnvelope::Receive(query) => {
                assert_eq!(
                    query.team_override.as_ref().map(|team| team.as_ref()),
                    Some(TEST_TEAM)
                );
                Ok(ResponseEnvelope::Receive(ReadOutcome {
                    action: CommandAction::Read,
                    team: TEST_TEAM.parse().expect("team"),
                    agent: TEST_LEAD.parse().expect("agent"),
                    selection_mode: ReadSelection::All,
                    mutation_applied: false,
                    count: 0,
                    message: None,
                    selected_message_id: None,
                    match_count: 0,
                    additional_match_count: 0,
                    bucket_counts: BucketCounts {
                        unread: 0,
                        pending_ack: 0,
                        history: 0,
                    },
                }))
            }
            other => panic!("unexpected request: {other:?}"),
        }));

        let client = GraftClient::from_transport(transport);
        let outcome = client
            .read_message(
                ReadQuery::new(
                    PathBuf::from("/tmp/home"),
                    PathBuf::from("/tmp/current"),
                    Some(TEST_SENDER),
                    Some(TEST_LEAD_ADDRESS),
                    Some(TEST_TEAM),
                    ReadSelection::All,
                    false,
                    false,
                    AckActivationMode::ReadOnly,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("query"),
            )
            .expect("read");

        assert_eq!(outcome.team.as_ref(), TEST_TEAM);
        assert_eq!(outcome.agent.as_ref(), TEST_LEAD);
    }

    #[test]
    fn ack_uses_shared_ack_envelope() {
        let transport = Arc::new(FakeClientTransport::new(|request| match request {
            RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(ack)) => {
                assert_eq!(
                    ack.team_override.as_ref().map(|team| team.as_ref()),
                    Some(TEST_TEAM)
                );
                let outcome: AckOutcome = serde_json::from_value(json!({
                    "action": "ack",
                    "team": TEST_TEAM,
                    "agent": TEST_LEAD,
                    "message_id": AtmMessageId::new(),
                    "task_id": null,
                    "reply_target": TEST_SENDER_ADDRESS,
                    "reply_message_id": AtmMessageId::new(),
                    "reply_text": "ack",
                    "warnings": []
                }))
                .expect("ack outcome");
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(
                    outcome,
                )))
            }
            other => panic!("unexpected request: {other:?}"),
        }));

        let client = GraftClient::from_transport(transport);
        let outcome = client
            .acknowledge_message(AckRequest {
                home_dir: PathBuf::from("/tmp/home"),
                current_dir: PathBuf::from("/tmp/current"),
                actor_override: Some(TEST_SENDER.parse().expect("actor")),
                team_override: Some(TEST_TEAM.parse().expect("team")),
                message_id: AtmMessageId::new(),
                reply_body: "ack".to_string(),
            })
            .expect("ack");

        assert_eq!(outcome.team.as_ref(), TEST_TEAM);
        assert_eq!(outcome.reply_text, "ack");
    }

    #[test]
    fn protocol_round_trip_uses_shared_request_family() {
        let request = RequestEnvelope::Receive(
            ReadQuery::new(
                PathBuf::from("/tmp/home"),
                PathBuf::from("/tmp/current"),
                Some(TEST_SENDER),
                Some(TEST_LEAD_ADDRESS),
                Some(TEST_TEAM),
                ReadSelection::All,
                false,
                false,
                AckActivationMode::ReadOnly,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .expect("query"),
        );
        let request_id = atm_core::protocol::next_request_id();

        let frame = atm_core::protocol::request_to_frame_payload(request_id, request.clone())
            .expect("frame");
        let (decoded_id, decoded_request) =
            atm_core::protocol::request_from_frame_payload(frame).expect("decode");

        assert_eq!(decoded_id, request_id);
        let encoded = serde_json::to_string(&decoded_request).expect("json");
        assert!(encoded.contains("\"Receive\""));
    }
}
