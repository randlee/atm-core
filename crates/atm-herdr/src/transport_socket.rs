//! Direct Herdr NDJSON transport over the server's local socket.

#![allow(dead_code)]

use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use atm_core::{HerdrSession, RequestDeadline};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Semaphore;

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
        Self {
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            appdata: std::env::var_os("APPDATA").map(PathBuf::from),
            home: std::env::var_os("HOME").map(PathBuf::from),
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
        let permit = acquire_permit(Arc::clone(&self.in_flight), deadline).await?;
        let endpoint = herdr_api_endpoint(&self.cfg, session, &self.env);
        let request = encode_request(op)?;

        #[cfg(unix)]
        let mut stream = connect_unix(&endpoint, deadline).await?;
        #[cfg(windows)]
        let mut stream =
            connect_named_pipe(&endpoint, deadline, self.pipe_busy_retry_delay).await?;

        write_request(&mut stream, &request, deadline).await?;
        let response = read_response_line(&mut stream, self.max_line_bytes, deadline).await?;
        drop(stream);
        drop(permit);
        decode_envelope(&response)
    }
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
            let raw = cfg.socket_path().cloned().or_else(|| {
                session.map(|session| {
                    config_dir(env)
                        .join("sessions")
                        .join(session.as_str())
                        .join("herdr.sock")
                })
            });
            HerdrEndpoint::UnixSocket(raw.unwrap_or_else(|| config_dir(env).join("herdr.sock")))
        }
        Platform::Windows => {
            let raw = cfg.socket_path().cloned().or_else(|| {
                session.map(|session| {
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
    deadline: RequestDeadline,
) -> Result<tokio::sync::OwnedSemaphorePermit, HerdrError> {
    let Some(remaining) = deadline.remaining() else {
        return Err(HerdrError::Timeout);
    };
    match tokio::time::timeout(remaining, in_flight.acquire_owned()).await {
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
        HerdrOp::Notify { title, body } => json!({
            "id": "atm:agent:notify",
            "method": "notification.show",
            "params": {"title": title, "body": body, "sound": "request"},
        }),
    };
    let mut bytes = serde_json::to_vec(&value).map_err(|error| HerdrError::InternalError {
        message: format!("failed to encode Herdr socket request: {error}"),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

async fn write_request<S>(
    stream: &mut S,
    request: &[u8],
    deadline: RequestDeadline,
) -> Result<(), HerdrError>
where
    S: AsyncWrite + Unpin,
{
    deadline_io(deadline, stream.write_all(request)).await?;
    deadline_io(deadline, stream.flush()).await
}

async fn read_response_line<S>(
    stream: &mut S,
    max_line_bytes: usize,
    deadline: RequestDeadline,
) -> Result<Vec<u8>, HerdrError>
where
    S: AsyncRead + Unpin,
{
    let mut line = Vec::with_capacity(4096);
    loop {
        let mut byte = [0_u8; 1];
        let read = deadline_io(deadline, stream.read(&mut byte)).await?;
        if read == 0 {
            return Err(incomplete_response(line.len()));
        }
        line.push(byte[0]);
        if byte[0] == b'\n' {
            line.pop();
            return Ok(line);
        }
        if line.len() > max_line_bytes {
            return Err(oversized_response(line.len(), max_line_bytes));
        }
    }
}

async fn deadline_io<T>(
    deadline: RequestDeadline,
    future: impl Future<Output = io::Result<T>>,
) -> Result<T, HerdrError> {
    let Some(remaining) = deadline.remaining() else {
        return Err(HerdrError::Timeout);
    };
    match tokio::time::timeout(remaining, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(socket_io_error(error)),
        Err(_) => Err(HerdrError::Timeout),
    }
}

fn socket_io_error(error: io::Error) -> HerdrError {
    HerdrError::ServerUnavailable {
        message: format!("Herdr socket transport failed: {error}"),
        retry_after: None,
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

#[cfg(unix)]
async fn connect_unix(
    endpoint: &HerdrEndpoint,
    deadline: RequestDeadline,
) -> Result<tokio::net::UnixStream, HerdrError> {
    let HerdrEndpoint::UnixSocket(path) = endpoint else {
        return Err(HerdrError::InternalError {
            message: "Windows Herdr endpoint selected on a Unix build".to_owned(),
        });
    };
    let Some(remaining) = deadline.remaining() else {
        return Err(HerdrError::Timeout);
    };
    match tokio::time::timeout(remaining, tokio::net::UnixStream::connect(path)).await {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(error)) => Err(socket_io_error(error)),
        Err(_) => Err(HerdrError::Timeout),
    }
}

#[cfg(windows)]
async fn connect_named_pipe(
    endpoint: &HerdrEndpoint,
    deadline: RequestDeadline,
    retry_delay: Duration,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, HerdrError> {
    const ERROR_PIPE_BUSY: i32 = 231;
    let HerdrEndpoint::NamedPipe(name) = endpoint else {
        return Err(HerdrError::InternalError {
            message: "Unix Herdr endpoint selected on a Windows build".to_owned(),
        });
    };
    loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(name) {
            Ok(client) => return Ok(client),
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                let Some(remaining) = deadline.remaining() else {
                    return Err(HerdrError::Timeout);
                };
                tokio::time::sleep(retry_delay.min(remaining)).await;
            }
            Err(error) => return Err(socket_io_error(error)),
        }
    }
}

fn decode_envelope(bytes: &[u8]) -> Result<HerdrEnvelope, HerdrError> {
    let value = serde_json::from_slice::<Value>(bytes).map_err(|error| {
        eprintln!("Herdr socket response JSON decode failed: {error}");
        HerdrError::ProtocolMismatch
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
    use atm_core::types::AgentName;

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
    fn endpoint_explicit_socket_path_wins_over_session() {
        let cfg = HerdrClientConfig {
            socket_path: Some(PathBuf::from("/explicit/herdr.sock")),
            ..HerdrClientConfig::default()
        };
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
    fn endpoint_windows_explicit_pipe_path_is_not_prefixed_twice() {
        let cfg = HerdrClientConfig {
            socket_path: Some(PathBuf::from(r"\\.\pipe\custom-herdr")),
            ..HerdrClientConfig::default()
        };
        assert_eq!(
            herdr_api_endpoint(&cfg, None, &env(Platform::Windows)),
            HerdrEndpoint::NamedPipe(r"\\.\pipe\custom-herdr".to_owned())
        );
    }

    #[test]
    fn endpoint_windows_explicit_path_gets_the_pipe_prefix() {
        let cfg = HerdrClientConfig {
            socket_path: Some(PathBuf::from(r"C:\Temp\herdr.sock")),
            ..HerdrClientConfig::default()
        };
        assert_eq!(
            herdr_api_endpoint(&cfg, None, &env(Platform::Windows)),
            HerdrEndpoint::NamedPipe(r"\\.\pipe\C:\Temp\herdr.sock".to_owned())
        );
    }

    #[test]
    fn request_ids_and_ndjson_framing_are_stable() {
        let agent: AgentName = "alice".parse().expect("agent");
        let request = encode_request(HerdrOp::Get { agent: &agent }).expect("request");
        assert_eq!(request.last(), Some(&b'\n'));
        let value: Value = serde_json::from_slice(&request).expect("request JSON");
        assert_eq!(value["id"], "atm:agent:get");
        assert_eq!(value["method"], "agent.get");
    }

    #[test]
    fn malformed_response_remains_a_protocol_mismatch() {
        assert!(matches!(
            decode_envelope(b"{"),
            Err(HerdrError::ProtocolMismatch)
        ));
    }

    #[test]
    fn socket_variant_is_constructible_only_in_tests_until_ay9() {
        let socket = SocketIo::new(&HerdrClientConfig::default());
        let _io = crate::transport::HerdrIo::Socket(socket);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_socket_call_sends_one_request_and_decodes_one_response() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("atm-herdr-ay8-{nonce}.sock"));
        let listener = tokio::net::UnixListener::bind(&path).expect("socket listener");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("client");
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = tokio::io::BufReader::new(read_half);
            let mut request = Vec::new();
            tokio::io::AsyncBufReadExt::read_until(&mut reader, b'\n', &mut request)
                .await
                .expect("request");
            assert_eq!(request.last(), Some(&b'\n'));
            let value: Value = serde_json::from_slice(&request).expect("request JSON");
            assert_eq!(value["method"], "agent.get");
            tokio::io::AsyncWriteExt::write_all(
                &mut write_half,
                br#"{"result":{"agent":{"name":"alice","agent_status":"idle"}}}
"#,
            )
            .await
            .expect("response");
        });

        let io = SocketIo {
            cfg: HerdrClientConfig {
                socket_path: Some(path.clone()),
                ..HerdrClientConfig::default()
            },
            env: env(Platform::Unix),
            max_line_bytes: HERDR_MAX_SOCKET_LINE_BYTES,
            in_flight: Arc::new(Semaphore::new(SOCKET_PERMITS)),
            pipe_busy_retry_delay: PIPE_BUSY_RETRY_DELAY,
        };
        let agent: AgentName = "alice".parse().expect("agent");
        let envelope = io
            .call(
                HerdrOp::Get { agent: &agent },
                None,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("socket response");
        assert!(envelope.result.is_some());
        server.await.expect("server task");
        std::fs::remove_file(path).expect("socket cleanup");
    }

    #[tokio::test]
    async fn expired_deadline_is_rejected_before_socket_io() {
        let result = read_response_line(
            &mut tokio::io::empty(),
            HERDR_MAX_SOCKET_LINE_BYTES,
            RequestDeadline::after(Duration::ZERO),
        )
        .await;
        assert!(matches!(result, Err(HerdrError::Timeout)));
    }

    #[tokio::test]
    async fn eof_without_newline_preserves_observed_byte_count() {
        let result = read_response_line(
            &mut std::io::Cursor::new(b"{}".to_vec()),
            HERDR_MAX_SOCKET_LINE_BYTES,
            RequestDeadline::after(Duration::from_secs(1)),
        )
        .await;
        assert!(matches!(
            result,
            Err(HerdrError::InternalError { message })
                if message.contains("after 2 bytes")
        ));
    }

    #[tokio::test]
    async fn oversized_response_is_rejected_at_the_line_boundary() {
        let result = read_response_line(
            &mut std::io::Cursor::new(b"abcd\n".to_vec()),
            3,
            RequestDeadline::after(Duration::from_secs(1)),
        )
        .await;
        assert!(matches!(
            result,
            Err(HerdrError::InternalError { message })
                if message.contains("exceeded the 3-byte limit at 4 bytes")
        ));
    }
}
