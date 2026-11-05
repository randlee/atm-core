#![allow(
    deprecated,
    reason = "this crate retains the deprecated IPC implementation only for the bounded daemon-bootstrap deletion lane"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fmt, thread};

use atm_core::caller_context::{CallerContext, CallerContextOverrides, resolve_cli_caller_context};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_storage::{AgentName, AtmError, TeamName};
#[cfg(unix)]
use interprocess::local_socket::Stream as LocalSocketStream;
#[cfg(unix)]
use interprocess::local_socket::prelude::*;
use std::net::TcpStream;
use std::sync::Arc;

use atm_core::api::{ApiRequest, DaemonApiClient};
pub use atm_core::protocol::{CompatibilityPreflight, CompatibilityVerdict, ReleaseVersion};

mod compatibility;
mod local_transport;

pub use compatibility::{Connection, Unverified, VersionVerified, verify_connection_compatibility};
pub use local_transport::{LocalDaemonTransport, local_daemon_transport};

const LOCAL_IPC_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
// The request deadline governs daemon work. Keep a narrow response-only grace
// so a daemon that reaches that deadline can serialize its typed terminal
// result instead of racing the client's identical socket-read timeout.
const LOCAL_IPC_RESPONSE_GRACE: Duration = Duration::from_millis(250);
#[derive(Debug, Clone)]
pub struct DaemonLocalIpcEndpoint(PathBuf);

impl DaemonLocalIpcEndpoint {
    pub fn new(path: PathBuf) -> Result<Self, AtmError> {
        validate_daemon_path("daemon local IPC endpoint", &path)?;
        Ok(Self(path))
    }

    pub fn display(&self) -> std::path::Display<'_> {
        self.0.display()
    }
}

impl AsRef<Path> for DaemonLocalIpcEndpoint {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl DaemonLocalIpcEndpoint {
    #[sc_lint_attributes::sc_lint(
        allow(PORT-010),
        reason = "Unix resolves to a filesystem socket path; Windows connects through loopback_tcp_client using the host-scoped local HTTP record."
    )]
    #[cfg(unix)]
    fn unix_socket_path(&self) -> Result<PathBuf, AtmError> {
        self.0.parent().map_or_else(
            || {
                Err(AtmError::daemon_unavailable(
                    "daemon local endpoint record has no runtime directory",
                ))
            },
            |runtime_dir| Ok(runtime_dir.join(atm_core::home::HOST_RUNTIME_SOCKET_FILE)),
        )
    }
}

fn validate_daemon_path(label: &str, path: &Path) -> Result<(), AtmError> {
    if path.as_os_str().is_empty() {
        return Err(AtmError::validation(format!("{label} must not be empty")));
    }
    if path.to_str().is_none() {
        return Err(AtmError::validation(format!(
            "{label} must be valid UTF-8 at the ATM boundary"
        )));
    }
    Ok(())
}

#[cfg(windows)]
fn reject_socket_override() -> Result<(), AtmError> {
    if std::env::var_os("ATM_DAEMON_SOCKET").is_some_and(|value| !value.is_empty()) {
        return Err(AtmError::socket_override_forbidden(
            "ATM_DAEMON_SOCKET cannot override the host singleton endpoint",
        ));
    }
    Ok(())
}

/// # Errors
///
/// Returns [`AtmError`] when the canonical same-host daemon HTTP record cannot
/// be resolved into a local IPC endpoint.
pub fn resolve_daemon_local_ipc_endpoint() -> Result<DaemonLocalIpcEndpoint, AtmError> {
    DaemonLocalIpcEndpoint::new(host_local_http_record_path()?)
}

/// # Errors
///
/// Returns [`AtmError`] when the canonical host-scoped daemon HTTP record
/// cannot be resolved into a local IPC endpoint.
///
/// The parameter is retained for source compatibility with callers that
/// previously supplied an ATM configuration home. Runtime endpoint discovery
/// is intentionally independent of `ATM_HOME`.
pub fn resolve_daemon_local_ipc_endpoint_from_home(
    _home_dir: &Path,
) -> Result<DaemonLocalIpcEndpoint, AtmError> {
    resolve_daemon_local_ipc_endpoint()
}

fn host_local_http_record_path() -> Result<PathBuf, AtmError> {
    #[cfg(windows)]
    reject_socket_override()?;

    let runtime_scope = atm_core::home::current_host_runtime_scope()?;
    Ok(runtime_scope
        .runtime_root
        .as_ref()
        .join(atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME))
}

pub fn parse_bootstrap_caller_context() -> Result<CallerContext, AtmError> {
    resolve_cli_caller_context(CallerContextOverrides::default())
}

pub fn parse_bootstrap_agent() -> Result<AgentName, AtmError> {
    Ok(parse_bootstrap_caller_context()?.caller_identity)
}

pub fn parse_bootstrap_team() -> Result<TeamName, AtmError> {
    Ok(parse_bootstrap_caller_context()?.caller_team)
}

/// A connected same-host daemon stream.
///
/// Unix UDS and loopback TCP carry the same HTTP request/response contract;
/// the variant only records the explicitly selected transport.
pub enum LocalDaemonConnection {
    #[cfg(unix)]
    UnixDomainSocket(LocalSocketStream),
    TcpLoopback(TcpStream),
}

#[deprecated(
    note = "legacy synchronous IPC framing; use atm-http-runtime's capability-authenticated Tokio client"
)]
pub fn try_connect(endpoint: &DaemonLocalIpcEndpoint) -> Result<LocalDaemonConnection, AtmError> {
    try_connect_with_transport(endpoint, local_daemon_transport()?)
}

#[sc_lint_attributes::sc_lint(
    allow(PORT-010),
    reason = "This adapter connects through a Unix domain socket; Windows uses loopback TCP via try_connect_local_http_record and the host-scoped endpoint record."
)]
#[cfg(unix)]
fn try_connect_unix_socket(
    endpoint: &DaemonLocalIpcEndpoint,
) -> Result<LocalSocketStream, AtmError> {
    let socket_path = endpoint.unix_socket_path()?;
    let name = atm_core::protocol::daemon_local_ipc_name_from_path(&socket_path)?;
    LocalSocketStream::connect(name).map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            format!(
                "failed to connect to daemon UDS endpoint {}: {source}",
                socket_path.display(),
            ),
            source,
        )
    })
}

fn try_connect_local_http_record(record_path: &Path) -> Result<TcpStream, AtmError> {
    let record = load_local_http_record(record_path)?;
    record.capability()?;
    let endpoint = record
        .ipv4_loopback
        .or(record.ipv6_loopback)
        .ok_or_else(|| {
            AtmError::daemon_unavailable("local HTTP endpoint record has no loopback endpoint")
        })?;
    TcpStream::connect_timeout(&endpoint, LOCAL_IPC_CONNECT_DEADLINE).map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            format!("failed to connect to daemon local HTTP endpoint {endpoint}: {source}"),
            source,
        )
    })
}

fn load_local_http_record(
    record_path: &Path,
) -> Result<atm_core::local_http::LocalHttpEndpointRecord, AtmError> {
    let contents = fs::read(record_path).map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            format!(
                "failed to read local HTTP endpoint record {}: {source}",
                record_path.display()
            ),
            source,
        )
    })?;
    let record: atm_core::local_http::LocalHttpEndpointRecord = serde_json::from_slice(&contents)
        .map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            format!(
                "failed to parse local HTTP endpoint record {}: {source}",
                record_path.display()
            ),
            source,
        )
    })?;
    record.capability()?;
    let owner_instance_id =
        atm_core::local_http::owner_instance_id_for_local_http_record(record_path)?;
    if record.daemon_instance_id != owner_instance_id {
        return Err(AtmError::daemon_unavailable(
            "local HTTP endpoint record belongs to a different daemon instance",
        ));
    }
    Ok(record)
}

/// Exchange one canonical request through the selected same-host HTTP transport.
///
/// Unix defaults to the daemon-owned UDS. TCP is used only when explicitly
/// requested with `ATM_LOCAL_TRANSPORT=tcp`; Windows always uses that path.
#[deprecated(
    note = "legacy synchronous IPC framing; use atm-http-runtime's capability-authenticated Tokio client"
)]
pub fn exchange_request(
    endpoint: &DaemonLocalIpcEndpoint,
    request: &RequestEnvelope,
    request_deadline: Duration,
) -> Result<ResponseEnvelope, AtmError> {
    exchange_request_with_transport(
        endpoint,
        request,
        request_deadline,
        local_daemon_transport()?,
    )
}

fn exchange_request_with_transport(
    endpoint: &DaemonLocalIpcEndpoint,
    request: &RequestEnvelope,
    request_deadline: Duration,
    transport: LocalDaemonTransport,
) -> Result<ResponseEnvelope, AtmError> {
    let response_deadline = request_deadline.saturating_add(LOCAL_IPC_RESPONSE_GRACE);
    execute_typed_local_request(
        typed_client_for_transport(endpoint, transport, response_deadline)?,
        request.clone(),
    )
}

fn typed_client_for_transport(
    endpoint: &DaemonLocalIpcEndpoint,
    transport: LocalDaemonTransport,
    request_timeout: Duration,
) -> Result<Arc<dyn DaemonApiClient>, AtmError> {
    match transport {
        #[cfg(unix)]
        LocalDaemonTransport::UnixDomainSocket => {
            atm_http_runtime::unix_socket_client(endpoint.unix_socket_path()?, request_timeout)
        }
        LocalDaemonTransport::TcpLoopback => {
            atm_http_runtime::loopback_tcp_client(endpoint.as_ref(), request_timeout)
        }
    }
}

fn execute_typed_local_request(
    client: Arc<dyn DaemonApiClient>,
    request: RequestEnvelope,
) -> Result<ResponseEnvelope, AtmError> {
    let execute = move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to build typed local HTTP client runtime")
                    .with_cause(source)
            })?
            .block_on(async move {
                client
                    .execute(ApiRequest::from(request))
                    .await
                    .map(|response| response.into_inner())
            })
    };

    if tokio::runtime::Handle::try_current().is_ok() {
        thread::spawn(execute).join().map_err(|_panic| {
            AtmError::daemon_unavailable("typed local HTTP client runtime panicked")
        })?
    } else {
        execute()
    }
}

/// Execute one typed request through the retained compatibility bridge.
///
/// New callers should remain async at their public boundary. This helper is
/// limited to synchronous worker integrations that already own a configured
/// [`DaemonApiClient`], such as the embedded graft receiver loop.
pub fn execute_api_request(
    client: Arc<dyn DaemonApiClient>,
    request: RequestEnvelope,
) -> Result<ResponseEnvelope, AtmError> {
    execute_typed_local_request(client, request)
}

fn try_connect_with_transport(
    endpoint: &DaemonLocalIpcEndpoint,
    transport: LocalDaemonTransport,
) -> Result<LocalDaemonConnection, AtmError> {
    match transport {
        #[cfg(unix)]
        LocalDaemonTransport::UnixDomainSocket => {
            try_connect_unix_socket(endpoint).map(LocalDaemonConnection::UnixDomainSocket)
        }
        LocalDaemonTransport::TcpLoopback => {
            try_connect_local_http_record(endpoint.as_ref()).map(LocalDaemonConnection::TcpLoopback)
        }
    }
}

pub fn unexpected_response(command: &str, response: impl fmt::Debug) -> AtmError {
    AtmError::validation(format!(
        "transport returned an unexpected response for `{command}`: {response:?}"
    ))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    #[cfg(unix)]
    use super::{DaemonLocalIpcEndpoint, LocalDaemonTransport, try_connect_with_transport};
    use super::{resolve_daemon_local_ipc_endpoint, resolve_daemon_local_ipc_endpoint_from_home};

    #[test]
    fn local_ipc_endpoint_is_host_scoped_not_atm_home_scoped() {
        let supplied_atm_home = TempDir::new().expect("temp ATM home");
        let expected = atm_core::home::current_host_runtime_scope()
            .expect("host runtime scope")
            .runtime_root
            .as_ref()
            .join(atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME);

        let canonical = resolve_daemon_local_ipc_endpoint().expect("canonical endpoint");
        let compatibility_endpoint =
            resolve_daemon_local_ipc_endpoint_from_home(supplied_atm_home.path())
                .expect("compatibility endpoint");

        assert_eq!(canonical.as_ref(), expected);
        assert_eq!(compatibility_endpoint.as_ref(), expected);
    }

    #[cfg(unix)]
    #[test]
    fn uds_connect_failure_keeps_the_socket_cause_in_the_displayed_error() {
        let tempdir = TempDir::new().expect("temp runtime");
        let socket_path = tempdir
            .path()
            .join(atm_core::home::HOST_RUNTIME_SOCKET_FILE);
        let result = try_connect_with_transport(
            &DaemonLocalIpcEndpoint::new(
                tempdir
                    .path()
                    .join(atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME),
            )
            .expect("endpoint"),
            LocalDaemonTransport::UnixDomainSocket,
        );
        let Err(error) = result else {
            panic!("no UDS listener exists");
        };
        assert!(
            error.message().contains(&socket_path.display().to_string()),
            "the displayed error identifies the failed UDS endpoint: {error:?}"
        );
        assert!(
            error.cause().is_some_and(|cause| !cause.is_empty()),
            "the underlying UDS connection failure remains structured"
        );
    }
}
