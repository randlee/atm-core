use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use atm_core::dispatcher::{DaemonRequest, DaemonResponse, DispatchError};
use atm_core::doctor::DoctorQuery;
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode as Code;
use atm_core::types::{AgentName, TeamName};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::net::UnixStream;

use crate::{
    AUTO_START_PUBLISH_TIMEOUT, ControlState, DaemonPaths, LocalEndpoint, MAX_FRAME_BYTES,
    REMOTE_CONNECT_TIMEOUT, REMOTE_IO_TIMEOUT, SAME_HOST_REQUEST_TIMEOUT, WireError,
    WireResponseEnvelope,
};

pub(crate) const SAME_HOST_SERVER_IO_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const REMOTE_SERVER_IO_TIMEOUT: Duration = Duration::from_secs(5);

pub fn request_doctor_json_with_autostart(query: DoctorQuery) -> Result<String, AtmError> {
    let (team_name, agent_name) =
        resolve_request_identity(query.team_override.clone(), Some(&query.current_dir), true)?;
    let request = DaemonRequest {
        team_name,
        agent_name,
        payload: atm_core::dispatcher::RequestPayload::Doctor(
            serde_json::to_value(&query).map_err(|error| {
                AtmError::daemon_protocol("failed to serialize doctor query").with_source(error)
            })?,
        ),
    };

    let home_dir = query.home_dir.clone();
    match request_local(&home_dir, &request, SAME_HOST_REQUEST_TIMEOUT) {
        Ok(response) => Ok(response.payload_json),
        Err(error) if should_retry_autostart(&error) => {
            auto_start_daemon(&home_dir)?;
            let response = request_local(&home_dir, &request, SAME_HOST_REQUEST_TIMEOUT)
                .map_err(add_daemon_ready_recovery)?;
            Ok(response.payload_json)
        }
        Err(error) => Err(error),
    }
}

pub fn ensure_daemon_running(home_dir: &Path) -> Result<(), AtmError> {
    let (team_name, agent_name) = resolve_request_identity(None, None, false)?;
    let heartbeat_request = DaemonRequest {
        team_name,
        agent_name,
        payload: atm_core::dispatcher::RequestPayload::Heartbeat(serde_json::json!({
            "pid": std::process::id(),
        })),
    };
    match request_local(home_dir, &heartbeat_request, SAME_HOST_REQUEST_TIMEOUT) {
        Ok(_) => Ok(()),
        Err(error) if should_retry_autostart(&error) => {
            auto_start_daemon(home_dir)?;
            request_local(home_dir, &heartbeat_request, SAME_HOST_REQUEST_TIMEOUT)
                .map(|_| ())
                .map_err(add_daemon_ready_recovery)
        }
        Err(error) => Err(error),
    }
}

pub fn request_local(
    home_dir: &Path,
    request: &DaemonRequest,
    timeout: Duration,
) -> Result<DaemonResponse, AtmError> {
    let state = read_control_state(home_dir)?;
    match state.local_endpoint {
        #[cfg(unix)]
        LocalEndpoint::UnixSocket(path) => {
            let mut stream = UnixStream::connect(&path).map_err(|error| {
                AtmError::daemon_unavailable(format!(
                    "failed to connect to local ATM daemon socket {}: {error}",
                    path.display()
                ))
                .with_source(error)
            })?;
            stream.set_read_timeout(Some(timeout)).map_err(|error| {
                AtmError::daemon_request_timeout("failed to set local daemon read timeout")
                    .with_source(error)
            })?;
            stream.set_write_timeout(Some(timeout)).map_err(|error| {
                AtmError::daemon_request_timeout("failed to set local daemon write timeout")
                    .with_source(error)
            })?;
            exchange_unix(&mut stream, request)
        }
        LocalEndpoint::TcpLoopback(addr) => {
            let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(|error| {
                AtmError::daemon_unavailable(format!(
                    "failed to connect to local ATM daemon loopback endpoint {addr}: {error}"
                ))
                .with_source(error)
            })?;
            stream.set_read_timeout(Some(timeout)).map_err(|error| {
                AtmError::daemon_request_timeout("failed to set local daemon read timeout")
                    .with_source(error)
            })?;
            stream.set_write_timeout(Some(timeout)).map_err(|error| {
                AtmError::daemon_request_timeout("failed to set local daemon write timeout")
                    .with_source(error)
            })?;
            exchange_tcp(&mut stream, request)
        }
    }
}

pub fn request_remote(
    address: SocketAddr,
    request: &DaemonRequest,
    retry_budget: Duration,
) -> Result<DaemonResponse, AtmError> {
    let deadline = Instant::now() + retry_budget;
    let mut last_error = None;
    while Instant::now() < deadline {
        match TcpStream::connect_timeout(&address, REMOTE_CONNECT_TIMEOUT) {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(REMOTE_IO_TIMEOUT))
                    .map_err(|error| {
                        AtmError::daemon_request_timeout("failed to set remote read timeout")
                            .with_source(error)
                    })?;
                stream
                    .set_write_timeout(Some(REMOTE_IO_TIMEOUT))
                    .map_err(|error| {
                        AtmError::daemon_request_timeout("failed to set remote write timeout")
                            .with_source(error)
                    })?;
                return exchange_tcp(&mut stream, request);
            }
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    Err(AtmError::daemon_remote_unavailable(format!(
        "remote daemon at {address} did not accept the request within {:?}",
        retry_budget
    ))
    .with_source(last_error.expect("remote retry captured at least one error")))
}

pub fn auto_start_daemon(home_dir: &Path) -> Result<(), AtmError> {
    let start_command = daemon_start_command();
    let mut command = Command::new(&start_command.program);
    command
        .args(&start_command.args)
        .env("ATM_HOME", home_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(current_dir) = &start_command.current_dir {
        command.current_dir(current_dir);
    }
    let mut child = command.spawn().map_err(|error| {
        AtmError::daemon_start_failed(format!(
            "failed to start atm-daemon with {}: {error}",
            start_command.display()
        ))
        .with_source(error)
    })?;

    let wait_deadline = Instant::now() + AUTO_START_PUBLISH_TIMEOUT;
    while Instant::now() < wait_deadline {
        if read_control_state(home_dir).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait().map_err(|error| {
            AtmError::daemon_start_failed("failed to monitor atm-daemon startup").with_source(error)
        })? {
            return Err(AtmError::daemon_start_failed(format!(
                "atm-daemon exited before publishing control state with status {status}"
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    Err(AtmError::daemon_start_failed(format!(
        "atm-daemon did not publish its control state within {:?}",
        AUTO_START_PUBLISH_TIMEOUT
    )))
}

fn resolve_request_identity(
    team_override: Option<TeamName>,
    current_dir: Option<&Path>,
    allow_anonymous_identity: bool,
) -> Result<(TeamName, AgentName), AtmError> {
    let team_name = team_override
        .or_else(|| {
            std::env::var("ATM_TEAM")
                .ok()
                .and_then(|value| value.parse().ok())
        })
        .or_else(|| current_dir.and_then(default_team_from_workspace_config))
        .ok_or_else(AtmError::team_unavailable)?;
    let agent_name = std::env::var("ATM_IDENTITY")
        .ok()
        .and_then(|value| value.parse().ok())
        .or_else(|| {
            allow_anonymous_identity.then(|| {
                "atm-doctor"
                    .parse()
                    .expect("atm-doctor is a valid agent name")
            })
        })
        .ok_or_else(AtmError::identity_unavailable)?;
    Ok((team_name, agent_name))
}

fn default_team_from_workspace_config(start_dir: &Path) -> Option<TeamName> {
    let mut current = Some(start_dir);
    while let Some(dir) = current {
        let candidate = dir.join(".atm.toml");
        if candidate.is_file() {
            let raw = fs::read_to_string(candidate).ok()?;
            let parsed: toml::Value = toml::from_str(&raw).ok()?;
            return parsed
                .get("atm")
                .and_then(|atm| atm.get("default_team"))
                .or_else(|| parsed.get("default_team"))
                .and_then(toml::Value::as_str)
                .and_then(|value| value.parse().ok());
        }
        current = dir.parent();
    }
    None
}

fn should_retry_autostart(error: &AtmError) -> bool {
    matches!(error.code, Code::DaemonUnavailable)
}

fn add_daemon_ready_recovery(error: AtmError) -> AtmError {
    error.with_recovery(
        "The ATM daemon started but did not become ready in time. Check atm-daemon logs and retry the command.",
    )
}

fn default_daemon_binary() -> Option<PathBuf> {
    if let Ok(current) = std::env::current_exe() {
        if current
            .components()
            .any(|component| component.as_os_str() == "target")
        {
            return None;
        }
        let dir = current.parent()?;
        let sibling = dir.join("atm-daemon");
        if sibling.exists() {
            return Some(sibling);
        }
        if let Some(parent) = dir.parent() {
            let sibling = parent.join("atm-daemon");
            if sibling.exists() {
                return Some(sibling);
            }
        }
    }
    None
}

struct StartCommand {
    program: PathBuf,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
}

impl StartCommand {
    fn display(&self) -> String {
        let mut parts = vec![self.program.display().to_string()];
        parts.extend(self.args.iter().cloned());
        parts.join(" ")
    }
}

fn daemon_start_command() -> StartCommand {
    if let Some(program) = std::env::var_os("ATM_DAEMON_BIN").map(PathBuf::from) {
        return StartCommand {
            program,
            args: Vec::new(),
            current_dir: None,
        };
    }
    if let Some(program) = default_daemon_binary() {
        return StartCommand {
            program,
            args: Vec::new(),
            current_dir: None,
        };
    }
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf();
    StartCommand {
        // TODO(phase-q): resolve the cargo path via a pinned install/service launcher rather than PATH lookup.
        program: PathBuf::from("cargo"),
        args: vec![
            "run".to_string(),
            "--quiet".to_string(),
            "-p".to_string(),
            "atm-daemon".to_string(),
            "--bin".to_string(),
            "atm-daemon".to_string(),
        ],
        current_dir: Some(workspace_root),
    }
}

pub(crate) fn dispatch_error_to_atm(error: DispatchError) -> AtmError {
    match error {
        DispatchError::Store(error) => AtmError::mailbox_read(error.to_string()),
        DispatchError::PayloadDecode(message) => AtmError::daemon_protocol(message),
        DispatchError::Handler(message) => AtmError::mailbox_read(message),
        DispatchError::ResponseEncode(message) => AtmError::mailbox_write(message),
        DispatchError::Unsupported(kind) => AtmError::daemon_protocol(format!(
            "request kind {kind:?} is not implemented by the daemon"
        )),
    }
}

pub(crate) fn read_control_state(home_dir: &Path) -> Result<ControlState, AtmError> {
    let paths = DaemonPaths::from_home(home_dir);
    let raw = fs::read(&paths.control_path).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "daemon control state is unavailable at {}: {error}",
            paths.control_path.display()
        ))
        .with_source(error)
    })?;
    serde_json::from_slice(&raw).map_err(|error| {
        AtmError::daemon_protocol("failed to parse daemon control state").with_source(error)
    })
}

pub(crate) fn write_control_state(path: &Path, state: &ControlState) -> Result<(), AtmError> {
    let bytes = serde_json::to_vec(state).map_err(|error| {
        AtmError::daemon_protocol("failed to serialize daemon control state").with_source(error)
    })?;
    atomic_write_control_state(path, &bytes)
}

#[cfg(unix)]
fn exchange_unix(
    stream: &mut UnixStream,
    request: &DaemonRequest,
) -> Result<DaemonResponse, AtmError> {
    write_frame(stream, request)?;
    let envelope: WireResponseEnvelope = read_frame(stream)?;
    response_from_envelope(envelope)
}

fn exchange_tcp(
    stream: &mut TcpStream,
    request: &DaemonRequest,
) -> Result<DaemonResponse, AtmError> {
    write_frame(stream, request)?;
    let envelope: WireResponseEnvelope = read_frame(stream)?;
    response_from_envelope(envelope)
}

fn response_from_envelope(envelope: WireResponseEnvelope) -> Result<DaemonResponse, AtmError> {
    if let Some(response) = envelope.response {
        return Ok(response);
    }
    let error = envelope
        .error
        .ok_or_else(|| AtmError::daemon_protocol("daemon response envelope was empty"))?;
    Err(wire_error_to_atm(error))
}

pub(crate) fn write_frame<T: Serialize, W: Write>(
    writer: &mut W,
    value: &T,
) -> Result<(), AtmError> {
    let mut writer = BufWriter::new(writer);
    serde_json::to_writer(&mut writer, value).map_err(|error| {
        AtmError::daemon_protocol("failed to serialize daemon frame").with_source(error)
    })?;
    writer.write_all(b"\n").map_err(|error| {
        AtmError::daemon_protocol("failed to write daemon frame delimiter").with_source(error)
    })?;
    writer.flush().map_err(|error| {
        AtmError::daemon_protocol("failed to flush daemon frame").with_source(error)
    })
}

pub(crate) fn read_frame<T: for<'de> Deserialize<'de>, R: std::io::Read>(
    reader: &mut R,
) -> Result<T, AtmError> {
    let mut reader = BufReader::new(reader);
    let mut frame = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let bytes = reader.read(&mut byte).map_err(|error| {
            AtmError::daemon_protocol("failed to read daemon frame").with_source(error)
        })?;
        if bytes == 0 {
            break;
        }
        frame.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
        if frame.len() > MAX_FRAME_BYTES {
            return Err(AtmError::daemon_protocol(format!(
                "daemon frame exceeded the {} byte safety limit",
                MAX_FRAME_BYTES
            )));
        }
    }
    if frame.is_empty() {
        return Err(AtmError::daemon_protocol(
            "daemon connection closed before a response frame was received",
        ));
    }
    if frame.last() == Some(&b'\n') {
        frame.pop();
    }
    serde_json::from_slice(&frame).map_err(|error| {
        AtmError::daemon_protocol("failed to decode daemon frame").with_source(error)
    })
}

fn atomic_write_control_state(path: &Path, bytes: &[u8]) -> Result<(), AtmError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AtmError::daemon_start_failed(format!(
                "failed to create daemon state directory {}: {error}",
                parent.display()
            ))
            .with_source(error)
        })?;
    }

    let temp_path = path.with_file_name(format!(
        ".{}.tmp.{}.json",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("control"),
        Uuid::new_v4()
    ));

    {
        let mut file = fs::File::create(&temp_path).map_err(|error| {
            AtmError::daemon_start_failed(format!(
                "failed to create daemon control temp file {}: {error}",
                temp_path.display()
            ))
            .with_source(error)
        })?;
        file.write_all(bytes).map_err(|error| {
            AtmError::daemon_start_failed(format!(
                "failed to write daemon control temp file {}: {error}",
                temp_path.display()
            ))
            .with_source(error)
        })?;
        file.sync_all().map_err(|error| {
            AtmError::daemon_start_failed(format!(
                "failed to sync daemon control temp file {}: {error}",
                temp_path.display()
            ))
            .with_source(error)
        })?;
    }

    fs::rename(&temp_path, path).map_err(|error| {
        AtmError::daemon_start_failed(format!(
            "failed to replace daemon control state {}: {error}",
            path.display()
        ))
        .with_source(error)
    })?;

    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        let directory = fs::File::open(parent).map_err(|error| {
            AtmError::daemon_start_failed(format!(
                "failed to open daemon state directory {} for sync: {error}",
                parent.display()
            ))
            .with_source(error)
        })?;
        directory.sync_all().map_err(|error| {
            AtmError::daemon_start_failed(format!(
                "failed to sync daemon state directory {}: {error}",
                parent.display()
            ))
            .with_source(error)
        })?;
    }

    Ok(())
}

fn wire_error_to_atm(error: WireError) -> AtmError {
    let recovery = error
        .recovery
        .unwrap_or_else(|| "Restart the daemon and retry the command.".to_string());
    match error.code {
        Code::ConfigHomeUnavailable
        | Code::ConfigParseFailed
        | Code::ConfigRetiredHookMembersKey
        | Code::ConfigRetiredLegacyHookKeys
        | Code::ConfigTeamParseFailed
        | Code::ConfigTeamMissing
        | Code::IdentityUnavailable
        | Code::AddressParseFailed
        | Code::TeamUnavailable
        | Code::TeamNotFound
        | Code::AgentNotFound
        | Code::MailboxReadFailed
        | Code::MailboxWriteFailed
        | Code::MailboxLockFailed
        | Code::MailboxLockReadOnlyFilesystem
        | Code::MailboxLockTimeout
        | Code::MessageValidationFailed
        | Code::SerializationFailed
        | Code::FilePolicyRejected
        | Code::FileReferenceRewriteFailed
        | Code::WaitTimeout
        | Code::AckInvalidState
        | Code::ClearInvalidState
        | Code::ObservabilityEmitFailed
        | Code::ObservabilityQueryFailed
        | Code::ObservabilityFollowFailed
        | Code::ObservabilityHealthFailed
        | Code::ObservabilityBootstrapFailed
        | Code::StoreOpenFailed
        | Code::StoreBootstrapFailed
        | Code::StoreMigrationFailed
        | Code::StoreQueryFailed
        | Code::StoreBusy
        | Code::StoreConstraintViolation
        | Code::StoreTransactionFailed
        | Code::ObservabilityHealthOk
        | Code::WarningInvalidTeamMemberSkipped
        | Code::WarningMailboxRecordSkipped
        | Code::WarningMalformedAtmFieldIgnored
        | Code::WarningObservabilityHealthDegraded
        | Code::WarningOriginInboxEntrySkipped
        | Code::WarningMissingTeamConfigFallback
        | Code::WarningSendAlertStateDegraded
        | Code::WarningIdentityDrift
        | Code::WarningBaselineMemberMissing
        | Code::WarningRestoreInProgress
        | Code::WarningStaleMailboxLock
        | Code::WarningHookSkipped
        | Code::WarningHookExecutionFailed => {
            AtmError::daemon_protocol(format!("{}: {}", error.code, error.message))
                .with_recovery(recovery)
        }
        Code::DaemonUnavailable => {
            AtmError::daemon_unavailable(error.message).with_recovery(recovery)
        }
        Code::DaemonStartFailed => {
            AtmError::daemon_start_failed(error.message).with_recovery(recovery)
        }
        Code::DaemonAlreadyRunning => {
            AtmError::daemon_already_running(error.message).with_recovery(recovery)
        }
        Code::DaemonRequestTimeout => {
            AtmError::daemon_request_timeout(error.message).with_recovery(recovery)
        }
        Code::DaemonProtocolFailed => {
            AtmError::daemon_protocol(error.message).with_recovery(recovery)
        }
        Code::DaemonRemoteUnavailable => {
            AtmError::daemon_remote_unavailable(error.message).with_recovery(recovery)
        }
    }
}
