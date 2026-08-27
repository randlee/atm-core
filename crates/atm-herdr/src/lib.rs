//! Tokio-native, shell-free process boundary for the Herdr CLI.

use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::types::AgentName;
use atm_core::{HerdrSession, RequestDeadline};
use serde_json::Value;
use tokio::io::AsyncReadExt;

pub const HERDR_WAKE_TEXT: &str = "You have unread ATM messages. Run: atm read";
const HERDR_PROCESS_CAP: Duration = Duration::from_secs(5);
const BREAKER_MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HerdrAgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
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
                AtmErrorCode::HerdrPromptFailed,
                "Herdr agent is not ready".to_owned(),
            ),
            HerdrError::AgentTargetAmbiguous => (
                AtmErrorCode::HerdrPromptFailed,
                "Herdr agent target is ambiguous".to_owned(),
            ),
            HerdrError::AgentNotRunning => (
                AtmErrorCode::HerdrUnavailable,
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

/// Per-host, in-memory circuit breaker shared by all Herdr operations.
#[derive(Debug, Clone)]
pub struct HerdrSpawnBreaker {
    state: Arc<Mutex<BreakerState>>,
}

impl HerdrSpawnBreaker {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(BreakerState::default())),
        }
    }

    #[must_use]
    pub fn state(&self) -> HerdrBreakerState {
        let Ok(state) = self.state.lock() else {
            return HerdrBreakerState::Open {
                retry_after: BREAKER_MAX_BACKOFF,
            };
        };
        breaker_state(&state)
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
            state: breaker_state(&state),
            consecutive_failures: state.consecutive_failures,
        }
    }

    #[must_use]
    pub fn permits_spawn(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        match breaker_state(&state) {
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
            state.opened_at = Some(Instant::now());
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

fn breaker_state(state: &BreakerState) -> HerdrBreakerState {
    let Some(opened_at) = state.opened_at else {
        return HerdrBreakerState::Closed;
    };
    let retry_after =
        breaker_backoff(state.consecutive_failures).saturating_sub(opened_at.elapsed());
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
}

impl HerdrProcessInvoker {
    #[must_use]
    pub fn new(breaker: Arc<HerdrSpawnBreaker>) -> Self {
        Self { breaker }
    }
}

impl HerdrProcessAdapter for HerdrProcessInvoker {
    fn prompt<'a>(
        &'a self,
        agent: &'a AgentName,
        session: Option<&'a HerdrSession>,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrPromptOutcome, HerdrError>> + Send + 'a>> {
        Box::pin(async move {
            let args = prompt_args(agent);
            let output = run_command(
                &self.breaker,
                &args,
                session,
                deadline,
                BreakerPolicy::Shared,
            )
            .await?;
            let result = if output.success {
                parse_prompt(&output.stdout)
            } else {
                Err(parse_error(&output.stderr))
            };
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
            let args = wait_args(agent, until, timeout);
            let output = run_command(
                &self.breaker,
                &args,
                session,
                deadline,
                BreakerPolicy::Shared,
            )
            .await?;
            let result = if output.success {
                parse_snapshot(&output.stdout).map(|snapshot| HerdrWaitOutcome { snapshot })
            } else {
                Err(parse_error(&output.stderr))
            };
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
        Box::pin(execute_get(
            "herdr",
            Arc::clone(&self.breaker),
            agent,
            session,
            deadline,
            breaker_policy,
        ))
    }

    fn list<'a>(
        &'a self,
        session: Option<&'a HerdrSession>,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrListOutcome, HerdrError>> + Send + 'a>> {
        Box::pin(execute_list(
            "herdr",
            Arc::clone(&self.breaker),
            session,
            deadline,
        ))
    }
}

async fn execute_get(
    binary: &str,
    breaker: Arc<HerdrSpawnBreaker>,
    agent: &AgentName,
    session: Option<&HerdrSession>,
    deadline: RequestDeadline,
    breaker_policy: BreakerPolicy,
) -> Result<HerdrGetOutcome, HerdrError> {
    let args = get_args(agent);
    let output =
        run_command_with_binary(binary, &breaker, &args, session, deadline, breaker_policy).await?;
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

async fn execute_list(
    binary: &str,
    breaker: Arc<HerdrSpawnBreaker>,
    session: Option<&HerdrSession>,
    deadline: RequestDeadline,
) -> Result<HerdrListOutcome, HerdrError> {
    let args = list_args();
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
    if result.is_err() {
        breaker.record_infrastructure_failure();
    } else {
        breaker.record_success();
    }
    result
}

impl HerdrAgentStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Unknown => "unknown",
        }
    }
}

struct CommandOutput {
    stdout: String,
    stderr: String,
    success: bool,
}

async fn run_command(
    breaker: &HerdrSpawnBreaker,
    args: &[String],
    session: Option<&HerdrSession>,
    deadline: RequestDeadline,
    breaker_policy: BreakerPolicy,
) -> Result<CommandOutput, HerdrError> {
    run_command_with_binary("herdr", breaker, args, session, deadline, breaker_policy).await
}

async fn run_command_with_binary(
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
    let effective_timeout = remaining.min(HERDR_PROCESS_CAP);
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
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut stdout).await;
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut stderr).await;
    }
    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        success: status.success(),
    })
}

fn session_environment(session: Option<&HerdrSession>) -> Option<(&'static str, &str)> {
    session.map(|session| ("HERDR_SESSION", session.as_str()))
}

fn prompt_args(agent: &AgentName) -> Vec<String> {
    vec![
        "agent".to_owned(),
        "prompt".to_owned(),
        agent.to_string(),
        HERDR_WAKE_TEXT.to_owned(),
    ]
}

fn wait_args(agent: &AgentName, until: &[HerdrAgentStatus], timeout: Duration) -> Vec<String> {
    let mut args = vec!["agent".to_owned(), "wait".to_owned(), agent.to_string()];
    for status in until {
        args.push("--until".to_owned());
        args.push(status.as_str().to_owned());
    }
    args.push("--timeout".to_owned());
    args.push(timeout.as_millis().to_string());
    args
}

fn get_args(agent: &AgentName) -> Vec<String> {
    vec!["agent".to_owned(), "get".to_owned(), agent.to_string()]
}

fn list_args() -> Vec<String> {
    vec!["agent".to_owned(), "list".to_owned()]
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

fn parse_prompt(stdout: &str) -> Result<HerdrPromptOutcome, HerdrError> {
    let envelope: Value = serde_json::from_str(stdout).map_err(|_| HerdrError::ProtocolMismatch)?;
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

fn parse_snapshot(stdout: &str) -> Result<AgentSnapshot, HerdrError> {
    let envelope: Value = serde_json::from_str(stdout).map_err(|_| HerdrError::ProtocolMismatch)?;
    let result = envelope.get("result").ok_or(HerdrError::ProtocolMismatch)?;
    snapshot_from_value(result.get("agent").unwrap_or(result))
}

fn parse_list(stdout: &str) -> Result<HerdrListOutcome, HerdrError> {
    let envelope: Value = serde_json::from_str(stdout).map_err(|_| HerdrError::ProtocolMismatch)?;
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

fn parse_error(stderr: &str) -> HerdrError {
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

#[cfg(feature = "test-utils")]
pub mod testing {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum FakeHerdrCall {
        Prompt {
            agent: String,
            session: Option<HerdrSession>,
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
    }

    #[derive(Debug, Default)]
    struct FakeState {
        calls: Vec<FakeHerdrCall>,
        prompt_results: VecDeque<Result<HerdrPromptOutcome, HerdrError>>,
        wait_results: VecDeque<Result<HerdrWaitOutcome, HerdrError>>,
        get_results: VecDeque<Result<HerdrGetOutcome, HerdrError>>,
        list_results: VecDeque<Result<HerdrListOutcome, HerdrError>>,
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
            _deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<HerdrPromptOutcome, HerdrError>> + Send + 'a>>
        {
            let result = self
                .state
                .lock()
                .map(|mut state| {
                    state.calls.push(FakeHerdrCall::Prompt {
                        agent: agent.to_string(),
                        session: session.cloned(),
                    });
                    state.prompt_results.pop_front()
                })
                .ok()
                .flatten()
                .unwrap_or_else(|| Ok(HerdrPromptOutcome::Accepted(default_snapshot(agent))));
            Box::pin(async move { result })
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
            let result = self
                .state
                .lock()
                .map(|mut state| {
                    state.calls.push(FakeHerdrCall::List {
                        session: session.cloned(),
                    });
                    state.list_results.pop_front()
                })
                .ok()
                .flatten()
                .unwrap_or_else(|| Ok(HerdrListOutcome { agents: Vec::new() }));
            Box::pin(async move { result })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_text_is_fixed_and_non_empty() {
        assert_eq!(
            HERDR_WAKE_TEXT,
            "You have unread ATM messages. Run: atm read"
        );
    }

    #[test]
    fn parses_prompt_snapshot_and_structured_errors() {
        let outcome =
            parse_prompt(r#"{"result":{"agent":{"name":"alice","agent_status":"working"}}}"#)
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
            parse_error(r#"{"error":{"code":"agent_blocked"}}"#),
            HerdrError::AgentBlocked
        );
    }

    #[test]
    fn every_adapter_argv_matches_the_herdr_contract() {
        let agent: AgentName = "alice".parse().expect("agent");
        assert_eq!(
            prompt_args(&agent),
            vec![
                "agent",
                "prompt",
                "alice",
                "You have unread ATM messages. Run: atm read"
            ]
        );
        assert_eq!(
            wait_args(
                &agent,
                &[HerdrAgentStatus::Idle, HerdrAgentStatus::Working],
                Duration::from_millis(2500)
            ),
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
        assert_eq!(get_args(&agent), vec!["agent", "get", "alice"]);
        assert_eq!(list_args(), vec!["agent", "list"]);
    }

    #[test]
    fn session_environment_is_only_present_for_an_explicit_session() {
        assert_eq!(session_environment(None), None);
        let session = HerdrSession::new("team-a").expect("session");
        assert_eq!(
            session_environment(Some(&session)),
            Some(("HERDR_SESSION", "team-a"))
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn external_deadline_kills_and_reaps_a_never_exiting_child() {
        let breaker = HerdrSpawnBreaker::default();
        let result = run_command_with_binary(
            "/bin/sh",
            &breaker,
            &["-c".to_owned(), "trap '' TERM; sleep 30".to_owned()],
            None,
            RequestDeadline::after(Duration::from_millis(50)),
            BreakerPolicy::Bypass,
        )
        .await;
        assert!(matches!(result, Err(HerdrError::TimedOut)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bypass_policy_spawns_even_when_the_shared_breaker_is_open() {
        let breaker = HerdrSpawnBreaker::default();
        breaker.record_infrastructure_failure();
        let failures = breaker.consecutive_failures();
        let result = run_command_with_binary(
            "/usr/bin/true",
            &breaker,
            &[],
            None,
            RequestDeadline::after(Duration::from_secs(1)),
            BreakerPolicy::Bypass,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(breaker.consecutive_failures(), failures);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failing_bypass_get_does_not_open_the_shared_breaker() {
        let breaker = Arc::new(HerdrSpawnBreaker::default());
        breaker.record_infrastructure_failure();
        let failures = breaker.consecutive_failures();
        let agent: AgentName = "alice".parse().expect("agent");
        let result = execute_get(
            "/usr/bin/false",
            Arc::clone(&breaker),
            &agent,
            None,
            RequestDeadline::after(Duration::from_secs(1)),
            BreakerPolicy::Bypass,
        )
        .await;
        assert!(result.is_err());
        assert_eq!(breaker.consecutive_failures(), failures);
        assert!(matches!(breaker.state(), HerdrBreakerState::Open { .. }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failing_list_opens_the_shared_breaker() {
        let breaker = Arc::new(HerdrSpawnBreaker::default());
        let result = execute_list(
            "/usr/bin/false",
            Arc::clone(&breaker),
            None,
            RequestDeadline::after(Duration::from_secs(1)),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(breaker.consecutive_failures(), 1);
        assert!(matches!(breaker.state(), HerdrBreakerState::Open { .. }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn two_second_caller_deadline_preempts_the_five_second_process_cap() {
        let started = Instant::now();
        let breaker = HerdrSpawnBreaker::default();
        let result = run_command_with_binary(
            "/bin/sh",
            &breaker,
            &["-c".to_owned(), "trap '' TERM; sleep 30".to_owned()],
            None,
            RequestDeadline::after(Duration::from_secs(2)),
            BreakerPolicy::Bypass,
        )
        .await;
        assert!(matches!(result, Err(HerdrError::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(5));
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
            state.opened_at = Some(Instant::now() - Duration::from_secs(31));
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
                parse_error(&format!(r#"{{"error":{{"code":"{code}"}}}}"#)),
                expected
            );
        }
    }

    #[test]
    fn breaker_opens_with_exponential_backoff_and_half_open_probe() {
        let breaker = HerdrSpawnBreaker::default();
        assert_eq!(breaker.state(), HerdrBreakerState::Closed);
        assert!(breaker.permits_spawn());
        breaker.record_infrastructure_failure();
        assert!(matches!(breaker.state(), HerdrBreakerState::Open { .. }));
        assert!(!breaker.permits_spawn());
        breaker.record_success();
        assert_eq!(breaker.state(), HerdrBreakerState::Closed);
    }
}
