//! Private, transport-neutral Herdr request and response contract.

use std::path::{Path, PathBuf};
use std::time::Duration;

use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::types::AgentName;
use atm_core::{HerdrSession, RequestDeadline};
use serde_json::Value;

use crate::transport_cli::CliIo;
use crate::{
    AgentSnapshot, HerdrAgentStatus, HerdrError, HerdrListOutcome, HerdrPromptOutcome,
    HerdrWaitOutcome,
};

/// Validated Herdr client configuration. Construction is pure and performs no
/// endpoint or filesystem I/O.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HerdrClientConfig {
    binary_path: Option<PathBuf>,
    socket_path: Option<PathBuf>,
}

impl HerdrClientConfig {
    pub fn try_new(
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
            binary_path,
            socket_path,
        })
    }

    pub fn binary_path(&self) -> Option<&Path> {
        self.binary_path.as_deref()
    }

    pub fn socket_path(&self) -> Option<&Path> {
        self.socket_path.as_deref()
    }
}

/// One request the private Herdr transport can carry.
pub(crate) enum HerdrOp<'a> {
    Prompt {
        agent: &'a AgentName,
        text: &'a str,
    },
    Wait {
        agent: &'a AgentName,
        until: &'a [HerdrAgentStatus],
        timeout: Duration,
    },
    Get {
        agent: &'a AgentName,
    },
    List,
    Notify {
        title: &'a str,
        body: &'a str,
    },
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

/// Private transport selection. AY.2 intentionally constructs only CLI.
#[derive(Clone, Debug)]
pub(crate) enum HerdrIo {
    Cli(CliIo),
}

impl Default for HerdrIo {
    fn default() -> Self {
        Self::Cli(CliIo::new(&HerdrClientConfig::default()))
    }
}

impl HerdrIo {
    pub(crate) fn from_config(config: &HerdrClientConfig) -> Self {
        Self::Cli(CliIo::new(config))
    }

    pub(crate) async fn call(
        &self,
        op: HerdrOp<'_>,
        session: Option<&HerdrSession>,
        deadline: RequestDeadline,
    ) -> Result<HerdrEnvelope, HerdrError> {
        match self {
            Self::Cli(cli) => cli.call(op, session, deadline).await,
        }
    }
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
        .ok_or(HerdrError::ProtocolMismatch)?
        .iter()
        .map(snapshot_from_value)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(HerdrListOutcome { agents })
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
        .ok_or(HerdrError::ProtocolMismatch)?;
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
        return HerdrError::ProtocolMismatch;
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
        "protocol_mismatch" => HerdrError::ProtocolMismatch,
        "timeout" => HerdrError::Timeout,
        "invalid_agent_name" => HerdrError::InvalidAgentName,
        "empty_agent_prompt" => HerdrError::EmptyAgentPrompt,
        "server_unavailable" => HerdrError::ServerUnavailable {
            message,
            retry_after,
        },
        "internal_error" | "agent_prompt_failed" => HerdrError::InternalError { message },
        other => HerdrError::Advisory {
            code: other.to_owned(),
            message,
        },
    }
}
