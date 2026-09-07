//! Private, transport-neutral Herdr request and response contract.

use std::path::{Path, PathBuf};
use std::time::Duration;

use atm_core::doctor::{
    HerdrEndpointDisplay, HerdrEndpointDisplayRoot, HerdrTransportKind, HerdrVersion,
};
use atm_core::HerdrAgentName;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::{HerdrSession, RequestDeadline};
use serde_json::Value;

use crate::transport_cli::CliIo;
use crate::transport_socket::SocketIo;
use crate::{
    AgentSnapshot, HerdrAgentStatus, HerdrError, HerdrListOutcome, HerdrPromptOutcome,
    HerdrWaitOutcome,
};

/// Validated Herdr client configuration. Construction is pure and performs no
/// endpoint or filesystem I/O.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HerdrClientConfig {
    transport: HerdrTransportKind,
    binary_path: Option<PathBuf>,
    socket_path: Option<PathBuf>,
}

impl Default for HerdrClientConfig {
    fn default() -> Self {
        Self {
            transport: HerdrTransportKind::Socket,
            binary_path: None,
            socket_path: None,
        }
    }
}

impl HerdrClientConfig {
    pub fn try_new(
        transport: HerdrTransportKind,
        binary_path: Option<PathBuf>,
        socket_path: Option<PathBuf>,
    ) -> Result<Self, AtmError> {
        for (key, path) in [
            ("binary_path", binary_path.as_ref()),
            ("socket_path", socket_path.as_ref()),
        ] {
            if let Some(path) = path.filter(|path| !path.is_absolute()) {
                return Err(AtmError::new(
                    AtmErrorCode::ConfigParseFailed,
                    format!("[herdr] {key} must be absolute: {}", path.display()),
                ));
            }
        }
        Ok(Self {
            transport,
            binary_path,
            socket_path,
        })
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn with_socket_path(socket_path: PathBuf) -> Self {
        Self {
            socket_path: Some(socket_path),
            ..Self::default()
        }
    }

    pub fn binary_path(&self) -> Option<&Path> {
        self.binary_path.as_deref()
    }

    pub fn transport(&self) -> &HerdrTransportKind {
        &self.transport
    }

    pub fn socket_path(&self) -> Option<&Path> {
        self.socket_path.as_deref()
    }
}

/// One request the private Herdr transport can carry.
pub(crate) enum HerdrOp<'a> {
    Prompt {
        agent: &'a HerdrAgentName,
        text: &'a str,
    },
    Wait {
        agent: &'a HerdrAgentName,
        until: &'a [HerdrAgentStatus],
        timeout: Duration,
    },
    Get {
        agent: &'a HerdrAgentName,
    },
    List,
    StatusServer,
    Notify {
        title: &'a str,
        body: &'a str,
    },
}

pub(crate) struct HerdrServerStatus {
    pub(crate) version: HerdrVersion,
    pub(crate) protocol: u32,
    pub(crate) live_handoff: bool,
}

/// Transport response before it is decoded into a public domain outcome.
pub(crate) struct HerdrEnvelope {
    pub result: Option<Value>,
    pub error: Option<HerdrErrorEnvelope>,
}

/// Structured Herdr error data carried independently of a transport.
pub(crate) struct HerdrErrorEnvelope {
    pub code: String,
    pub message: String,
    pub retry_after_ms: Option<u64>,
}

/// Private transport selection. AY.9 owns the sole production factory; this
/// enum never crosses the atm-herdr crate boundary.
#[derive(Clone, Debug)]
pub(crate) enum HerdrIo {
    Cli(CliIo),
    Socket(SocketIo),
}

impl Default for HerdrIo {
    fn default() -> Self {
        Self::from_config(&HerdrClientConfig::default())
    }
}

impl HerdrIo {
    pub(crate) fn from_config(config: &HerdrClientConfig) -> Self {
        match config.transport() {
            HerdrTransportKind::Cli => Self::Cli(CliIo::new(config)),
            HerdrTransportKind::Socket => Self::Socket(SocketIo::new(config)),
        }
    }

    /// Returns only core's sanitized endpoint DTO; the raw endpoint stays in
    /// the private socket transport.
    pub(crate) fn endpoint_display(
        &self,
        session: Option<&HerdrSession>,
    ) -> Option<HerdrEndpointDisplay> {
        match self {
            Self::Cli(_) => None,
            Self::Socket(socket) => socket_endpoint_display(&socket.cfg, session, &socket.env),
        }
    }

    pub(crate) async fn call(
        &self,
        op: HerdrOp<'_>,
        session: Option<&HerdrSession>,
        deadline: RequestDeadline,
    ) -> Result<HerdrEnvelope, HerdrError> {
        match self {
            Self::Cli(cli) => cli.call(op, session, deadline).await,
            Self::Socket(socket) => socket.call(op, session, deadline).await,
        }
    }
}

fn socket_endpoint_display(
    config: &HerdrClientConfig,
    session: Option<&HerdrSession>,
    env: &crate::transport_socket::HerdrHostEnv,
) -> Option<HerdrEndpointDisplay> {
    use crate::transport_socket::{HerdrEndpoint, herdr_api_endpoint};

    match herdr_api_endpoint(config, session, env) {
        HerdrEndpoint::UnixSocket(path) => {
            if config.socket_path().is_some() {
                configured_endpoint_display(&path, false)
            } else if let Some(relative) = env
                .xdg_config_home
                .as_deref()
                .and_then(|root| path.strip_prefix(root).ok())
            {
                HerdrEndpointDisplay::from_relative(
                    HerdrEndpointDisplayRoot::XdgConfigHome,
                    relative,
                    false,
                )
                .ok()
            } else if let Some(relative) = env
                .home
                .as_deref()
                .and_then(|root| path.strip_prefix(root).ok())
            {
                HerdrEndpointDisplay::from_relative(HerdrEndpointDisplayRoot::Home, relative, false)
                    .ok()
            } else {
                configured_endpoint_display(&path, false)
            }
        }
        HerdrEndpoint::NamedPipe(path) => {
            let raw = path.strip_prefix(r"\\.\pipe\").unwrap_or(&path);
            if config.socket_path().is_some() {
                configured_named_pipe_display(raw)
            } else if let Some(relative) = env
                .appdata
                .as_deref()
                .and_then(|root| raw.strip_prefix(&format!(r"{}\", root.display())))
            {
                named_pipe_display(HerdrEndpointDisplayRoot::AppData, relative)
            } else if let Some(relative) = env
                .home
                .as_deref()
                .and_then(|root| raw.strip_prefix(&format!(r"{}\", root.display())))
            {
                named_pipe_display(HerdrEndpointDisplayRoot::Home, relative)
            } else {
                configured_named_pipe_display(raw)
            }
        }
    }
}

fn configured_endpoint_display(path: &Path, named_pipe: bool) -> Option<HerdrEndpointDisplay> {
    let filename = path.file_name()?;
    HerdrEndpointDisplay::from_relative(
        HerdrEndpointDisplayRoot::Configured,
        Path::new(filename),
        named_pipe,
    )
    .ok()
}

fn configured_named_pipe_display(path: &str) -> Option<HerdrEndpointDisplay> {
    configured_endpoint_display(Path::new(path), true)
}

fn named_pipe_display(
    root: HerdrEndpointDisplayRoot,
    relative: &str,
) -> Option<HerdrEndpointDisplay> {
    HerdrEndpointDisplay::from_relative(root, Path::new(&relative.replace('\\', "/")), true).ok()
}

pub(crate) fn prompt_from_envelope(
    envelope: HerdrEnvelope,
) -> Result<HerdrPromptOutcome, HerdrError> {
    let error = error_from_envelope(&envelope);
    let result = envelope.result.ok_or(error)?;
    let snapshot = result
        .get("agent")
        .map(snapshot_from_value)
        .transpose()?
        .unwrap_or(AgentSnapshot {
            name: None,
            status: HerdrAgentStatus::Unknown,
            workspace_id: None,
        });
    Ok(HerdrPromptOutcome::Accepted(snapshot))
}

pub(crate) fn snapshot_from_envelope(
    envelope: HerdrEnvelope,
) -> Result<HerdrWaitOutcome, HerdrError> {
    let error = error_from_envelope(&envelope);
    let result = envelope.result.ok_or(error)?;
    Ok(HerdrWaitOutcome {
        snapshot: snapshot_from_value(result.get("agent").unwrap_or(&result))?,
    })
}

pub(crate) fn get_from_envelope(envelope: HerdrEnvelope) -> Result<AgentSnapshot, HerdrError> {
    let error = error_from_envelope(&envelope);
    let result = envelope.result.ok_or(error)?;
    snapshot_from_value(result.get("agent").unwrap_or(&result))
}

pub(crate) fn list_from_envelope(envelope: HerdrEnvelope) -> Result<HerdrListOutcome, HerdrError> {
    let error = error_from_envelope(&envelope);
    let result = envelope.result.ok_or(error)?;
    let agents = result
        .get("agents")
        .and_then(Value::as_array)
        .ok_or_else(|| protocol_mismatch("response did not contain an agents array"))?
        .iter()
        .map(snapshot_from_value)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(HerdrListOutcome { agents })
}

pub(crate) fn server_status_from_envelope(
    envelope: HerdrEnvelope,
) -> Result<HerdrServerStatus, HerdrError> {
    let error = error_from_envelope(&envelope);
    let result = envelope.result.ok_or(error)?;
    let version = result
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| protocol_mismatch("response did not contain a version"))
        .and_then(|version| {
            HerdrVersion::parse(version).map_err(|error| protocol_mismatch(error.to_string()))
        })?;
    let protocol = result
        .get("protocol")
        .and_then(Value::as_u64)
        .and_then(|protocol| u32::try_from(protocol).ok())
        .ok_or_else(|| protocol_mismatch("response did not contain a protocol number"))?;
    let live_handoff = result
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|capabilities| {
            capabilities
                .iter()
                .any(|capability| capability.as_str() == Some("live_handoff"))
        })
        .unwrap_or(false);
    Ok(HerdrServerStatus {
        version,
        protocol,
        live_handoff,
    })
}

pub(crate) fn unit_from_envelope(envelope: HerdrEnvelope) -> Result<(), HerdrError> {
    if envelope.result.is_some() {
        Ok(())
    } else {
        Err(error_from_envelope(&envelope))
    }
}

fn snapshot_from_value(value: &Value) -> Result<AgentSnapshot, HerdrError> {
    let status = value
        .get("agent_status")
        .or_else(|| value.get("status"))
        .and_then(Value::as_str)
        .ok_or_else(|| protocol_mismatch("agent response did not contain a status"))?;
    Ok(AgentSnapshot {
        name: value.get("name").and_then(Value::as_str).map(str::to_owned),
        status: parse_status(status),
        workspace_id: value
            .get("workspace_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn parse_status(status: &str) -> HerdrAgentStatus {
    match status {
        "idle" => HerdrAgentStatus::Idle,
        "working" => HerdrAgentStatus::Working,
        "blocked" => HerdrAgentStatus::Blocked,
        "done" => HerdrAgentStatus::Done,
        _ => HerdrAgentStatus::Unknown,
    }
}

fn error_from_envelope(envelope: &HerdrEnvelope) -> HerdrError {
    let Some(error) = &envelope.error else {
        return protocol_mismatch("response contained neither a result nor an error");
    };
    let message = error.message.clone();
    let retry_after = error.retry_after_ms.map(Duration::from_millis);
    match error.code.as_str() {
        "agent_blocked" => HerdrError::AgentBlocked,
        "agent_not_found" => HerdrError::AgentNotFound,
        "agent_not_ready" => HerdrError::AgentNotReady,
        "agent_target_ambiguous" => HerdrError::AgentTargetAmbiguous,
        "agent_not_running" => HerdrError::AgentNotRunning,
        "agent_prompt_stalled" => HerdrError::AgentPromptStalled,
        "server_not_running" => HerdrError::ServerNotRunning,
        "protocol_mismatch" => protocol_mismatch(&message),
        "timeout" => HerdrError::Timeout,
        "invalid_agent_name" => HerdrError::InvalidAgentName,
        "empty_agent_prompt" => HerdrError::EmptyAgentPrompt,
        "server_unavailable" => HerdrError::ServerUnavailable {
            message,
            retry_after,
            io_error_kind: None,
        },
        "internal_error" | "agent_prompt_failed" => HerdrError::InternalError { message },
        other => HerdrError::Advisory {
            code: other.to_owned(),
            message,
        },
    }
}

fn protocol_mismatch(message: impl Into<String>) -> HerdrError {
    HerdrError::ProtocolMismatch {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{HerdrClientConfig, HerdrIo, socket_endpoint_display};
    use atm_core::doctor::HerdrTransportKind;
    use std::path::PathBuf;

    use crate::transport_socket::{HerdrHostEnv, Platform};

    #[test]
    fn production_factory_selects_each_closed_transport_once() {
        let socket = HerdrClientConfig::default();
        let cli = HerdrClientConfig::try_new(HerdrTransportKind::Cli, None, None)
            .expect("CLI is a supported explicit selection");

        assert!(matches!(HerdrIo::from_config(&socket), HerdrIo::Socket(_)));
        assert!(matches!(HerdrIo::from_config(&cli), HerdrIo::Cli(_)));
    }

    #[test]
    fn socket_endpoint_display_redacts_xdg_root_before_doctor_projection() {
        let endpoint = socket_endpoint_display(
            &HerdrClientConfig::default(),
            None,
            &HerdrHostEnv {
                xdg_config_home: Some(PathBuf::from("/private/atlas/.config")),
                appdata: None,
                home: Some(PathBuf::from("/private/atlas")),
                platform: Platform::Unix,
            },
        )
        .expect("default socket endpoint has a symbolic display");

        assert_eq!(endpoint.as_str(), "$XDG_CONFIG_HOME/herdr/herdr.sock");
        assert!(!endpoint.as_str().contains("atlas"));
    }

    #[test]
    fn named_pipe_display_redacts_appdata_before_doctor_projection() {
        let endpoint = socket_endpoint_display(
            &HerdrClientConfig::default(),
            None,
            &HerdrHostEnv {
                xdg_config_home: None,
                appdata: Some(PathBuf::from(r"C:\Users\atlas\AppData\Roaming")),
                home: Some(PathBuf::from(r"C:\Users\atlas")),
                platform: Platform::Windows,
            },
        )
        .expect("default named-pipe endpoint has a symbolic display");

        assert_eq!(endpoint.as_str(), r"\\.\pipe\%APPDATA%/herdr/herdr.sock");
        assert!(!endpoint.as_str().contains("atlas"));
    }
}
