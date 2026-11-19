//! Direct Herdr NDJSON transport over the server's local socket.

#![allow(dead_code)]

use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use atm_core::{EnvSource, HerdrSession, ProcessEnvSource, RequestDeadline};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Semaphore;
use tokio::time::Instant;

use crate::HerdrError;
use crate::transport::{HerdrClientConfig, HerdrEnvelope, HerdrErrorEnvelope, HerdrOp};

const SOCKET_PERMITS: usize = 16;
const PIPE_BUSY_RETRY_DELAY: Duration = Duration::from_millis(10);
const HERDR_MAX_SOCKET_LINE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Platform {
    Unix,
    Windows,
}

#[derive(Clone, Debug)]
pub(crate) struct HerdrHostEnv {
    pub(crate) xdg_config_home: Option<PathBuf>,
    pub(crate) appdata: Option<PathBuf>,
    pub(crate) home: Option<PathBuf>,
    pub(crate) platform: Platform,
}

impl HerdrHostEnv {
    pub(crate) fn capture() -> Self {
        let process_env = ProcessEnvSource;
        let home = atm_core::home::resolve_user_home_via(&process_env);
        let xdg_config_home = process_env
            .var("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let appdata = process_env
            .var("APPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let (xdg_config_home, appdata) = if cfg!(windows) {
            (None, appdata)
        } else {
            (xdg_config_home, None)
        };
        Self {
            xdg_config_home,
            appdata,
            home,
            platform: if cfg!(windows) {
                Platform::Windows
            } else {
                Platform::Unix
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HerdrEndpoint {
    UnixSocket(PathBuf),
    NamedPipe(String),
}

#[derive(Clone, Debug)]
pub(crate) struct SocketIo {
    pub(crate) cfg: HerdrClientConfig,
    pub(crate) env: HerdrHostEnv,
    pub(crate) max_line_bytes: usize,
    pub(crate) in_flight: Arc<Semaphore>,
    pub(crate) pipe_busy_retry_delay: Duration,
}

impl SocketIo {
    pub(crate) fn new(cfg: &HerdrClientConfig) -> Self {
        Self {
            cfg: cfg.clone(),
            env: HerdrHostEnv::capture(),
            max_line_bytes: HERDR_MAX_SOCKET_LINE_BYTES,
            in_flight: Arc::new(Semaphore::new(SOCKET_PERMITS)),
            pipe_busy_retry_delay: PIPE_BUSY_RETRY_DELAY,
        }
    }

    pub(crate) async fn call(
        &self,
        op: HerdrOp<'_>,
        session: Option<&HerdrSession>,
        deadline: RequestDeadline,
    ) -> Result<HerdrEnvelope, HerdrError> {
        self.round_trip(encode_request(op)?, session, deadline)
            .await
    }

    /// Doctor-only server information probe. It deliberately uses a fresh
    /// one-request connection rather than adding a ping to a nudge call.
    pub(crate) async fn server_info(
        &self,
        session: Option<&HerdrSession>,
        deadline: RequestDeadline,
    ) -> Result<HerdrEnvelope, HerdrError> {
        self.round_trip(encode_ping_request(), session, deadline)
            .await
    }

    async fn round_trip(
        &self,
        request: Vec<u8>,
        session: Option<&HerdrSession>,
        deadline: RequestDeadline,
    ) -> Result<HerdrEnvelope, HerdrError> {
        let deadline = tokio_deadline(deadline)?;
        #[cfg(unix)]
        let connect = |endpoint, deadline| async move { connect_unix(&endpoint, deadline).await };
        #[cfg(windows)]
        let connect = {
            let retry_delay = self.pipe_busy_retry_delay;
            move |endpoint, deadline| async move {
                connect_named_pipe_at(&endpoint, deadline, retry_delay).await
            }
        };
        self.round_trip_with_deadline(request, session, deadline, connect)
            .await
    }

    /// Internal transport-entry seam. The injected absolute deadline covers
    /// permit acquisition, connection setup, and the complete stream exchange.
    async fn round_trip_with_deadline<C, F, S>(
        &self,
        request: Vec<u8>,
        session: Option<&HerdrSession>,
        deadline: Instant,
        connect: C,
    ) -> Result<HerdrEnvelope, HerdrError>
    where
        C: FnOnce(HerdrEndpoint, Instant) -> F,
        F: Future<Output = Result<S, HerdrError>>,
        S: AsyncRead + AsyncWrite + Unpin,
    {
        if Instant::now() >= deadline {
            return Err(HerdrError::Timeout);
        }
        let permit = acquire_permit(Arc::clone(&self.in_flight), deadline).await?;
        let endpoint = herdr_api_endpoint(&self.cfg, session, &self.env);
        let mut stream = tokio::time::timeout_at(deadline, connect(endpoint, deadline))
            .await
            .map_err(|_| HerdrError::Timeout)??;

        let response =
            exchange_over_stream(&mut stream, &request, self.max_line_bytes, deadline).await?;
        drop(stream);
        drop(permit);
        Ok(response)
    }
}

fn tokio_deadline(deadline: RequestDeadline) -> Result<Instant, HerdrError> {
    let Some(remaining) = deadline.remaining() else {
        return Err(HerdrError::Timeout);
    };
    Ok(Instant::now() + remaining)
}

/// Pure endpoint selection. Environment is supplied by the composition root
/// and is never sampled while a request is in flight.
pub(crate) fn herdr_api_endpoint(
    cfg: &HerdrClientConfig,
    session: Option<&HerdrSession>,
    env: &HerdrHostEnv,
) -> HerdrEndpoint {
    match env.platform {
        Platform::Unix => {
            let raw = cfg
                .socket_path()
                .map(std::path::Path::to_path_buf)
                .or_else(|| {
                    HerdrSession::named(session).map(|session| {
                        config_dir(env)
                            .join("sessions")
                            .join(session.as_str())
                            .join("herdr.sock")
                    })
                });
            HerdrEndpoint::UnixSocket(raw.unwrap_or_else(|| config_dir(env).join("herdr.sock")))
        }
        Platform::Windows => {
            let raw = cfg
                .socket_path()
                .map(std::path::Path::to_path_buf)
                .or_else(|| {
                    HerdrSession::named(session).map(|session| {
                        PathBuf::from(format!(
                            r"{}\sessions\{}\herdr.sock",
                            config_dir(env).display(),
                            session.as_str()
                        ))
                    })
                });
            let raw = raw.unwrap_or_else(|| {
                PathBuf::from(format!(r"{}\herdr.sock", config_dir(env).display()))
            });
            HerdrEndpoint::NamedPipe(named_pipe_name(raw))
        }
    }
}

fn config_dir(env: &HerdrHostEnv) -> PathBuf {
    match env.platform {
        Platform::Unix => env
            .xdg_config_home
            .clone()
            .or_else(|| env.home.as_ref().map(|home| home.join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("herdr"),
        Platform::Windows => {
            let base = env
                .appdata
                .clone()
                .or_else(|| env.home.clone())
                .unwrap_or_else(|| PathBuf::from("."));
            PathBuf::from(format!(r"{}\herdr", base.display()))
        }
    }
}

fn named_pipe_name(path: PathBuf) -> String {
    let path = path.to_string_lossy();
    if path.starts_with(r"\\.\pipe\") {
        path.into_owned()
    } else {
        format!(r"\\.\pipe\{path}")
    }
}

async fn acquire_permit(
    in_flight: Arc<Semaphore>,
    deadline: Instant,
) -> Result<tokio::sync::OwnedSemaphorePermit, HerdrError> {
    match tokio::time::timeout_at(deadline, in_flight.acquire_owned()).await {
        Ok(Ok(permit)) => Ok(permit),
        Ok(Err(_)) => Err(HerdrError::InternalError {
            message: "Herdr socket permit pool was closed".to_owned(),
        }),
        Err(_) => Err(HerdrError::Timeout),
    }
}

fn encode_request(op: HerdrOp<'_>) -> Result<Vec<u8>, HerdrError> {
    let value = match op {
        HerdrOp::Prompt { agent, text } => json!({
            "id": "atm:agent:prompt",
            "method": "agent.prompt",
            "params": {"target": agent.to_string(), "text": text},
        }),
        HerdrOp::Wait {
            agent,
            until,
            timeout,
        } => json!({
            "id": "atm:agent:wait",
            "method": "agent.wait",
            "params": {
                "target": agent.to_string(),
                "until": until.iter().map(|status| status.as_str()).collect::<Vec<_>>(),
                "timeout_ms": timeout.as_millis(),
            },
        }),
        HerdrOp::Get { agent } => json!({
            "id": "atm:agent:get",
            "method": "agent.get",
            "params": {"target": agent.to_string()},
        }),
        HerdrOp::List => json!({
            "id": "atm:agent:list",
            "method": "agent.list",
            "params": {},
        }),
        HerdrOp::StatusServer => unreachable!("socket server status uses the ping probe"),
    };
    let mut bytes = serde_json::to_vec(&value).map_err(|error| HerdrError::InternalError {
        message: format!("failed to encode Herdr socket request: {error}"),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn encode_ping_request() -> Vec<u8> {
    let mut bytes = br#"{"id":"atm:agent:ping","method":"ping","params":{}}"#.to_vec();
    bytes.push(b'\n');
    bytes
}

async fn write_request<S>(
    stream: &mut S,
    request: &[u8],
    deadline: Instant,
) -> Result<(), HerdrError>
where
    S: AsyncWrite + Unpin,
{
    deadline_io(deadline, stream.write_all(request)).await?;
    deadline_io(deadline, stream.flush()).await
}

/// Exchange one NDJSON request and response on an already connected stream.
/// The absolute Tokio deadline is injected here so the same transport path is
/// testable under a paused clock without consulting the std clock.
async fn exchange_over_stream<S>(
    stream: &mut S,
    request: &[u8],
    max_line_bytes: usize,
    deadline: Instant,
) -> Result<HerdrEnvelope, HerdrError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_request(stream, request, deadline).await?;
    let response = read_response_line(stream, max_line_bytes, deadline).await?;
    decode_envelope(&response)
}

async fn read_response_line<S>(
    stream: &mut S,
    max_line_bytes: usize,
    deadline: Instant,
) -> Result<Vec<u8>, HerdrError>
where
    S: AsyncRead + Unpin,
{
    let mut line = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 8192];
    loop {
        let read_limit = (max_line_bytes + 1 - line.len()).min(chunk.len());
        let read = deadline_io(deadline, stream.read(&mut chunk[..read_limit])).await?;
        if read == 0 {
            return Err(incomplete_response(line.len()));
        }
        let received = &chunk[..read];
        if let Some(newline) = received.iter().position(|byte| *byte == b'\n') {
            let observed = line.len() + newline;
            if observed > max_line_bytes {
                return Err(oversized_response(observed, max_line_bytes));
            }
            line.extend_from_slice(&received[..newline]);
            return Ok(line);
        }
        line.extend_from_slice(received);
        if line.len() > max_line_bytes {
            return Err(oversized_response(line.len(), max_line_bytes));
        }
    }
}

async fn deadline_io<T>(
    deadline: Instant,
    future: impl Future<Output = io::Result<T>>,
) -> Result<T, HerdrError> {
    match tokio::time::timeout_at(deadline, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(socket_io_error(error)),
        Err(_) => Err(HerdrError::Timeout),
    }
}

fn socket_io_error(error: io::Error) -> HerdrError {
    HerdrError::ServerUnavailable {
        message: format!("Herdr socket transport failed: {error}"),
        retry_after: None,
        io_error_kind: Some(error.kind()),
    }
}

fn incomplete_response(observed_bytes: usize) -> HerdrError {
    eprintln!("Herdr socket response ended before LF after {observed_bytes} bytes");
    HerdrError::InternalError {
        message: format!("Herdr socket response ended before LF after {observed_bytes} bytes"),
    }
}

fn oversized_response(observed_bytes: usize, max_line_bytes: usize) -> HerdrError {
    eprintln!(
        "Herdr socket response exceeded the {max_line_bytes}-byte limit at {observed_bytes} bytes"
    );
    HerdrError::InternalError {
        message: format!(
            "Herdr socket response exceeded the {max_line_bytes}-byte limit at {observed_bytes} bytes"
        ),
    }
}

#[sc_lint_attributes::sc_lint(allow(PORT-010), reason = "Unix connects UnixStream; Windows connect_named_pipe opens a named pipe (crates/atm-herdr/src/transport_socket.rs).")]
#[cfg(unix)]
async fn connect_unix(
    endpoint: &HerdrEndpoint,
    deadline: Instant,
) -> Result<tokio::net::UnixStream, HerdrError> {
    let HerdrEndpoint::UnixSocket(path) = endpoint else {
        return Err(HerdrError::InternalError {
            message: "Windows Herdr endpoint selected on a Unix build".to_owned(),
        });
    };
    connect_with_deadline(deadline, tokio::net::UnixStream::connect(path)).await
}

async fn connect_with_deadline<T>(
    deadline: Instant,
    future: impl Future<Output = io::Result<T>>,
) -> Result<T, HerdrError> {
    deadline_io(deadline, future).await
}

#[cfg(windows)]
async fn connect_named_pipe_at(
    endpoint: &HerdrEndpoint,
    deadline: Instant,
    retry_delay: Duration,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, HerdrError> {
    let HerdrEndpoint::NamedPipe(name) = endpoint else {
        return Err(HerdrError::InternalError {
            message: "Unix Herdr endpoint selected on a Windows build".to_owned(),
        });
    };
    retry_pipe_busy(deadline, retry_delay, || {
        tokio::net::windows::named_pipe::ClientOptions::new().open(name)
    })
    .await
}

#[cfg(all(windows, test))]
async fn connect_named_pipe(
    endpoint: &HerdrEndpoint,
    deadline: RequestDeadline,
    retry_delay: Duration,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, HerdrError> {
    connect_named_pipe_at(endpoint, tokio_deadline(deadline)?, retry_delay).await
}

#[cfg(windows)]
#[sc_lint_attributes::sc_lint(allow(SCB-RUNTIME-005), reason = "ADR-070: Windows ERROR_PIPE_BUSY retry backoff; cancelled by timeout_at(request_deadline).")]
async fn retry_pipe_busy<T>(
    deadline: Instant,
    retry_delay: Duration,
    mut open: impl FnMut() -> io::Result<T>,
) -> Result<T, HerdrError> {
    const ERROR_PIPE_BUSY: i32 = 231;
    match tokio::time::timeout_at(deadline, async {
        loop {
            match open() {
                Ok(client) => return Ok(client),
                Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                    tokio::time::sleep(retry_delay).await;
                }
                Err(error) => return Err(socket_io_error(error)),
            }
        }
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(HerdrError::Timeout),
    }
}

pub(super) fn decode_envelope(bytes: &[u8]) -> Result<HerdrEnvelope, HerdrError> {
    let value = serde_json::from_slice::<Value>(bytes).map_err(|error| {
        eprintln!("Herdr socket response JSON decode failed: {error}");
        HerdrError::ProtocolMismatch {
            message: format!("failed to decode Herdr socket response: {error}"),
        }
    })?;
    let error = value.get("error").and_then(|error| {
        Some(HerdrErrorEnvelope {
            code: error.get("code")?.as_str()?.to_owned(),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            retry_after_ms: error.get("retry_after_ms").and_then(Value::as_u64),
        })
    });
    Ok(HerdrEnvelope {
        result: value.get("result").cloned(),
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use atm_core::HerdrAgentName;

    fn test_deadline(after: Duration) -> Instant {
        Instant::now() + after
    }

    fn env(platform: Platform) -> HerdrHostEnv {
        HerdrHostEnv {
            xdg_config_home: Some(PathBuf::from("/xdg")),
            appdata: Some(PathBuf::from(r"C:\Users\alice\AppData\Roaming")),
            home: Some(PathBuf::from("/home/alice")),
            platform,
        }
    }

    #[test]
    fn endpoint_defaults_to_xdg_unix_socket() {
        let cfg = HerdrClientConfig::default();
        assert_eq!(
            herdr_api_endpoint(&cfg, None, &env(Platform::Unix)),
            HerdrEndpoint::UnixSocket(PathBuf::from("/xdg/herdr/herdr.sock"))
        );
    }

    #[test]
    fn endpoint_defaults_to_home_config_when_xdg_is_absent() {
        let cfg = HerdrClientConfig::default();
        let mut host = env(Platform::Unix);
        host.xdg_config_home = None;
        assert_eq!(
            herdr_api_endpoint(&cfg, None, &host),
            HerdrEndpoint::UnixSocket(PathBuf::from("/home/alice/.config/herdr/herdr.sock"))
        );
    }

    #[test]
    fn endpoint_uses_session_before_default() {
        let cfg = HerdrClientConfig::default();
        let session = HerdrSession::new("agent-session").expect("session");
        assert_eq!(
            herdr_api_endpoint(&cfg, Some(&session), &env(Platform::Unix)),
            HerdrEndpoint::UnixSocket(PathBuf::from(
                "/xdg/herdr/sessions/agent-session/herdr.sock"
            ))
        );
    }

    #[test]
    fn endpoint_default_session_uses_default_socket() {
        let cfg = HerdrClientConfig::default();
        let session = HerdrSession::new("default").expect("session");
        assert_eq!(
            herdr_api_endpoint(&cfg, Some(&session), &env(Platform::Unix)),
            HerdrEndpoint::UnixSocket(PathBuf::from("/xdg/herdr/herdr.sock"))
        );
    }

    #[test]
    fn endpoint_explicit_socket_path_wins_over_session() {
        let cfg = HerdrClientConfig::with_socket_path(PathBuf::from("/explicit/herdr.sock"));
        let session = HerdrSession::new("agent-session").expect("session");
        assert_eq!(
            herdr_api_endpoint(&cfg, Some(&session), &env(Platform::Unix)),
            HerdrEndpoint::UnixSocket(PathBuf::from("/explicit/herdr.sock"))
        );
    }

    #[test]
    fn endpoint_windows_uses_full_named_pipe_name() {
        let cfg = HerdrClientConfig::default();
        assert_eq!(
            herdr_api_endpoint(&cfg, None, &env(Platform::Windows)),
            HerdrEndpoint::NamedPipe(
                r"\\.\pipe\C:\Users\alice\AppData\Roaming\herdr\herdr.sock".to_owned()
            )
        );
    }

    #[test]
    fn endpoint_windows_uses_session_path() {
        let cfg = HerdrClientConfig::default();
        let session = HerdrSession::new("agent-session").expect("session");
        assert_eq!(
            herdr_api_endpoint(&cfg, Some(&session), &env(Platform::Windows)),
            HerdrEndpoint::NamedPipe(
                r"\\.\pipe\C:\Users\alice\AppData\Roaming\herdr\sessions\agent-session\herdr.sock"
                    .to_owned()
            )
        );
    }

    #[test]
    fn endpoint_windows_default_session_uses_default_socket() {
        let cfg = HerdrClientConfig::default();
        let session = HerdrSession::new("default").expect("session");
        assert_eq!(
            herdr_api_endpoint(&cfg, Some(&session), &env(Platform::Windows)),
            HerdrEndpoint::NamedPipe(
                r"\\.\pipe\C:\Users\alice\AppData\Roaming\herdr\herdr.sock".to_owned()
            )
        );
    }

    #[test]
    fn endpoint_windows_explicit_socket_path_wins_over_named_session() {
        let cfg = HerdrClientConfig::with_socket_path(PathBuf::from(r"C:\Temp\herdr.sock"));
        let session = HerdrSession::new("agent-session").expect("session");
        assert_eq!(
            herdr_api_endpoint(&cfg, Some(&session), &env(Platform::Windows)),
            HerdrEndpoint::NamedPipe(r"\\.\pipe\C:\Temp\herdr.sock".to_owned())
        );
    }

    #[test]
    fn endpoint_windows_explicit_socket_path_wins_over_default_session() {
        let cfg = HerdrClientConfig::with_socket_path(PathBuf::from(r"C:\Temp\herdr.sock"));
        let session = HerdrSession::new("default").expect("session");
        assert_eq!(
            herdr_api_endpoint(&cfg, Some(&session), &env(Platform::Windows)),
            HerdrEndpoint::NamedPipe(r"\\.\pipe\C:\Temp\herdr.sock".to_owned())
        );
    }

    #[test]
    fn endpoint_windows_explicit_pipe_path_is_not_prefixed_twice() {
        let cfg = HerdrClientConfig::with_socket_path(PathBuf::from(r"\\.\pipe\custom-herdr"));
        assert_eq!(
            herdr_api_endpoint(&cfg, None, &env(Platform::Windows)),
            HerdrEndpoint::NamedPipe(r"\\.\pipe\custom-herdr".to_owned())
        );
    }

    #[test]
    fn endpoint_windows_explicit_path_gets_the_pipe_prefix() {
        let cfg = HerdrClientConfig::with_socket_path(PathBuf::from(r"C:\Temp\herdr.sock"));
        assert_eq!(
            herdr_api_endpoint(&cfg, None, &env(Platform::Windows)),
            HerdrEndpoint::NamedPipe(r"\\.\pipe\C:\Temp\herdr.sock".to_owned())
        );
    }

    #[test]
    fn request_ids_and_ndjson_framing_are_stable() {
        let agent = HerdrAgentName::new("alice").expect("agent");
        let request = encode_request(HerdrOp::Get { agent: &agent }).expect("request");
        assert_eq!(request.last(), Some(&b'\n'));
        let value: Value = serde_json::from_slice(&request).expect("request JSON");
        assert_eq!(value["id"], "atm:agent:get");
        assert_eq!(value["method"], "agent.get");
    }

    #[test]
    fn doctor_ping_is_a_separate_ndjson_request() {
        let request = encode_ping_request();
        assert_eq!(request.last(), Some(&b'\n'));
        let value: Value = serde_json::from_slice(&request).expect("ping JSON");
        assert_eq!(value["id"], "atm:agent:ping");
        assert_eq!(value["method"], "ping");
    }

    #[test]
    fn malformed_response_remains_a_protocol_mismatch() {
        assert!(matches!(
            decode_envelope(b"{"),
            Err(HerdrError::ProtocolMismatch { .. })
        ));
    }

    #[test]
    fn refused_socket_connection_remains_server_unavailable() {
        assert!(matches!(
            socket_io_error(io::Error::from(io::ErrorKind::ConnectionRefused)),
            HerdrError::ServerUnavailable {
                retry_after: None,
                io_error_kind: Some(io::ErrorKind::ConnectionRefused),
                ..
            }
        ));
    }

    #[test]
    fn socket_variant_is_constructible_only_in_tests_until_ay9() {
        let socket = SocketIo::new(&HerdrClientConfig::default());
        let _io = crate::transport::HerdrIo::Socket(socket);
    }

    #[tokio::test(start_paused = true)]
    async fn socket_exchange_over_duplex_sends_one_request_and_decodes_one_response() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let (mut client, mut server) = tokio::io::duplex(1024);
        let server = tokio::spawn(async move {
            let mut request = Vec::new();
            tokio::io::BufReader::new(&mut server)
                .read_until(b'\n', &mut request)
                .await
                .expect("request");
            assert_eq!(request.last(), Some(&b'\n'));
            let value: Value = serde_json::from_slice(&request).expect("request JSON");
            assert_eq!(value["method"], "agent.get");
            server
                .write_all(
                    br#"{"result":{"agent":{"name":"alice","agent_status":"idle"}}}
"#,
                )
                .await
                .expect("response");
        });
        let agent = HerdrAgentName::new("alice").expect("agent");
        let request = encode_request(HerdrOp::Get { agent: &agent }).expect("request");
        let envelope = exchange_over_stream(
            &mut client,
            &request,
            HERDR_MAX_SOCKET_LINE_BYTES,
            test_deadline(Duration::from_secs(1)),
        )
        .await
        .expect("socket response");
        assert!(envelope.result.is_some());
        server.await.expect("server task");
    }

    #[tokio::test(start_paused = true)]
    async fn expired_deadline_is_rejected_before_socket_io() {
        let (mut client, _server) = tokio::io::duplex(16);
        let result =
            read_response_line(&mut client, HERDR_MAX_SOCKET_LINE_BYTES, Instant::now()).await;
        assert!(matches!(result, Err(HerdrError::Timeout)));
    }

    #[tokio::test(start_paused = true)]
    async fn expired_request_deadline_is_rejected_at_the_transport_entry() {
        assert!(matches!(
            tokio_deadline(RequestDeadline::after(Duration::ZERO)),
            Err(HerdrError::Timeout)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn injected_transport_entry_deadline_expires_before_connection() {
        let io = SocketIo::new(&HerdrClientConfig::default());
        let connector_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let connector_was_called = Arc::clone(&connector_called);

        let result = io
            .round_trip_with_deadline(
                b"request\n".to_vec(),
                None,
                Instant::now(),
                move |_endpoint, _deadline| async move {
                    connector_was_called.store(true, std::sync::atomic::Ordering::SeqCst);
                    let (client, _server) = tokio::io::duplex(16);
                    Ok(client)
                },
            )
            .await;

        assert!(matches!(result, Err(HerdrError::Timeout)));
        assert!(!connector_called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn injected_transport_entry_deadline_covers_connection_and_stream_io() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let io = SocketIo::new(&HerdrClientConfig::default());
        let deadline = test_deadline(Duration::from_nanos(4));
        let (client, mut server) = tokio::io::duplex(16);
        let (connect_started_tx, connect_started_rx) = tokio::sync::oneshot::channel();
        let (connect_gate_tx, connect_gate_rx) = tokio::sync::oneshot::channel();
        let (request_seen_tx, request_seen_rx) = tokio::sync::oneshot::channel();
        let (response_gate_tx, response_gate_rx) = tokio::sync::oneshot::channel();
        let server_task = tokio::spawn(async move {
            let mut request = [0_u8; 8];
            server
                .read_exact(&mut request)
                .await
                .expect("request reaches the stream after delayed connection");
            assert_eq!(&request, b"payload\n");
            request_seen_tx.send(()).expect("request observed");
            response_gate_rx.await.expect("response gate");
            let _ = server.write_all(b"{}\n").await;
        });
        let (result_tx, mut result_rx) = tokio::sync::oneshot::channel();
        let exchange = tokio::spawn(async move {
            let result = io
                .round_trip_with_deadline(
                    b"payload\n".to_vec(),
                    None,
                    deadline,
                    move |_endpoint, _deadline| async move {
                        connect_started_tx.send(()).expect("connector started");
                        connect_gate_rx.await.expect("release connector");
                        Ok(client)
                    },
                )
                .await;
            let _ = result_tx.send(result);
        });

        connect_started_rx.await.expect("connector entry");
        tokio::time::advance(Duration::from_nanos(3)).await;
        connect_gate_tx
            .send(())
            .expect("release connector before deadline");
        request_seen_rx
            .await
            .expect("request reached stream exchange");
        assert_eq!(Instant::now(), deadline - Duration::from_nanos(1));
        assert!(matches!(
            result_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        tokio::time::advance(Duration::from_nanos(1)).await;

        assert!(matches!(
            result_rx.await.expect("transport result"),
            Err(HerdrError::Timeout)
        ));
        response_gate_tx.send(()).expect("release response fixture");
        exchange.await.expect("transport task");
        server_task.await.expect("server fixture");
    }

    #[tokio::test(start_paused = true)]
    async fn delayed_transport_entry_keeps_the_injected_absolute_deadline() {
        let deadline = test_deadline(Duration::from_nanos(4));
        tokio::time::advance(Duration::from_nanos(3)).await;
        let (mut client, _server) = tokio::io::duplex(16);

        let result = exchange_over_stream(&mut client, b"request\n", 128, deadline).await;

        assert!(matches!(result, Err(HerdrError::Timeout)));
        assert!(Instant::now() >= deadline);
    }

    #[tokio::test(start_paused = true)]
    async fn nested_write_and_read_share_the_injected_deadline_at_the_exact_boundary() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let deadline = test_deadline(Duration::from_nanos(4));
        let (mut client, mut server) = tokio::io::duplex(1);
        let (read_gate_tx, read_gate_rx) = tokio::sync::oneshot::channel();
        let (request_seen_tx, request_seen_rx) = tokio::sync::oneshot::channel();
        let (response_gate_tx, response_gate_rx) = tokio::sync::oneshot::channel();
        let server_task = tokio::spawn(async move {
            read_gate_rx.await.expect("read gate");
            let mut request = [0_u8; 8];
            server
                .read_exact(&mut request)
                .await
                .expect("full request after read gate");
            assert_eq!(&request, b"payload\n");
            request_seen_tx.send(()).expect("request observed");
            response_gate_rx.await.expect("response gate");
            let _ = server.write_all(b"{}\n").await;
        });
        let (result_tx, mut result_rx) = tokio::sync::oneshot::channel();
        let exchange = tokio::spawn(async move {
            let result = exchange_over_stream(&mut client, b"payload\n", 128, deadline).await;
            let _ = result_tx.send(result);
        });

        tokio::time::advance(Duration::from_nanos(2)).await;
        read_gate_tx.send(()).expect("release server read");
        request_seen_rx.await.expect("server received request");
        tokio::time::advance(Duration::from_nanos(1)).await;
        assert!(
            Instant::now() < deadline,
            "d-1ns remains before the deadline"
        );
        assert!(matches!(
            result_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        tokio::time::advance(Duration::from_nanos(1)).await;
        assert_eq!(Instant::now(), deadline);

        let result = result_rx.await.expect("exchange result");
        assert!(matches!(result, Err(HerdrError::Timeout)));
        response_gate_tx.send(()).expect("release response fixture");
        exchange.await.expect("exchange task");
        server_task.await.expect("server fixture");
    }

    #[tokio::test(start_paused = true)]
    async fn permit_wait_uses_the_call_deadline_and_releases_cleanly() {
        let permits = Arc::new(Semaphore::new(1));
        let held = acquire_permit(Arc::clone(&permits), test_deadline(Duration::from_secs(1)))
            .await
            .expect("first permit");
        let result = acquire_permit(Arc::clone(&permits), Instant::now()).await;
        assert!(matches!(result, Err(HerdrError::Timeout)));
        drop(held);
        let _ = acquire_permit(permits, test_deadline(Duration::from_secs(1)))
            .await
            .expect("released permit");
    }

    #[tokio::test(start_paused = true)]
    async fn seventeen_callers_are_bounded_by_sixteen_permits() {
        let permits = Arc::new(Semaphore::new(SOCKET_PERMITS));
        let mut held = Vec::new();
        for _ in 0..SOCKET_PERMITS {
            held.push(
                acquire_permit(Arc::clone(&permits), test_deadline(Duration::from_secs(1)))
                    .await
                    .expect("permit"),
            );
        }

        let waiter = tokio::spawn(acquire_permit(
            Arc::clone(&permits),
            test_deadline(Duration::from_secs(1)),
        ));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(matches!(
            waiter.await.expect("waiter"),
            Err(HerdrError::Timeout)
        ));
        drop(held);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_a_permit_wait_releases_all_state() {
        let permits = Arc::new(Semaphore::new(1));
        let held = acquire_permit(Arc::clone(&permits), test_deadline(Duration::from_secs(1)))
            .await
            .expect("held permit");
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let waiter_permits = Arc::clone(&permits);
        let waiter = tokio::spawn(async move {
            let _ = started_tx.send(());
            acquire_permit(waiter_permits, test_deadline(Duration::from_secs(1))).await
        });
        started_rx.await.expect("waiter reached permit acquisition");
        waiter.abort();
        let _ = waiter.await;
        drop(held);
        let _ = acquire_permit(permits, test_deadline(Duration::from_secs(1)))
            .await
            .expect("permit after cancellation");
    }

    #[tokio::test(start_paused = true)]
    async fn eof_without_newline_preserves_observed_byte_count() {
        let result = read_response_line(
            &mut std::io::Cursor::new(b"{}".to_vec()),
            HERDR_MAX_SOCKET_LINE_BYTES,
            test_deadline(Duration::from_secs(1)),
        )
        .await;
        assert!(matches!(
            result,
            Err(HerdrError::InternalError { message })
                if message.contains("after 2 bytes")
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn oversized_response_is_rejected_at_the_line_boundary() {
        let result = read_response_line(
            &mut std::io::Cursor::new(b"abcd\n".to_vec()),
            3,
            test_deadline(Duration::from_secs(1)),
        )
        .await;
        assert!(matches!(
            result,
            Err(HerdrError::InternalError { message })
                if message.contains("exceeded the 3-byte limit at 4 bytes")
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn stalled_connect_honors_the_absolute_deadline() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let deadline = test_deadline(Duration::from_secs(1));
        let task = tokio::spawn(connect_with_deadline(deadline, async move {
            let _ = started_tx.send(());
            std::future::pending::<io::Result<()>>().await
        }));
        started_rx.await.expect("connect future was polled");
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(matches!(
            task.await.expect("connect task"),
            Err(HerdrError::Timeout)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn stalled_write_honors_the_absolute_deadline() {
        let (mut client, _server) = tokio::io::duplex(1);
        let request = vec![b'x'; 4096];
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let deadline = test_deadline(Duration::from_secs(1));
        let task = tokio::spawn(async move {
            let _ = started_tx.send(());
            write_request(&mut client, &request, deadline).await
        });
        started_rx.await.expect("write future was polled");
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(matches!(
            task.await.expect("write task"),
            Err(HerdrError::Timeout)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_io_wait_does_not_retain_the_socket_permit() {
        let permits = Arc::new(Semaphore::new(1));
        let held = acquire_permit(Arc::clone(&permits), test_deadline(Duration::from_secs(1)))
            .await
            .expect("held permit");
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let waiter_permits = Arc::clone(&permits);
        let waiter = tokio::spawn(async move {
            let _ = started_tx.send(());
            acquire_permit(waiter_permits, test_deadline(Duration::from_secs(1))).await
        });
        started_rx.await.expect("waiter reached permit acquisition");
        waiter.abort();
        let _ = waiter.await;
        drop(held);
        let _ = acquire_permit(permits, test_deadline(Duration::from_secs(1)))
            .await
            .expect("permit after I/O cancellation");
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_connect_write_and_read_futures_is_clean() {
        let (connect_started_tx, connect_started_rx) = tokio::sync::oneshot::channel();
        let connect = tokio::spawn(connect_with_deadline(
            test_deadline(Duration::from_secs(1)),
            async move {
                let _ = connect_started_tx.send(());
                std::future::pending::<io::Result<()>>().await
            },
        ));
        connect_started_rx.await.expect("connect future was polled");
        connect.abort();
        assert!(connect.await.is_err());

        let (write_started_tx, write_started_rx) = tokio::sync::oneshot::channel();
        let write = tokio::spawn(deadline_io(
            test_deadline(Duration::from_secs(1)),
            async move {
                let _ = write_started_tx.send(());
                std::future::pending::<io::Result<()>>().await
            },
        ));
        write_started_rx.await.expect("write future was polled");
        write.abort();
        assert!(write.await.is_err());

        let (read_started_tx, read_started_rx) = tokio::sync::oneshot::channel();
        let read = tokio::spawn(deadline_io(
            test_deadline(Duration::from_secs(1)),
            async move {
                let _ = read_started_tx.send(());
                std::future::pending::<io::Result<usize>>().await
            },
        ));
        read_started_rx.await.expect("read future was polled");
        read.abort();
        assert!(read.await.is_err());
    }

    #[cfg(windows)]
    #[tokio::test(start_paused = true)]
    async fn named_pipe_busy_retry_is_covered_with_a_bounded_deadline() {
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = Arc::clone(&attempts);
        let (busy_tx, busy_rx) = tokio::sync::oneshot::channel();
        let mut busy_tx = Some(busy_tx);
        let deadline = test_deadline(PIPE_BUSY_RETRY_DELAY + Duration::from_nanos(1));
        let retry = tokio::spawn(retry_pipe_busy(
            deadline,
            PIPE_BUSY_RETRY_DELAY,
            move || {
                if observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
                    busy_tx
                        .take()
                        .expect("first busy notification")
                        .send(())
                        .expect("busy receiver is waiting");
                    Err(io::Error::from_raw_os_error(231))
                } else {
                    Ok(())
                }
            },
        ));
        busy_rx
            .await
            .expect("first busy result reached retry sleep");
        tokio::time::advance(PIPE_BUSY_RETRY_DELAY).await;
        retry.await.expect("retry task").expect("retry");
        assert_eq!(attempts.load(std::sync::atomic::Ordering::Relaxed), 2);
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = Arc::clone(&attempts);
        let expired = retry_pipe_busy(Instant::now(), PIPE_BUSY_RETRY_DELAY, move || {
            observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Err::<(), _>(io::Error::from_raw_os_error(231))
        })
        .await;
        assert!(matches!(expired, Err(HerdrError::Timeout)));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn named_pipe_connect_exchanges_one_request_with_a_server() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::windows::named_pipe::ServerOptions;

        let pipe_name = unique_test_pipe_name();
        let mut server = ServerOptions::new()
            .create(&pipe_name)
            .expect("create test named-pipe server");
        let endpoint = HerdrEndpoint::NamedPipe(pipe_name);

        let mut client = connect_named_pipe(
            &endpoint,
            RequestDeadline::after(Duration::from_secs(5)),
            PIPE_BUSY_RETRY_DELAY,
        )
        .await
        .expect("connect to test named-pipe server");
        server.connect().await.expect("accept named-pipe client");

        client
            .write_all(b"request\n")
            .await
            .expect("write one request");
        let mut request = [0; 8];
        server
            .read_exact(&mut request)
            .await
            .expect("read one request");
        assert_eq!(&request, b"request\n");

        server
            .write_all(b"response\n")
            .await
            .expect("write one response");
        let mut response = [0; 9];
        client
            .read_exact(&mut response)
            .await
            .expect("read one response");
        assert_eq!(&response, b"response\n");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn missing_named_pipe_returns_a_typed_error_before_deadline() {
        let endpoint = HerdrEndpoint::NamedPipe(unique_test_pipe_name());
        let deadline = RequestDeadline::after(Duration::from_secs(5));

        let result = connect_named_pipe(&endpoint, deadline, PIPE_BUSY_RETRY_DELAY).await;

        assert!(matches!(result, Err(HerdrError::ServerUnavailable { .. })));
    }

    #[cfg(windows)]
    fn unique_test_pipe_name() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_PIPE_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_PIPE_ID.fetch_add(1, Ordering::Relaxed);
        format!(r"\\.\pipe\atm-herdr-test-{}-{id}", std::process::id())
    }
}
