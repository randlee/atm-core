//! Tokio-native, shell-free process boundary for the Herdr CLI.

use std::future::Future;
use std::num::NonZeroU32;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use atm_core::RequestDeadline;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::types::AgentName;
use serde_json::Value;

pub const HERDR_WAKE_TEXT: &str = "You have unread ATM messages. Run: atm read";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HerdrAgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HerdrPromptOutcome {
    Accepted,
    BlockedBeforeInput,
    TargetNotPresent,
    AdvisoryFailure { code: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrWaitOutcome {
    pub status: HerdrAgentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrGetOutcome {
    pub status: HerdrAgentStatus,
}

/// Shared adapter used by the immediate Herdr hook and the AQ2.7 queue pump.
pub trait HerdrProcessAdapter: Send + Sync {
    fn prompt(
        &self,
        agent: &AgentName,
        session: Option<&str>,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrPromptOutcome, AtmError>> + Send + '_>>;
    fn wait(
        &self,
        agent: &AgentName,
        session: Option<&str>,
        until: &[HerdrAgentStatus],
        timeout: Duration,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrWaitOutcome, AtmError>> + Send + '_>>;
    fn get(
        &self,
        agent: &AgentName,
        session: Option<&str>,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrGetOutcome, AtmError>> + Send + '_>>;
}

#[derive(Debug, Clone)]
pub struct HerdrSpawnBreaker {
    state: Arc<Mutex<BreakerState>>,
    failure_threshold: NonZeroU32,
    cooldown: Duration,
}

#[derive(Debug, Default)]
struct BreakerState {
    consecutive_failures: u32,
    opened_at: Option<Instant>,
}

impl Default for HerdrSpawnBreaker {
    fn default() -> Self {
        Self::new(
            NonZeroU32::new(3).expect("non-zero breaker threshold"),
            Duration::from_secs(5),
        )
    }
}

impl HerdrSpawnBreaker {
    #[must_use]
    pub fn new(failure_threshold: NonZeroU32, cooldown: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(BreakerState::default())),
            failure_threshold,
            cooldown,
        }
    }

    #[must_use]
    pub fn permits_spawn(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if let Some(opened_at) = state.opened_at {
            if opened_at.elapsed() < self.cooldown {
                return false;
            }
            state.opened_at = None;
            state.consecutive_failures = 0;
        }
        true
    }

    pub fn record_success(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.consecutive_failures = 0;
            state.opened_at = None;
        }
    }

    pub fn record_failure(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.consecutive_failures = state.consecutive_failures.saturating_add(1);
            if state.consecutive_failures >= self.failure_threshold.get() {
                state.opened_at = Some(Instant::now());
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct HerdrProcessInvoker {
    breaker: HerdrSpawnBreaker,
}

impl Default for HerdrProcessInvoker {
    fn default() -> Self {
        Self {
            breaker: HerdrSpawnBreaker::default(),
        }
    }
}

impl HerdrProcessInvoker {
    #[must_use]
    pub fn new(breaker: HerdrSpawnBreaker) -> Self {
        Self { breaker }
    }
}

impl HerdrProcessAdapter for HerdrProcessInvoker {
    fn prompt(
        &self,
        agent: &AgentName,
        session: Option<&str>,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrPromptOutcome, AtmError>> + Send + '_>> {
        let agent = agent.clone();
        let session = session.map(str::to_owned);
        let breaker = self.breaker.clone();
        Box::pin(async move {
            let output = run_command(
                &breaker,
                &["agent", "prompt", agent.as_str(), HERDR_WAKE_TEXT],
                session.as_deref(),
                deadline,
            )
            .await?;
            parse_prompt(output)
        })
    }

    fn wait(
        &self,
        agent: &AgentName,
        session: Option<&str>,
        until: &[HerdrAgentStatus],
        timeout: Duration,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrWaitOutcome, AtmError>> + Send + '_>> {
        let agent = agent.clone();
        let session = session.map(str::to_owned);
        let breaker = self.breaker.clone();
        let statuses = until
            .iter()
            .map(|status| status.as_str())
            .collect::<Vec<_>>();
        Box::pin(async move {
            let mut args = vec!["agent", "wait", agent.as_str()];
            for status in &statuses {
                args.push("--until");
                args.push(status);
            }
            let timeout_ms = timeout.as_millis().to_string();
            args.push("--timeout");
            args.push(&timeout_ms);
            let output = run_command(&breaker, &args, session.as_deref(), deadline).await?;
            if !output.success {
                return Err(parse_error(&output.stderr));
            }
            let status = parse_agent_status(&output.stdout)
                .ok_or_else(|| herdr_error("missing agent status in wait response"))?;
            Ok(HerdrWaitOutcome { status })
        })
    }

    fn get(
        &self,
        agent: &AgentName,
        session: Option<&str>,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrGetOutcome, AtmError>> + Send + '_>> {
        let agent = agent.clone();
        let session = session.map(str::to_owned);
        let breaker = self.breaker.clone();
        Box::pin(async move {
            let output = run_command(
                &breaker,
                &["agent", "get", agent.as_str()],
                session.as_deref(),
                deadline,
            )
            .await?;
            if !output.success {
                return Err(parse_error(&output.stderr));
            }
            let status = parse_agent_status(&output.stdout)
                .ok_or_else(|| herdr_error("missing agent status in get response"))?;
            Ok(HerdrGetOutcome { status })
        })
    }
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
    args: &[&str],
    session: Option<&str>,
    deadline: RequestDeadline,
) -> Result<CommandOutput, AtmError> {
    if !breaker.permits_spawn() {
        return Err(AtmError::new(
            AtmErrorCode::HerdrUnavailable,
            "Herdr process spawn breaker is open",
        ));
    }
    let remaining = deadline
        .remaining()
        .ok_or_else(|| herdr_error("Herdr process deadline expired"))?;
    let mut command = tokio::process::Command::new("herdr");
    command.args(args);
    if let Some(session) = session {
        command.env("HERDR_SESSION", session);
    }
    let output = tokio::time::timeout(remaining, command.output())
        .await
        .map_err(|_| {
            breaker.record_failure();
            herdr_error("Herdr process deadline expired")
        })?
        .map_err(|error| {
            breaker.record_failure();
            AtmError::new(AtmErrorCode::HerdrUnavailable, "failed to start Herdr CLI")
                .with_cause(error)
        })?;
    breaker.record_success();
    let result = CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        success: output.status.success(),
    };
    if result.success {
        return Ok(result);
    }
    if output.status.code() == Some(2) {
        return Err(AtmError::new(
            AtmErrorCode::InternalError,
            "Herdr CLI rejected an impossible ATM command invocation",
        ));
    }
    Ok(result)
}

fn parse_prompt(output: CommandOutput) -> Result<HerdrPromptOutcome, AtmError> {
    let envelope: Value = serde_json::from_str(if output.success {
        &output.stdout
    } else {
        &output.stderr
    })
    .map_err(|_| herdr_error("Herdr returned invalid prompt JSON"))?;
    if envelope.get("result").is_some() {
        return Ok(HerdrPromptOutcome::Accepted);
    }
    match envelope
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
    {
        Some("agent_blocked") => Ok(HerdrPromptOutcome::BlockedBeforeInput),
        Some("agent_not_found") => Ok(HerdrPromptOutcome::TargetNotPresent),
        Some(code) => Ok(HerdrPromptOutcome::AdvisoryFailure {
            code: code.to_owned(),
        }),
        None => Err(herdr_error(
            "Herdr prompt response contained neither result nor error",
        )),
    }
}

fn parse_agent_status(stdout: &str) -> Option<HerdrAgentStatus> {
    let envelope: Value = serde_json::from_str(stdout).ok()?;
    let status = envelope
        .get("result")?
        .get("agent")?
        .get("agent_status")?
        .as_str()?;
    Some(match status {
        "idle" => HerdrAgentStatus::Idle,
        "working" => HerdrAgentStatus::Working,
        "blocked" => HerdrAgentStatus::Blocked,
        "done" => HerdrAgentStatus::Done,
        _ => HerdrAgentStatus::Unknown,
    })
}

fn parse_error(stderr: &str) -> AtmError {
    let code = serde_json::from_str::<Value>(stderr)
        .ok()
        .and_then(|envelope| envelope.get("error").cloned())
        .and_then(|error| error.get("code").and_then(Value::as_str).map(str::to_owned));
    match code.as_deref() {
        Some("server_not_running") | Some("protocol_mismatch") => AtmError::new(
            AtmErrorCode::HerdrUnavailable,
            "Herdr server is unavailable",
        ),
        Some("agent_not_found") => AtmError::new(
            AtmErrorCode::HerdrAgentNotVisible,
            "agent is not visible in the configured Herdr session",
        ),
        Some(code) => AtmError::new(
            AtmErrorCode::HerdrPromptFailed,
            format!("Herdr command failed with {code}"),
        ),
        None => AtmError::new(
            AtmErrorCode::HerdrUnavailable,
            "Herdr returned no structured error",
        ),
    }
}

fn herdr_error(message: impl Into<String>) -> AtmError {
    AtmError::new(AtmErrorCode::HerdrPromptFailed, message)
}

#[cfg(feature = "test-utils")]
pub mod testing {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum FakeHerdrCall {
        Prompt {
            agent: String,
            session: Option<String>,
        },
        Wait {
            agent: String,
            session: Option<String>,
        },
        Get {
            agent: String,
            session: Option<String>,
        },
    }

    #[derive(Debug, Default, Clone)]
    pub struct FakeHerdrProcessAdapter {
        calls: Arc<Mutex<Vec<FakeHerdrCall>>>,
    }

    impl FakeHerdrProcessAdapter {
        #[must_use]
        pub fn calls(&self) -> Vec<FakeHerdrCall> {
            self.calls
                .lock()
                .map(|calls| calls.clone())
                .unwrap_or_default()
        }
    }

    impl HerdrProcessAdapter for FakeHerdrProcessAdapter {
        fn prompt(
            &self,
            agent: &AgentName,
            session: Option<&str>,
            _deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<HerdrPromptOutcome, AtmError>> + Send + '_>>
        {
            let _ = self.calls.lock().map(|mut calls| {
                calls.push(FakeHerdrCall::Prompt {
                    agent: agent.to_string(),
                    session: session.map(str::to_owned),
                });
            });
            Box::pin(async { Ok(HerdrPromptOutcome::Accepted) })
        }

        fn wait(
            &self,
            agent: &AgentName,
            session: Option<&str>,
            _until: &[HerdrAgentStatus],
            _timeout: Duration,
            _deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<HerdrWaitOutcome, AtmError>> + Send + '_>> {
            let _ = self.calls.lock().map(|mut calls| {
                calls.push(FakeHerdrCall::Wait {
                    agent: agent.to_string(),
                    session: session.map(str::to_owned),
                });
            });
            Box::pin(async {
                Ok(HerdrWaitOutcome {
                    status: HerdrAgentStatus::Idle,
                })
            })
        }

        fn get(
            &self,
            agent: &AgentName,
            session: Option<&str>,
            _deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<HerdrGetOutcome, AtmError>> + Send + '_>> {
            let _ = self.calls.lock().map(|mut calls| {
                calls.push(FakeHerdrCall::Get {
                    agent: agent.to_string(),
                    session: session.map(str::to_owned),
                });
            });
            Box::pin(async {
                Ok(HerdrGetOutcome {
                    status: HerdrAgentStatus::Idle,
                })
            })
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
    fn prompt_outcomes_parse_structured_codes() {
        assert_eq!(
            parse_prompt(CommandOutput {
                stdout: r#"{"result":{"type":"agent_prompted"}}"#.into(),
                stderr: String::new(),
                success: true,
            })
            .unwrap(),
            HerdrPromptOutcome::Accepted
        );
        assert_eq!(
            parse_prompt(CommandOutput {
                stdout: String::new(),
                stderr: r#"{"error":{"code":"agent_blocked"}}"#.into(),
                success: false,
            })
            .unwrap(),
            HerdrPromptOutcome::BlockedBeforeInput
        );
    }

    #[test]
    fn breaker_opens_after_consecutive_failures() {
        let breaker = HerdrSpawnBreaker::new(
            NonZeroU32::new(2).expect("threshold"),
            Duration::from_secs(60),
        );
        assert!(breaker.permits_spawn());
        breaker.record_failure();
        assert!(breaker.permits_spawn());
        breaker.record_failure();
        assert!(!breaker.permits_spawn());
        breaker.record_success();
        assert!(breaker.permits_spawn());
    }
}
