//! Tokio-native, shell-free process boundary for the Herdr CLI.

use std::future::Future;
use std::pin::Pin;
#[cfg(any())]
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::types::AgentName;
use atm_core::{HerdrSession, RequestDeadline};
#[cfg(any())]
use serde_json::Value;
#[cfg(any())]
use tokio::io::AsyncReadExt;

mod transport;
mod transport_cli;

use transport::{
    HerdrIo, HerdrOp, get_from_envelope, list_from_envelope, prompt_from_envelope,
    snapshot_from_envelope, unit_from_envelope,
};

/// Oldest Herdr release this daemon supports (ADR-061). Keyed on the Herdr
/// release version reported by `ping.version`, not on Herdr's bincode-only
/// `PROTOCOL_VERSION`. Every Herdr release at or above this value must keep
/// working from one daemon build; raising it is a breaking change that needs
/// Rand's recorded approval and sign-off. Set to 0.8.0 by Rand on 2026-09-05
/// from the M5 drift review of v0.8.0..v0.8.2.
pub const HERDR_MINIMUM_VERSION: &str = "0.8.0";
pub(crate) const HERDR_PROCESS_CAP: Duration = Duration::from_secs(5);
const BREAKER_MAX_BACKOFF: Duration = Duration::from_secs(30);
pub(crate) const HERDR_MAX_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HerdrAgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

impl HerdrAgentStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSnapshot {
    pub name: Option<String>,
    pub status: HerdrAgentStatus,
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HerdrPromptOutcome {
    Accepted(AgentSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrWaitOutcome {
    pub snapshot: AgentSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrGetOutcome {
    pub snapshot: AgentSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrListOutcome {
    pub agents: Vec<AgentSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerPolicy {
    Shared,
    Bypass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HerdrError {
    AgentBlocked,
    AgentNotFound,
    AgentNotReady,
    AgentTargetAmbiguous,
    AgentNotRunning,
    AgentPromptStalled,
    ServerNotRunning,
    ProtocolMismatch,
    Timeout,
    InvalidAgentName,
    EmptyAgentPrompt,
    ServerUnavailable,
    InternalError,
    TimedOut,
    Unavailable { retry_after: Duration },
    Advisory { code: String },
}

impl From<HerdrError> for AtmError {
    fn from(error: HerdrError) -> Self {
        let (code, message) = match error {
            HerdrError::AgentBlocked => (
                AtmErrorCode::PostSendHerdrPromptFailed,
                "Herdr agent is blocked".to_owned(),
            ),
            HerdrError::AgentNotFound => (
                AtmErrorCode::HerdrAgentNotVisible,
                "Herdr agent was not found".to_owned(),
            ),
            HerdrError::AgentNotReady => (
                AtmErrorCode::HerdrAgentNotVisible,
                "Herdr agent is not ready".to_owned(),
            ),
            HerdrError::AgentTargetAmbiguous => (
                AtmErrorCode::HerdrAgentNotVisible,
                "Herdr agent target is ambiguous".to_owned(),
            ),
            HerdrError::AgentNotRunning => (
                AtmErrorCode::HerdrAgentNotVisible,
                "Herdr agent is not running".to_owned(),
            ),
            HerdrError::AgentPromptStalled => (
                AtmErrorCode::HerdrPromptFailed,
                "Herdr prompt stalled".to_owned(),
            ),
            HerdrError::ServerNotRunning
            | HerdrError::ProtocolMismatch
            | HerdrError::ServerUnavailable
            | HerdrError::TimedOut
            | HerdrError::Timeout => (
                AtmErrorCode::HerdrUnavailable,
                "Herdr server is unavailable".to_owned(),
            ),
            HerdrError::InvalidAgentName => (
                AtmErrorCode::HerdrPromptFailed,
                "Herdr agent name is invalid".to_owned(),
            ),
            HerdrError::EmptyAgentPrompt => (
                AtmErrorCode::HerdrPromptFailed,
                "Herdr prompt is empty".to_owned(),
            ),
            HerdrError::InternalError => (
                AtmErrorCode::HerdrPromptFailed,
                "Herdr returned an internal error".to_owned(),
            ),
            HerdrError::Unavailable { retry_after } => (
                AtmErrorCode::HerdrUnavailable,
                format!("Herdr process breaker is open; retry after {retry_after:?}"),
            ),
            HerdrError::Advisory { code } => (
                AtmErrorCode::HerdrPromptFailed,
                format!("Herdr command failed with {code}"),
            ),
        };
        AtmError::new(code, message)
    }
}

impl HerdrError {
    /// Stable backend-facing outcome classification. Wire error-code strings
    /// remain private to this crate.
    #[must_use]
    pub fn emission_outcome(&self) -> &'static str {
        match self {
            Self::AgentBlocked => "blocked_before_input",
            Self::AgentNotFound | Self::AgentNotRunning | Self::AgentTargetAmbiguous => {
                "target_not_present"
            }
            Self::AgentNotReady => "not_ready",
            Self::AgentPromptStalled => "prompt_stalled",
            Self::ServerNotRunning | Self::ServerUnavailable => "server_outage",
            Self::ProtocolMismatch => "protocol_incompatible",
            Self::Timeout | Self::TimedOut => "timed_out",
            Self::InvalidAgentName => "invalid_target",
            Self::EmptyAgentPrompt => "invalid_prompt",
            Self::InternalError => "internal_failure",
            Self::Unavailable { .. } => "breaker_unavailable",
            Self::Advisory { .. } => "advisory_failure",
        }
    }

    pub fn is_infrastructure(&self) -> bool {
        matches!(
            self,
            Self::ServerNotRunning
                | Self::ProtocolMismatch
                | Self::ServerUnavailable
                | Self::TimedOut
        )
    }
}

/// The only cross-crate Herdr process contract. Consumers provide the
/// external deadline and receive typed outcomes without knowing Herdr's wire
/// format or argv.
pub trait HerdrProcessAdapter: Send + Sync {
    fn prompt<'a>(
        &'a self,
        agent: &'a AgentName,
        session: Option<&'a HerdrSession>,
        text: &'a str,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrPromptOutcome, HerdrError>> + Send + 'a>>;

    fn wait<'a>(
        &'a self,
        agent: &'a AgentName,
        session: Option<&'a HerdrSession>,
        until: &'a [HerdrAgentStatus],
        timeout: Duration,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrWaitOutcome, HerdrError>> + Send + 'a>>;

    fn get<'a>(
        &'a self,
        agent: &'a AgentName,
        session: Option<&'a HerdrSession>,
        deadline: RequestDeadline,
        breaker_policy: BreakerPolicy,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrGetOutcome, HerdrError>> + Send + 'a>>;

    fn list<'a>(
        &'a self,
        session: Option<&'a HerdrSession>,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrListOutcome, HerdrError>> + Send + 'a>>;

    /// Shows a desktop notification without targeting a Herdr pane.
    fn notify<'a>(
        &'a self,
        title: &'a str,
        body: &'a str,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<(), HerdrError>> + Send + 'a>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HerdrBreakerState {
    Closed,
    Open { retry_after: Duration },
    HalfOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HerdrBreakerSnapshot {
    pub state: HerdrBreakerState,
    pub consecutive_failures: u32,
}

#[derive(Debug, Default)]
struct BreakerState {
    consecutive_failures: u32,
    opened_at: Option<Instant>,
    half_open_probe: bool,
}

/// Supplies the current time to a [`HerdrSpawnBreaker`].
///
/// Production code always uses [`SystemBreakerClock`]. Tests that assert on
/// the breaker's cooldown window inject a fixed or manually advanced clock
/// instead, so assertions cannot be perturbed by scheduling delays under a
/// loaded, parallel test run (see `TestBreakerClock` in this module's tests).
trait BreakerClock: std::fmt::Debug + Send + Sync {
    fn now(&self) -> Instant;
}

#[derive(Debug, Default)]
struct SystemBreakerClock;

impl BreakerClock for SystemBreakerClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Per-host, in-memory circuit breaker shared by all Herdr operations.
#[derive(Debug, Clone)]
pub struct HerdrSpawnBreaker {
    state: Arc<Mutex<BreakerState>>,
    clock: Arc<dyn BreakerClock>,
}

impl HerdrSpawnBreaker {
    #[must_use]
    pub fn new() -> Self {
        Self::with_clock(Arc::new(SystemBreakerClock))
    }

    fn with_clock(clock: Arc<dyn BreakerClock>) -> Self {
        Self {
            state: Arc::new(Mutex::new(BreakerState::default())),
            clock,
        }
    }

    #[must_use]
    pub fn state(&self) -> HerdrBreakerState {
        let Ok(state) = self.state.lock() else {
            return HerdrBreakerState::Open {
                retry_after: BREAKER_MAX_BACKOFF,
            };
        };
        breaker_state(&state, self.clock.as_ref())
    }

    /// Reads the state and failure counter under one lock for coherent
    /// diagnostic projection.
    #[must_use]
    pub fn snapshot(&self) -> HerdrBreakerSnapshot {
        let Ok(state) = self.state.lock() else {
            return HerdrBreakerSnapshot {
                state: HerdrBreakerState::Open {
                    retry_after: BREAKER_MAX_BACKOFF,
                },
                consecutive_failures: u32::MAX,
            };
        };
        HerdrBreakerSnapshot {
            state: breaker_state(&state, self.clock.as_ref()),
            consecutive_failures: state.consecutive_failures,
        }
    }

    #[must_use]
    pub fn permits_spawn(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        match breaker_state(&state, self.clock.as_ref()) {
            HerdrBreakerState::Closed => true,
            HerdrBreakerState::HalfOpen if !state.half_open_probe => {
                state.half_open_probe = true;
                true
            }
            HerdrBreakerState::Open { retry_after } if retry_after.is_zero() => {
                state.half_open_probe = true;
                true
            }
            HerdrBreakerState::Open { .. } | HerdrBreakerState::HalfOpen => false,
        }
    }

    pub fn record_success(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = BreakerState::default();
        }
    }

    pub fn record_infrastructure_failure(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.consecutive_failures = state.consecutive_failures.saturating_add(1);
            state.opened_at = Some(self.clock.now());
            state.half_open_probe = false;
        }
    }

    #[must_use]
    pub fn consecutive_failures(&self) -> u32 {
        self.state
            .lock()
            .map(|state| state.consecutive_failures)
            .unwrap_or(u32::MAX)
    }
}

impl Default for HerdrSpawnBreaker {
    fn default() -> Self {
        Self::new()
    }
}

fn breaker_state(state: &BreakerState, clock: &dyn BreakerClock) -> HerdrBreakerState {
    let Some(opened_at) = state.opened_at else {
        return HerdrBreakerState::Closed;
    };
    let retry_after = breaker_backoff(state.consecutive_failures)
        .saturating_sub(clock.now().saturating_duration_since(opened_at));
    if retry_after.is_zero() || state.half_open_probe {
        HerdrBreakerState::HalfOpen
    } else {
        HerdrBreakerState::Open { retry_after }
    }
}

fn breaker_backoff(consecutive_failures: u32) -> Duration {
    let exponent = consecutive_failures.saturating_sub(1).min(5);
    Duration::from_secs(1_u64 << exponent).min(BREAKER_MAX_BACKOFF)
}

#[derive(Debug, Clone)]
pub struct HerdrProcessInvoker {
    breaker: Arc<HerdrSpawnBreaker>,
    io: HerdrIo,
}

impl HerdrProcessInvoker {
    #[must_use]
    pub fn new(breaker: Arc<HerdrSpawnBreaker>) -> Self {
        Self {
            breaker,
            io: HerdrIo::default(),
        }
    }

    async fn call(
        &self,
        op: HerdrOp<'_>,
        session: Option<&HerdrSession>,
        deadline: RequestDeadline,
        breaker_policy: BreakerPolicy,
    ) -> Result<transport::HerdrEnvelope, HerdrError> {
        if breaker_policy == BreakerPolicy::Shared && !self.breaker.permits_spawn() {
            let retry_after = match self.breaker.state() {
                HerdrBreakerState::Open { retry_after } => retry_after,
                HerdrBreakerState::HalfOpen | HerdrBreakerState::Closed => Duration::ZERO,
            };
            return Err(HerdrError::Unavailable { retry_after });
        }
        let result = self.io.call(op, session, deadline).await;
        if result.is_err() && breaker_policy == BreakerPolicy::Shared {
            self.breaker.record_infrastructure_failure();
        }
        result
    }
}

fn validate_prompt_text(text: &str) -> Result<(), HerdrError> {
    if text.trim().is_empty() {
        Err(HerdrError::EmptyAgentPrompt)
    } else {
        Ok(())
    }
}

impl HerdrProcessAdapter for HerdrProcessInvoker {
    fn prompt<'a>(
        &'a self,
        agent: &'a AgentName,
        session: Option<&'a HerdrSession>,
        text: &'a str,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrPromptOutcome, HerdrError>> + Send + 'a>> {
        if let Err(error) = validate_prompt_text(text) {
            return Box::pin(async move { Err(error) });
        }
        Box::pin(async move {
            let result = self
                .call(
                    HerdrOp::Prompt { agent, text },
                    session,
                    deadline,
                    BreakerPolicy::Shared,
                )
                .await
                .and_then(prompt_from_envelope);
            record_result(&self.breaker, &result);
            result
        })
    }

    fn wait<'a>(
        &'a self,
        agent: &'a AgentName,
        session: Option<&'a HerdrSession>,
        until: &'a [HerdrAgentStatus],
        timeout: Duration,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrWaitOutcome, HerdrError>> + Send + 'a>> {
        Box::pin(async move {
            let result = self
                .call(
                    HerdrOp::Wait {
                        agent,
                        until,
                        timeout,
                    },
                    session,
                    deadline,
                    BreakerPolicy::Shared,
                )
                .await
                .and_then(snapshot_from_envelope);
            record_result(&self.breaker, &result);
            result
        })
    }

    fn get<'a>(
        &'a self,
        agent: &'a AgentName,
        session: Option<&'a HerdrSession>,
        deadline: RequestDeadline,
        breaker_policy: BreakerPolicy,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrGetOutcome, HerdrError>> + Send + 'a>> {
        Box::pin(async move {
            let result = self
                .call(HerdrOp::Get { agent }, session, deadline, breaker_policy)
                .await
                .and_then(get_from_envelope)
                .map(|snapshot| HerdrGetOutcome { snapshot });
            if breaker_policy == BreakerPolicy::Shared {
                record_result(&self.breaker, &result);
            }
            result
        })
    }

    fn list<'a>(
        &'a self,
        session: Option<&'a HerdrSession>,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrListOutcome, HerdrError>> + Send + 'a>> {
        Box::pin(async move {
            let result = self
                .call(HerdrOp::List, session, deadline, BreakerPolicy::Shared)
                .await
                .and_then(list_from_envelope);
            record_result(&self.breaker, &result);
            result
        })
    }

    fn notify<'a>(
        &'a self,
        title: &'a str,
        body: &'a str,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<(), HerdrError>> + Send + 'a>> {
        Box::pin(async move {
            self.call(
                HerdrOp::Notify { title, body },
                None,
                deadline,
                BreakerPolicy::Bypass,
            )
            .await
            .and_then(unit_from_envelope)
        })
    }
}

#[cfg(any())]
mod legacy_process_oracle {
    use super::legacy_parser_oracle::{parse_error, parse_list, parse_snapshot};
    use super::*;

    pub(super) async fn execute_get(
        binary: &str,
        breaker: Arc<HerdrSpawnBreaker>,
        agent: &AgentName,
        session: Option<&HerdrSession>,
        deadline: RequestDeadline,
        breaker_policy: BreakerPolicy,
    ) -> Result<HerdrGetOutcome, HerdrError> {
        let args = get_args(agent);
        let output =
            run_command_with_binary(binary, &breaker, &args, session, deadline, breaker_policy)
                .await?;
        let result = if output.success {
            parse_snapshot(&output.stdout).map(|snapshot| HerdrGetOutcome { snapshot })
        } else {
            Err(parse_error(&output.stderr))
        };
        if breaker_policy == BreakerPolicy::Shared {
            record_result(&breaker, &result);
        }
        result
    }

    pub(super) async fn execute_list(
        binary: &str,
        breaker: Arc<HerdrSpawnBreaker>,
        session: Option<&HerdrSession>,
        deadline: RequestDeadline,
    ) -> Result<HerdrListOutcome, HerdrError> {
        execute_list_with_args(binary, breaker, list_args(), session, deadline).await
    }

    pub(super) async fn execute_list_with_args(
        binary: &str,
        breaker: Arc<HerdrSpawnBreaker>,
        args: Vec<String>,
        session: Option<&HerdrSession>,
        deadline: RequestDeadline,
    ) -> Result<HerdrListOutcome, HerdrError> {
        let output = run_command_with_binary(
            binary,
            &breaker,
            &args,
            session,
            deadline,
            BreakerPolicy::Shared,
        )
        .await?;
        let result = if output.success {
            parse_list(&output.stdout)
        } else {
            Err(parse_error(&output.stderr))
        };
        record_result(&breaker, &result);
        result
    }

    pub(super) struct CommandOutput {
        stdout: String,
        stderr: String,
        success: bool,
    }

    /// Selects the timeout actually applied to a spawned `herdr` child process:
    /// whichever of the caller's `remaining` deadline or [`HERDR_PROCESS_CAP`] is
    /// smaller (HR-SAFE-002). A pure function so the caller-deadline-vs-process-
    /// cap precedence is unit-testable deterministically, without spawning a
    /// process or depending on wall-clock scheduling (see the
    /// `effective_process_timeout_*` tests below).
    pub(super) fn effective_process_timeout(remaining: Duration) -> Duration {
        remaining.min(HERDR_PROCESS_CAP)
    }

    pub(super) async fn run_command_with_binary(
        binary: &str,
        breaker: &HerdrSpawnBreaker,
        args: &[String],
        session: Option<&HerdrSession>,
        deadline: RequestDeadline,
        breaker_policy: BreakerPolicy,
    ) -> Result<CommandOutput, HerdrError> {
        if breaker_policy == BreakerPolicy::Shared && !breaker.permits_spawn() {
            let retry_after = match breaker.state() {
                HerdrBreakerState::Open { retry_after } => retry_after,
                HerdrBreakerState::HalfOpen | HerdrBreakerState::Closed => Duration::ZERO,
            };
            return Err(HerdrError::Unavailable { retry_after });
        }
        let Some(remaining) = deadline.remaining() else {
            if breaker_policy == BreakerPolicy::Shared {
                breaker.record_infrastructure_failure();
            }
            return Err(HerdrError::TimedOut);
        };
        let effective_timeout = effective_process_timeout(remaining);
        let mut command = tokio::process::Command::new(binary);
        command
            .args(args)
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some((name, value)) = session_environment(session) {
            command.env(name, value);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                if breaker_policy == BreakerPolicy::Shared {
                    breaker.record_infrastructure_failure();
                }
                return Err(HerdrError::ServerUnavailable);
            }
        };
        let status = match tokio::time::timeout(effective_timeout, child.wait()).await {
            Ok(Ok(status)) => status,
            Ok(Err(_)) => {
                if breaker_policy == BreakerPolicy::Shared {
                    breaker.record_infrastructure_failure();
                }
                return Err(HerdrError::ServerUnavailable);
            }
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                if breaker_policy == BreakerPolicy::Shared {
                    breaker.record_infrastructure_failure();
                }
                return Err(HerdrError::TimedOut);
            }
        };
        let (stdout, stderr) = capture_command_output(&mut child).await?;
        Ok(CommandOutput {
            stdout,
            stderr,
            success: status.success(),
        })
    }

    async fn capture_command_output(
        child: &mut tokio::process::Child,
    ) -> Result<(String, String), HerdrError> {
        let (stdout, stdout_truncated) = if let Some(pipe) = child.stdout.take() {
            read_capped(pipe).await
        } else {
            (Vec::new(), false)
        };
        let (stderr, stderr_truncated) = if let Some(pipe) = child.stderr.take() {
            read_capped(pipe).await
        } else {
            (Vec::new(), false)
        };
        if stdout_truncated || stderr_truncated {
            return Err(HerdrError::Advisory {
                code: format!(
                    "output_truncated: stdout={stdout_truncated}, stderr={stderr_truncated}, limit={HERDR_MAX_OUTPUT_BYTES}"
                ),
            });
        }
        Ok((
            String::from_utf8_lossy(&stdout).into_owned(),
            String::from_utf8_lossy(&stderr).into_owned(),
        ))
    }

    pub(super) async fn read_capped(reader: impl tokio::io::AsyncRead + Unpin) -> (Vec<u8>, bool) {
        let mut output = Vec::new();
        let _ = reader
            .take((HERDR_MAX_OUTPUT_BYTES as u64).saturating_add(1))
            .read_to_end(&mut output)
            .await;
        let truncated = output.len() > HERDR_MAX_OUTPUT_BYTES;
        if truncated {
            output.truncate(HERDR_MAX_OUTPUT_BYTES);
        }
        (output, truncated)
    }

    pub(super) fn session_environment(
        session: Option<&HerdrSession>,
    ) -> Option<(&'static str, &str)> {
        session.map(|session| ("HERDR_SESSION", session.as_str()))
    }

    pub(super) fn prompt_args(agent: &AgentName, text: &str) -> Vec<String> {
        vec![
            "agent".to_owned(),
            "prompt".to_owned(),
            agent.to_string(),
            text.to_owned(),
        ]
    }

    pub(super) fn wait_args(
        agent: &AgentName,
        until: &[HerdrAgentStatus],
        timeout: Duration,
    ) -> Vec<String> {
        let mut args = vec!["agent".to_owned(), "wait".to_owned(), agent.to_string()];
        for status in until {
            args.push("--until".to_owned());
            args.push(status.as_str().to_owned());
        }
        args.push("--timeout".to_owned());
        args.push(timeout.as_millis().to_string());
        args
    }

    pub(super) fn get_args(agent: &AgentName) -> Vec<String> {
        vec!["agent".to_owned(), "get".to_owned(), agent.to_string()]
    }

    pub(super) fn list_args() -> Vec<String> {
        vec!["agent".to_owned(), "list".to_owned()]
    }

    pub(super) fn notification_args(title: &str, body: &str) -> Vec<String> {
        vec![
            "notification".to_owned(),
            "show".to_owned(),
            title.to_owned(),
            "--body".to_owned(),
            body.to_owned(),
            "--sound".to_owned(),
            "request".to_owned(),
        ]
    }
}

fn record_result<T>(breaker: &HerdrSpawnBreaker, result: &Result<T, HerdrError>) {
    if let Err(error) = result {
        if error.is_infrastructure() {
            breaker.record_infrastructure_failure();
        } else {
            // A typed lifecycle/target response proves that the Herdr
            // process was reachable. In particular, a lifecycle response
            // during HALF_OPEN releases the single probe and closes the
            // infrastructure breaker rather than wedging it half-open.
            breaker.record_success();
        }
    } else {
        breaker.record_success();
    }
}

#[cfg(any())]
mod legacy_parser_oracle {
    use super::*;

    pub(super) fn parse_prompt(stdout: &str) -> Result<HerdrPromptOutcome, HerdrError> {
        let envelope: Value =
            serde_json::from_str(stdout).map_err(|_| HerdrError::ProtocolMismatch)?;
        if let Some(result) = envelope.get("result") {
            let snapshot = result
                .get("agent")
                .map(snapshot_from_value)
                .transpose()?
                .unwrap_or(AgentSnapshot {
                    name: None,
                    status: HerdrAgentStatus::Unknown,
                    workspace_id: None,
                });
            return Ok(HerdrPromptOutcome::Accepted(snapshot));
        }
        Err(parse_error_value(&envelope))
    }

    pub(super) fn parse_snapshot(stdout: &str) -> Result<AgentSnapshot, HerdrError> {
        let envelope: Value =
            serde_json::from_str(stdout).map_err(|_| HerdrError::ProtocolMismatch)?;
        let result = envelope.get("result").ok_or(HerdrError::ProtocolMismatch)?;
        snapshot_from_value(result.get("agent").unwrap_or(result))
    }

    pub(super) fn parse_list(stdout: &str) -> Result<HerdrListOutcome, HerdrError> {
        let envelope: Value =
            serde_json::from_str(stdout).map_err(|_| HerdrError::ProtocolMismatch)?;
        let result = envelope.get("result").ok_or(HerdrError::ProtocolMismatch)?;
        let agents = result
            .get("agents")
            .and_then(Value::as_array)
            .ok_or(HerdrError::ProtocolMismatch)?
            .iter()
            .map(snapshot_from_value)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(HerdrListOutcome { agents })
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

    pub(super) fn parse_error(stderr: &str) -> HerdrError {
        match serde_json::from_str::<Value>(stderr) {
            Ok(envelope) => parse_error_value(&envelope),
            Err(_) => HerdrError::ProtocolMismatch,
        }
    }

    fn parse_error_value(envelope: &Value) -> HerdrError {
        let Some(code) = envelope
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_str)
        else {
            return HerdrError::ProtocolMismatch;
        };
        match code {
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
            "server_unavailable" => HerdrError::ServerUnavailable,
            "internal_error" | "agent_prompt_failed" => HerdrError::InternalError,
            other => HerdrError::Advisory {
                code: other.to_owned(),
            },
        }
    }
}

#[cfg(feature = "test-utils")]
pub mod testing {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum FakeHerdrCall {
        Prompt {
            agent: String,
            session: Option<HerdrSession>,
            text: String,
        },
        Wait {
            agent: String,
            session: Option<HerdrSession>,
            until: Vec<HerdrAgentStatus>,
            timeout: Duration,
        },
        Get {
            agent: String,
            session: Option<HerdrSession>,
            breaker_policy: BreakerPolicy,
        },
        List {
            session: Option<HerdrSession>,
        },
        Notify {
            title: String,
            body: String,
        },
    }

    #[derive(Debug, Default)]
    struct FakeState {
        calls: Vec<FakeHerdrCall>,
        prompt_results: VecDeque<Result<HerdrPromptOutcome, HerdrError>>,
        prompt_gate: Option<Arc<tokio::sync::Notify>>,
        wait_results: VecDeque<Result<HerdrWaitOutcome, HerdrError>>,
        get_results: VecDeque<Result<HerdrGetOutcome, HerdrError>>,
        list_results: VecDeque<Result<HerdrListOutcome, HerdrError>>,
        list_gate: Option<Arc<tokio::sync::Notify>>,
        notify_results: VecDeque<Result<(), HerdrError>>,
    }

    #[derive(Debug, Default, Clone)]
    pub struct FakeHerdrProcessAdapter {
        state: Arc<Mutex<FakeState>>,
    }

    impl FakeHerdrProcessAdapter {
        #[must_use]
        pub fn calls(&self) -> Vec<FakeHerdrCall> {
            self.state
                .lock()
                .map(|state| state.calls.clone())
                .unwrap_or_default()
        }

        pub fn queue_prompt_result(&self, result: Result<HerdrPromptOutcome, HerdrError>) {
            if let Ok(mut state) = self.state.lock() {
                state.prompt_results.push_back(result);
            }
        }

        /// Blocks the next prompt until the returned notifier is woken.
        pub fn block_next_prompt(&self) -> Arc<tokio::sync::Notify> {
            let gate = Arc::new(tokio::sync::Notify::new());
            if let Ok(mut state) = self.state.lock() {
                state.prompt_gate = Some(Arc::clone(&gate));
            }
            gate
        }

        pub fn queue_wait_result(&self, result: Result<HerdrWaitOutcome, HerdrError>) {
            if let Ok(mut state) = self.state.lock() {
                state.wait_results.push_back(result);
            }
        }

        pub fn queue_get_result(&self, result: Result<HerdrGetOutcome, HerdrError>) {
            if let Ok(mut state) = self.state.lock() {
                state.get_results.push_back(result);
            }
        }

        pub fn queue_list_result(&self, result: Result<HerdrListOutcome, HerdrError>) {
            if let Ok(mut state) = self.state.lock() {
                state.list_results.push_back(result);
            }
        }

        pub fn queue_notify_result(&self, result: Result<(), HerdrError>) {
            if let Ok(mut state) = self.state.lock() {
                state.notify_results.push_back(result);
            }
        }

        /// Blocks the next list call until the returned notifier is woken.
        pub fn block_next_list(&self) -> Arc<tokio::sync::Notify> {
            let gate = Arc::new(tokio::sync::Notify::new());
            if let Ok(mut state) = self.state.lock() {
                state.list_gate = Some(Arc::clone(&gate));
            }
            gate
        }
    }

    fn default_snapshot(agent: &AgentName) -> AgentSnapshot {
        AgentSnapshot {
            name: Some(agent.to_string()),
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        }
    }

    impl HerdrProcessAdapter for FakeHerdrProcessAdapter {
        fn prompt<'a>(
            &'a self,
            agent: &'a AgentName,
            session: Option<&'a HerdrSession>,
            text: &'a str,
            _deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<HerdrPromptOutcome, HerdrError>> + Send + 'a>>
        {
            let (gate, result) = self
                .state
                .lock()
                .map(|mut state| {
                    state.calls.push(FakeHerdrCall::Prompt {
                        agent: agent.to_string(),
                        session: session.cloned(),
                        text: text.to_owned(),
                    });
                    (state.prompt_gate.take(), state.prompt_results.pop_front())
                })
                .ok()
                .unwrap_or((None, None));
            let result =
                result.unwrap_or_else(|| Ok(HerdrPromptOutcome::Accepted(default_snapshot(agent))));
            Box::pin(async move {
                if let Some(gate) = gate {
                    gate.notified().await;
                }
                result
            })
        }

        fn wait<'a>(
            &'a self,
            agent: &'a AgentName,
            session: Option<&'a HerdrSession>,
            until: &'a [HerdrAgentStatus],
            timeout: Duration,
            _deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<HerdrWaitOutcome, HerdrError>> + Send + 'a>>
        {
            let result = self
                .state
                .lock()
                .map(|mut state| {
                    state.calls.push(FakeHerdrCall::Wait {
                        agent: agent.to_string(),
                        session: session.cloned(),
                        until: until.to_vec(),
                        timeout,
                    });
                    state.wait_results.pop_front()
                })
                .ok()
                .flatten()
                .unwrap_or_else(|| {
                    Ok(HerdrWaitOutcome {
                        snapshot: default_snapshot(agent),
                    })
                });
            Box::pin(async move { result })
        }

        fn get<'a>(
            &'a self,
            agent: &'a AgentName,
            session: Option<&'a HerdrSession>,
            _deadline: RequestDeadline,
            breaker_policy: BreakerPolicy,
        ) -> Pin<Box<dyn Future<Output = Result<HerdrGetOutcome, HerdrError>> + Send + 'a>>
        {
            let result = self
                .state
                .lock()
                .map(|mut state| {
                    state.calls.push(FakeHerdrCall::Get {
                        agent: agent.to_string(),
                        session: session.cloned(),
                        breaker_policy,
                    });
                    state.get_results.pop_front()
                })
                .ok()
                .flatten()
                .unwrap_or_else(|| {
                    Ok(HerdrGetOutcome {
                        snapshot: default_snapshot(agent),
                    })
                });
            Box::pin(async move { result })
        }

        fn list<'a>(
            &'a self,
            session: Option<&'a HerdrSession>,
            _deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<HerdrListOutcome, HerdrError>> + Send + 'a>>
        {
            let (gate, result) = self
                .state
                .lock()
                .map(|mut state| {
                    state.calls.push(FakeHerdrCall::List {
                        session: session.cloned(),
                    });
                    (state.list_gate.take(), state.list_results.pop_front())
                })
                .ok()
                .unwrap_or((None, None));
            let result = result.unwrap_or_else(|| Ok(HerdrListOutcome { agents: Vec::new() }));
            Box::pin(async move {
                if let Some(gate) = gate {
                    gate.notified().await;
                }
                result
            })
        }

        fn notify<'a>(
            &'a self,
            title: &'a str,
            body: &'a str,
            _deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<(), HerdrError>> + Send + 'a>> {
            let result = self
                .state
                .lock()
                .map(|mut state| {
                    state.calls.push(FakeHerdrCall::Notify {
                        title: title.to_owned(),
                        body: body.to_owned(),
                    });
                    state.notify_results.pop_front()
                })
                .ok()
                .flatten()
                .unwrap_or(Ok(()));
            Box::pin(async move { result })
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn herdr_minimum_version_is_three_part_semver_at_least_0_8_0() {
        let parts: Vec<u64> = super::HERDR_MINIMUM_VERSION
            .split('.')
            .map(|part| {
                part.parse()
                    .expect("HERDR_MINIMUM_VERSION parts are integers")
            })
            .collect();
        assert_eq!(
            parts.len(),
            3,
            "HERDR_MINIMUM_VERSION must be MAJOR.MINOR.PATCH"
        );
        assert!(
            (parts[0], parts[1], parts[2]) >= (0, 8, 0),
            "HERDR_MINIMUM_VERSION may only be raised with Rand's recorded approval (ADR-061)"
        );
    }

    use super::*;

    /// A manually controlled clock used by tests that assert on the
    /// breaker's cooldown state. Keeping the instant behind a mutex allows
    /// the same fixture to cover synchronous state transitions and async
    /// process-backed calls without relying on scheduler timing.
    #[derive(Debug)]
    struct TestBreakerClock {
        now: Mutex<Instant>,
    }

    impl TestBreakerClock {
        fn new() -> Self {
            Self {
                now: Mutex::new(Instant::now()),
            }
        }

        fn advance(&self, duration: Duration) {
            *self.now.lock().expect("test clock lock") += duration;
        }
    }

    impl BreakerClock for TestBreakerClock {
        fn now(&self) -> Instant {
            *self.now.lock().expect("test clock lock")
        }
    }

    #[test]
    fn parses_prompt_snapshot_and_structured_errors() {
        let outcome = prompt_from_envelope(transport::HerdrEnvelope {
            result: Some(serde_json::json!({"agent":{"name":"alice","agent_status":"working"}})),
            error: None,
        })
        .expect("prompt response");
        assert_eq!(
            outcome,
            HerdrPromptOutcome::Accepted(AgentSnapshot {
                name: Some("alice".to_owned()),
                status: HerdrAgentStatus::Working,
                workspace_id: None,
            })
        );
        assert_eq!(
            transport::unit_from_envelope(transport::HerdrEnvelope {
                result: None,
                error: Some(transport::HerdrErrorEnvelope {
                    code: "agent_blocked".to_owned(),
                    message: String::new(),
                    retry_after_ms: None,
                }),
            }),
            Err(HerdrError::AgentBlocked)
        );
    }

    #[test]
    fn every_adapter_argv_matches_the_herdr_contract() {
        let agent: AgentName = "alice".parse().expect("agent");
        let text = "line one\nline two\nline three\nline four\nline five\nline six";
        let args = transport_cli::command_args(HerdrOp::Prompt {
            agent: &agent,
            text,
        });
        assert_eq!(args.len(), 4);
        assert_eq!(args, vec!["agent", "prompt", "alice", text]);
        assert_eq!(
            validate_prompt_text("  \n"),
            Err(HerdrError::EmptyAgentPrompt)
        );
        assert_eq!(
            transport_cli::command_args(HerdrOp::Wait {
                agent: &agent,
                until: &[HerdrAgentStatus::Idle, HerdrAgentStatus::Working],
                timeout: Duration::from_millis(2500),
            }),
            vec![
                "agent",
                "wait",
                "alice",
                "--until",
                "idle",
                "--until",
                "working",
                "--timeout",
                "2500"
            ]
        );
        assert_eq!(
            transport_cli::command_args(HerdrOp::Get { agent: &agent }),
            vec!["agent", "get", "alice"]
        );
        assert_eq!(
            transport_cli::command_args(HerdrOp::List),
            vec!["agent", "list"]
        );
        assert_eq!(
            transport_cli::command_args(HerdrOp::Notify {
                title: "Task escalation",
                body: "line one\nline two"
            }),
            vec![
                "notification",
                "show",
                "Task escalation",
                "--body",
                "line one\nline two",
                "--sound",
                "request"
            ]
        );
    }

    #[tokio::test]
    async fn capped_output_reports_truncation_without_retaining_extra_bytes() {
        let input = vec![b'x'; HERDR_MAX_OUTPUT_BYTES + 1];
        let (output, truncated) = transport_cli::read_capped(std::io::Cursor::new(input)).await;
        assert!(truncated);
        assert_eq!(output.len(), HERDR_MAX_OUTPUT_BYTES);
    }

    #[tokio::test]
    async fn empty_prompt_is_rejected_before_process_spawn() {
        let agent: AgentName = "alice".parse().expect("agent");
        let invoker = HerdrProcessInvoker {
            breaker: Arc::new(HerdrSpawnBreaker::default()),
            io: HerdrIo::default(),
        };
        assert_eq!(
            invoker
                .prompt(
                    &agent,
                    None,
                    " \n",
                    RequestDeadline::after(Duration::from_secs(1)),
                )
                .await,
            Err(HerdrError::EmptyAgentPrompt)
        );
    }

    #[test]
    fn typed_non_infrastructure_error_leaves_the_breaker_closed() {
        let breaker = Arc::new(HerdrSpawnBreaker::default());
        record_result::<()>(&breaker, &Err(HerdrError::AgentNotReady));
        assert_eq!(breaker.consecutive_failures(), 0);
        assert_eq!(breaker.state(), HerdrBreakerState::Closed);
    }

    /// HR-SAFE-002 precedence proof (deterministic, no wall-clock): a
    /// caller deadline shorter than `HERDR_PROCESS_CAP` governs the
    /// effective process timeout.
    #[test]
    fn effective_process_timeout_uses_the_shorter_caller_deadline() {
        assert_eq!(
            transport_cli::effective_process_timeout(Duration::from_secs(2)),
            Duration::from_secs(2)
        );
    }

    /// HR-SAFE-002 precedence proof (deterministic, no wall-clock): a
    /// caller deadline longer than `HERDR_PROCESS_CAP` is clamped to the
    /// cap, so a slow/misbehaving `herdr` child is never allowed to run
    /// past it.
    #[test]
    fn effective_process_timeout_clamps_to_the_process_cap() {
        assert_eq!(
            transport_cli::effective_process_timeout(Duration::from_secs(30)),
            HERDR_PROCESS_CAP
        );
    }

    /// A caller deadline of zero (already expired) selects a zero timeout,
    /// never a negative or the process cap.
    #[test]
    fn effective_process_timeout_of_zero_remaining_is_zero() {
        assert_eq!(
            transport_cli::effective_process_timeout(Duration::ZERO),
            Duration::ZERO
        );
    }

    #[test]
    fn breaker_backoff_and_probe_policy_are_explicit() {
        for (failures, seconds) in [(1, 1), (2, 2), (3, 4), (4, 8), (5, 16), (6, 30), (20, 30)] {
            assert_eq!(breaker_backoff(failures), Duration::from_secs(seconds));
        }
        let breaker = HerdrSpawnBreaker::default();
        for _ in 0..3 {
            record_result::<()>(&breaker, &Err(HerdrError::ServerNotRunning));
        }
        assert_eq!(breaker.consecutive_failures(), 3);
        assert!(!breaker.permits_spawn());
        {
            let mut state = breaker.state.lock().expect("breaker lock");
            state.opened_at = Some(
                Instant::now()
                    .checked_sub(Duration::from_secs(31))
                    .expect("a 31-second test offset must be representable"),
            );
            state.half_open_probe = false;
        }
        assert!(breaker.permits_spawn(), "half-open allows one probe");
        assert!(!breaker.permits_spawn(), "half-open rejects a second probe");
        breaker.record_success();
        assert_eq!(breaker.snapshot().consecutive_failures, 0);
        assert_eq!(breaker.state(), HerdrBreakerState::Closed);
    }

    #[test]
    fn all_structured_error_codes_have_typed_mappings() {
        let cases = [
            ("agent_blocked", HerdrError::AgentBlocked),
            ("agent_not_found", HerdrError::AgentNotFound),
            ("agent_not_ready", HerdrError::AgentNotReady),
            ("agent_target_ambiguous", HerdrError::AgentTargetAmbiguous),
            ("agent_not_running", HerdrError::AgentNotRunning),
            ("agent_prompt_stalled", HerdrError::AgentPromptStalled),
            ("server_not_running", HerdrError::ServerNotRunning),
            ("protocol_mismatch", HerdrError::ProtocolMismatch),
            ("timeout", HerdrError::Timeout),
            ("invalid_agent_name", HerdrError::InvalidAgentName),
            ("empty_agent_prompt", HerdrError::EmptyAgentPrompt),
            ("server_unavailable", HerdrError::ServerUnavailable),
            ("internal_error", HerdrError::InternalError),
            ("agent_prompt_failed", HerdrError::InternalError),
        ];
        for (code, expected) in cases {
            assert_eq!(
                transport::unit_from_envelope(transport::HerdrEnvelope {
                    result: None,
                    error: Some(transport::HerdrErrorEnvelope {
                        code: code.to_owned(),
                        message: String::new(),
                        retry_after_ms: None
                    }),
                }),
                Err(expected)
            );
        }
    }

    #[test]
    fn breaker_opens_with_exponential_backoff_and_half_open_probe() {
        let clock = Arc::new(TestBreakerClock::new());
        let breaker = HerdrSpawnBreaker::with_clock(clock.clone());
        assert_eq!(breaker.state(), HerdrBreakerState::Closed);
        assert!(breaker.permits_spawn());
        breaker.record_infrastructure_failure();
        assert!(matches!(breaker.state(), HerdrBreakerState::Open { .. }));
        assert!(!breaker.permits_spawn());
        clock.advance(Duration::from_secs(1));
        assert!(
            breaker.permits_spawn(),
            "the injected clock reached the probe window"
        );
        breaker.record_success();
        assert_eq!(breaker.state(), HerdrBreakerState::Closed);
    }
}
