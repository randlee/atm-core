//! Daemon-owned execution of task-hook modules.
//!
//! Hook children from task chains, cron runs, and doctor requests share one
//! bounded permit pool so a misbehaving registration cannot exhaust daemon
//! process capacity.

use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use atm_core::api::RequestDeadline;
use atm_core::atm_temp::EnvSource;
use atm_core::boundary::{
    CronHookRunner, HOOK_ACTION_RESERVE_MS, HookExitCause, HookOutcome, HookRunner, HookTarget,
    PhaseId, sealed,
};
use atm_core::doctor::hooks::{HookDoctorReply, HookDoctorRequest};
use atm_core::error::AtmError;
use atm_core::hook::{
    CronHookOutcome, CronStopReply, HookReply, HookRequest, StopReply, parse_cron_stop_reply,
    parse_cron_tick_reply,
};
use atm_core::types::TeamName;
use tokio::sync::Semaphore;
use tracing::Instrument;

use crate::process_deadline::{SpawnFailure, SpawnSpec, SpawnedOutput, spawn_with_deadline};

/// Maximum concurrently running hook children for one daemon.
///
/// Sixteen permits leave room for normal daemon work while supporting several
/// independent task chains, cron runs, and doctor requests at once.
pub const HOOK_CONCURRENCY_MAX: usize = 16;

/// Runs registered hook modules in the daemon's constrained subprocess
/// environment.
#[derive(Debug)]
pub struct DaemonHookRunner {
    base_env: Result<Vec<(OsString, OsString)>, AtmError>,
    python: Result<PathBuf, AtmError>,
    permits: Arc<Semaphore>,
}

fn missing(detail: impl Into<String>) -> AtmError {
    AtmError::hook_exit(HookExitCause::Missing(detail.into()))
}

struct CapturedEnv<'a>(&'a [(OsString, OsString)]);

impl EnvSource for CapturedEnv<'_> {
    fn var(&self, key: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .and_then(|(_, value)| value.to_str().map(str::to_owned))
    }
}

// On Windows `ProcessEnvSource` (std::env::var) matches names case-insensitively, so `Path` satisfies `PATH`.

fn base_environment(
    env: &dyn EnvSource,
    windows: bool,
) -> Result<Vec<(OsString, OsString)>, AtmError> {
    let keys: &[&str] = if windows {
        &["PATH", "HOME", "SystemRoot", "USERPROFILE"]
    } else {
        &["PATH", "HOME"]
    };
    let mut values = Vec::new();
    for key in keys {
        match env.var(key) {
            Some(value) => values.push((OsString::from(key), OsString::from(value))),
            None if windows && *key == "HOME" => {}
            None => return Err(missing(format!("{key} missing"))),
        }
    }
    Ok(values)
}

pub(crate) fn hook_env(
    env: &dyn EnvSource,
    team: &TeamName,
    phase: &PhaseId,
    windows: bool,
) -> Result<Vec<(OsString, OsString)>, AtmError> {
    let mut values = base_environment(env, windows)?;
    values.push(("ATM_TEAM".into(), team.as_str().into()));
    values.push(("ATM_PHASE".into(), phase.as_str().into()));
    Ok(values)
}

fn executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        path.metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn resolve_python(base: &[(OsString, OsString)], env: &dyn EnvSource) -> Result<PathBuf, AtmError> {
    let path = base
        .iter()
        .find(|(key, _)| key == "PATH")
        .map(|(_, value)| value)
        .ok_or_else(|| missing("PATH missing"))?;
    let name = if cfg!(windows) { "python" } else { "python3" };
    let extensions = if cfg!(windows) {
        env.var("PATHEXT")
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    } else {
        vec![String::new()]
    };
    for directory in std::env::split_paths(path) {
        for extension in &extensions {
            let candidate = directory.join(format!("{name}{extension}"));
            if executable(&candidate) {
                return candidate
                    .canonicalize()
                    .map_err(|error| missing(format!("cannot resolve {name}: {error}")));
            }
        }
    }
    Err(missing(format!("{name} not found on PATH")))
}

impl DaemonHookRunner {
    /// Captures the daemon environment and configures the production permit
    /// limit for hook children.
    pub fn new(env: &dyn EnvSource) -> Self {
        Self::with_limit(env, HOOK_CONCURRENCY_MAX)
    }

    pub(crate) fn with_limit(env: &dyn EnvSource, permits: usize) -> Self {
        let base_env = base_environment(env, cfg!(windows));
        let python = base_env
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|base| resolve_python(base, env));
        Self {
            base_env,
            python,
            permits: Arc::new(Semaphore::new(permits)),
        }
    }

    async fn spec(&self, target: &HookTarget, hook: &str) -> Result<SpawnSpec, AtmError> {
        for (path, directory) in [
            (target.cwd.clone(), true),
            (target.cwd.join(&target.module_path), false),
        ] {
            let metadata = tokio::fs::metadata(&path).await.map_err(|error| {
                AtmError::hook_exit(HookExitCause::Spawn {
                    path: path.clone(),
                    cause: error.to_string(),
                })
            })?;
            if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
                return Err(AtmError::hook_exit(HookExitCause::Spawn {
                    path,
                    cause: if directory {
                        "not a directory"
                    } else {
                        "not a regular file"
                    }
                    .into(),
                }));
            }
        }
        let env = hook_env(
            &CapturedEnv(self.base_env.as_ref().map_err(Clone::clone)?),
            &target.team,
            &target.phase,
            cfg!(windows),
        )?;
        Ok(SpawnSpec {
            program: self.python.clone()?,
            args: vec![target.module_path.as_os_str().into(), hook.into()],
            env,
            cwd: target.cwd.clone(),
        })
    }

    async fn execute(
        &self,
        target: &HookTarget,
        hook: &str,
        stdin: Vec<u8>,
        deadline: RequestDeadline,
        doctor: bool,
    ) -> Result<SpawnedOutput, AtmError> {
        let started_at = tokio::time::Instant::now();
        let remaining = deadline.remaining().unwrap_or_default();
        let budget = if doctor {
            doctor_budget(target.timeout_ms, remaining)
        } else {
            runner_budget(target.timeout_ms, remaining)
        }
        .ok_or_else(|| {
            let error = AtmError::hook_timeout(
                &target.phase,
                if doctor {
                    "doctor budget exhausted"
                } else {
                    "request budget exhausted before the hook ran"
                },
            );
            tracing::warn!(
                subsystem = "atm_hook",
                team = %target.team,
                phase = %target.phase,
                hook,
                budget_ms = target.timeout_ms,
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                outcome = "budget_exhausted",
                "hook execution did not receive a budget"
            );
            error
        })?;
        let started = tokio::time::Instant::now();
        let _permit = tokio::time::timeout(budget, self.permits.clone().acquire_owned())
            .await
            .map_err(|_| {
                tracing::warn!(
                    subsystem = "atm_hook", team = %target.team, phase = %target.phase, hook,
                    budget_ms = budget.as_millis() as u64,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    outcome = "permit_timeout", "hook concurrency limit reached"
                );
                AtmError::hook_timeout(&target.phase, "hook concurrency limit reached")
            })?
            .map_err(|_| missing("hook semaphore closed"))?;
        let spec = self.spec(target, hook).await?;
        let remaining = remaining_spawn_budget(&target.phase, budget, started.elapsed())?;
        let output = spawn_with_deadline(spec, &stdin, remaining)
            .instrument(hook_spawn_span(target, hook, budget))
            .await
            .map_err(|failure| {
                let error = spawn_failure_error(&target.phase, target.timeout_ms, budget, failure);
                tracing::warn!(
                    subsystem = "atm_hook", team = %target.team, phase = %target.phase, hook,
                    budget_ms = budget.as_millis() as u64,
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                    outcome = "failed", %error, "hook execution failed"
                );
                error
            })?;
        check_status(&output).map_err(|error| {
            tracing::warn!(
                subsystem = "atm_hook", team = %target.team, phase = %target.phase, hook,
                budget_ms = budget.as_millis() as u64, elapsed_ms = output.elapsed_ms,
                outcome = "nonzero_exit", %error, "hook execution exited unsuccessfully"
            );
            error
        })?;
        tracing::info!(
            subsystem = "atm_hook", team = %target.team, phase = %target.phase, hook,
            budget_ms = budget.as_millis() as u64, elapsed_ms = output.elapsed_ms,
            outcome = "ok", "hook execution completed"
        );
        Ok(output)
    }
}

fn hook_spawn_span(target: &HookTarget, hook: &str, budget: Duration) -> tracing::Span {
    tracing::info_span!(
        "atm_hook.spawn",
        team = %target.team,
        phase = %target.phase,
        hook,
        budget_ms = budget.as_millis() as u64,
    )
}

fn spawn_failure_error(
    phase: &PhaseId,
    timeout_ms: u32,
    budget: Duration,
    failure: SpawnFailure,
) -> AtmError {
    match failure {
        SpawnFailure::TimedOut { after_ms } => {
            let limit = if budget < Duration::from_millis(u64::from(timeout_ms)) {
                "request deadline"
            } else {
                "configured timeout"
            };
            AtmError::hook_timeout(
                phase,
                &format!("no reply within {after_ms} ms (limited by {limit})"),
            )
        }
        SpawnFailure::Failed(error) => error,
    }
}

fn check_status(output: &SpawnedOutput) -> Result<(), AtmError> {
    if output.status.success() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        if let Some(signal) = output.status.signal() {
            return Err(AtmError::hook_exit(HookExitCause::Signal(signal)));
        }
    }
    let tail = &output.stderr[output.stderr.len().saturating_sub(512)..];
    Err(AtmError::hook_exit(HookExitCause::Exit {
        code: output.status.code().unwrap_or(-1),
        stderr_tail: String::from_utf8_lossy(tail).into_owned(),
    }))
}

impl sealed::Sealed for DaemonHookRunner {}

#[async_trait]
impl HookRunner for DaemonHookRunner {
    async fn run(
        &self,
        target: &HookTarget,
        request: &HookRequest,
        deadline: RequestDeadline,
    ) -> Result<HookOutcome, AtmError> {
        let stdin = serde_json::to_vec(request)
            .map_err(|error| AtmError::hook_malformed(&error.to_string()))?;
        let output = self
            .execute(target, request.hook.as_str(), stdin, deadline, false)
            .await?;
        Ok(HookOutcome {
            reply: HookReply::parse(&output.stdout)?,
            elapsed_ms: output.elapsed_ms,
        })
    }

    async fn doctor(
        &self,
        target: &HookTarget,
        deadline: RequestDeadline,
    ) -> Result<HookDoctorReply, AtmError> {
        let stdin = serde_json::to_vec(&HookDoctorRequest::for_target(target))
            .map_err(|error| AtmError::hook_malformed(&error.to_string()))?;
        let output = self
            .execute(target, "doctor", stdin, deadline, true)
            .await?;
        HookDoctorReply::parse(&output.stdout)
    }

    async fn stop(
        &self,
        target: &HookTarget,
        request: &HookRequest,
        deadline: RequestDeadline,
    ) -> Result<StopReply, AtmError> {
        let stdin = serde_json::to_vec(request)
            .map_err(|error| AtmError::hook_malformed(&error.to_string()))?;
        let output = self
            .execute(target, request.hook.as_str(), stdin, deadline, false)
            .await?;
        StopReply::parse(&output.stdout)
    }
}

#[async_trait]
impl CronHookRunner for DaemonHookRunner {
    async fn run_cron(
        &self,
        target: &HookTarget,
        request: &HookRequest,
        deadline: RequestDeadline,
    ) -> Result<CronHookOutcome, AtmError> {
        let stdin = serde_json::to_vec(request)
            .map_err(|error| AtmError::hook_malformed(&error.to_string()))?;
        let output = self
            .execute(target, request.hook.as_str(), stdin, deadline, false)
            .await?;
        Ok(CronHookOutcome {
            reply: parse_cron_tick_reply(&output.stdout)?,
            elapsed_ms: output.elapsed_ms,
        })
    }

    async fn stop_cron(
        &self,
        target: &HookTarget,
        request: &HookRequest,
        deadline: RequestDeadline,
    ) -> Result<CronStopReply, AtmError> {
        let stdin = serde_json::to_vec(request)
            .map_err(|error| AtmError::hook_malformed(&error.to_string()))?;
        let output = self
            .execute(target, request.hook.as_str(), stdin, deadline, false)
            .await?;
        parse_cron_stop_reply(&output.stdout)
    }
}

pub(crate) fn runner_budget(timeout_ms: u32, remaining: Duration) -> Option<Duration> {
    doctor_budget(
        timeout_ms,
        remaining.checked_sub(Duration::from_millis(u64::from(HOOK_ACTION_RESERVE_MS)))?,
    )
}

pub(crate) fn doctor_budget(timeout_ms: u32, remaining: Duration) -> Option<Duration> {
    let budget = remaining.min(Duration::from_millis(timeout_ms.into()));
    (!budget.is_zero()).then_some(budget)
}

fn remaining_spawn_budget(
    phase: &PhaseId,
    budget: Duration,
    elapsed: Duration,
) -> Result<Duration, AtmError> {
    budget
        .checked_sub(elapsed)
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| AtmError::hook_timeout(phase, "budget exhausted before spawn"))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::Mutex;

    use atm_core::atm_temp::ProcessEnvSource;
    use atm_core::boundary::{CronSchedule, HookName, HookRegistration};
    use atm_core::error::AtmErrorCode;
    use atm_core::hook::{HOOK_SCHEMA_VERSION, HookCaller};
    use serde_json::{Value, json};
    use tempfile::TempDir;
    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id};
    use tracing::{Event, Subscriber};
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::registry::LookupSpan;

    use super::*;

    #[derive(Clone, Default)]
    struct CapturedFields(BTreeMap<String, String>);

    impl Visit for CapturedFields {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.insert(field.name().into(), format!("{value:?}"));
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.insert(field.name().into(), value.into());
        }

        fn record_u64(&mut self, field: &Field, value: u64) {
            self.0.insert(field.name().into(), value.to_string());
        }
    }

    #[derive(Clone, Default)]
    struct KillEventCapture(Arc<Mutex<Vec<CapturedFields>>>);

    impl<S> Layer<S> for KillEventCapture
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
            let mut fields = CapturedFields::default();
            attrs.record(&mut fields);
            ctx.span(id)
                .expect("new span is registered")
                .extensions_mut()
                .insert(fields);
        }

        fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
            let mut fields = CapturedFields::default();
            if let Some(scope) = ctx.event_scope(event) {
                for span in scope.from_root() {
                    if let Some(span_fields) = span.extensions().get::<CapturedFields>() {
                        fields.0.extend(span_fields.0.clone());
                    }
                }
            }
            event.record(&mut fields);
            if fields
                .0
                .get("outcome")
                .is_some_and(|value| value == "killed")
            {
                self.0.lock().expect("capture lock").push(fields);
            }
        }
    }

    struct TestEnv(BTreeMap<String, String>);

    fn fold_lookup(values: &BTreeMap<String, String>, key: &str) -> Option<String> {
        values
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.clone())
    }

    impl EnvSource for TestEnv {
        fn var(&self, key: &str) -> Option<String> {
            if cfg!(windows) {
                fold_lookup(&self.0, key)
            } else {
                self.0.get(key).cloned()
            }
        }
    }

    struct WindowsEnv(BTreeMap<String, String>);

    impl EnvSource for WindowsEnv {
        fn var(&self, key: &str) -> Option<String> {
            fold_lookup(&self.0, key)
        }
    }

    struct CountingEnv {
        inner: WindowsEnv,
        calls: Cell<usize>,
    }

    impl EnvSource for CountingEnv {
        fn var(&self, key: &str) -> Option<String> {
            self.calls.set(self.calls.get() + 1);
            self.inner.var(key)
        }
    }

    struct Fixture {
        _root: TempDir,
        target: HookTarget,
        env: TestEnv,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let cwd = root.path().canonicalize().unwrap();
            let module_path = cwd.join("fixture_hook.py");
            fs::write(
                &module_path,
                include_str!("../tests/hook_fixture/fixture_hook.py"),
            )
            .unwrap();
            fs::write(
                cwd.join("fixture_reply.json"),
                r#"{"result":"pass-through"}"#,
            )
            .unwrap();
            let mut values = BTreeMap::from([
                (
                    "PATH".into(),
                    ProcessEnvSource.var("PATH").expect("test PATH"),
                ),
                ("HOME".into(), cwd.to_string_lossy().into_owned()),
                ("ATM_BK3_LEAK_SENTINEL".into(), "1".into()),
            ]);
            if cfg!(windows) {
                for key in ["SystemRoot", "USERPROFILE"] {
                    values.insert(
                        key.into(),
                        ProcessEnvSource.var(key).expect("Windows environment"),
                    );
                }
            }
            Self {
                _root: root,
                env: TestEnv(values),
                target: HookTarget {
                    team: "test-team".parse().unwrap(),
                    phase: PhaseId::parse("bk").unwrap(),
                    module_path,
                    cwd,
                    timeout_ms: 30_000,
                    custom: json!({"goal":"ship", "n":[1,{"y":null}]})
                        .as_object()
                        .unwrap()
                        .clone(),
                },
            }
        }

        fn request(&self, hook: &str) -> HookRequest {
            HookRequest {
                schema_version: HOOK_SCHEMA_VERSION.into(),
                hook: HookName::parse(hook).unwrap(),
                team: self.target.team.clone(),
                phase: self.target.phase.clone(),
                caller: HookCaller {
                    identity: "test-agent".parse().unwrap(),
                    team: self.target.team.clone(),
                    host: None,
                },
                request: json!({"args": ["check"], "created_by": "test-agent"}),
                custom: self.target.custom.clone(),
            }
        }

        fn runner(&self) -> DaemonHookRunner {
            DaemonHookRunner::new(&self.env)
        }
        fn script(&self, source: &str) {
            fs::write(&self.target.module_path, source).unwrap();
        }
        fn reply(&self, reply: Value) {
            fs::write(
                self.target.cwd.join("fixture_reply.json"),
                serde_json::to_vec(&reply).unwrap(),
            )
            .unwrap();
        }
        fn json(&self, name: &str) -> Value {
            serde_json::from_slice(&fs::read(self.target.cwd.join(name)).unwrap()).unwrap()
        }
        fn no_spawn(&self) {
            assert!(!self.target.cwd.join("fixture_request.json").exists());
        }
        async fn run(&self) -> Result<HookOutcome, AtmError> {
            self.runner()
                .run(
                    &self.target,
                    &self.request("task.close"),
                    RequestDeadline::after(Duration::from_secs(60)),
                )
                .await
        }
    }

    #[test]
    fn runner_debug_reports_its_type() {
        let fixture = Fixture::new();
        assert!(format!("{:?}", fixture.runner()).starts_with("DaemonHookRunner"));
    }

    #[test]
    fn runner_budget_rules() {
        for (timeout, remaining, expected) in [
            (2500, 10000, Some(2500)),
            (2500, 2000, Some(1500)),
            (30000, 600, Some(100)),
            (2500, 500, None),
            (2500, 0, None),
        ] {
            assert_eq!(
                runner_budget(timeout, Duration::from_millis(remaining)),
                expected.map(Duration::from_millis)
            );
        }
    }

    #[test]
    fn doctor_budget_has_no_reserve() {
        for (timeout, remaining, expected) in [
            (2500, 600, Some(600)),
            (2500, 10000, Some(2500)),
            (2500, 0, None),
        ] {
            assert_eq!(
                doctor_budget(timeout, Duration::from_millis(remaining)),
                expected.map(Duration::from_millis)
            );
        }
    }

    #[test]
    fn exhausted_post_permit_budget_reports_before_spawn() {
        let phase = PhaseId::parse("bk").unwrap();
        let error =
            remaining_spawn_budget(&phase, Duration::from_millis(1), Duration::from_millis(1))
                .unwrap_err();
        assert_eq!(
            error.message(),
            AtmError::hook_timeout(&phase, "budget exhausted before spawn").message()
        );
    }

    #[test]
    fn hook_env_rules() {
        let team = "test-team".parse().unwrap();
        let phase = PhaseId::parse("bk").unwrap();
        let unix = BTreeMap::from([
            ("PATH".into(), "/bin".into()),
            ("HOME".into(), "/home/test".into()),
        ]);
        assert_eq!(
            hook_env(&TestEnv(unix.clone()), &team, &phase, false)
                .unwrap()
                .len(),
            4
        );
        for key in ["PATH", "HOME"] {
            let mut values = unix.clone();
            values.remove(key);
            let error = hook_env(&TestEnv(values), &team, &phase, false).unwrap_err();
            assert_eq!(error.code(), AtmErrorCode::HookExit);
            assert!(error.message().contains(key));
        }
        let windows = BTreeMap::from([
            ("Path".into(), "C:\\Python".into()),
            ("SystemRoot".into(), "C:\\Windows".into()),
            ("USERPROFILE".into(), "C:\\Users\\test".into()),
        ]);
        let captured: BTreeMap<_, _> = hook_env(&WindowsEnv(windows.clone()), &team, &phase, true)
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(
            captured.get(&OsString::from("PATH")),
            Some(&OsString::from("C:\\Python"))
        );
        assert!(!captured.contains_key(&OsString::from("HOME")));
        for key in ["SystemRoot", "USERPROFILE", "Path"] {
            let mut values = windows.clone();
            values.remove(key);
            assert_eq!(
                hook_env(&WindowsEnv(values), &team, &phase, true)
                    .unwrap_err()
                    .code(),
                AtmErrorCode::HookExit
            );
        }
    }

    #[test]
    fn windows_env_lookup_is_one_call_per_key() {
        let team = "test-team".parse().unwrap();
        let phase = PhaseId::parse("bk").unwrap();
        let env = CountingEnv {
            inner: WindowsEnv(BTreeMap::from([
                ("pAtH".into(), "C:\\Py".into()),
                ("systemroot".into(), "C:\\Windows".into()),
                ("UserProfile".into(), "C:\\Users\\t".into()),
            ])),
            calls: Cell::new(0),
        };

        let actual: BTreeMap<_, _> = hook_env(&env, &team, &phase, true)
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(env.calls.get(), 4);
        assert_eq!(
            actual,
            BTreeMap::from([
                (OsString::from("PATH"), OsString::from("C:\\Py")),
                (OsString::from("SystemRoot"), OsString::from("C:\\Windows")),
                (
                    OsString::from("USERPROFILE"),
                    OsString::from("C:\\Users\\t"),
                ),
                (OsString::from("ATM_TEAM"), OsString::from("test-team")),
                (OsString::from("ATM_PHASE"), OsString::from("bk")),
            ])
        );
        assert_eq!(env.var(&"X".repeat(70)), None);
    }

    #[tokio::test]
    async fn configured_environment_has_only_explicit_contract_keys() {
        let fixture = Fixture::new();
        let spec = fixture
            .runner()
            .spec(&fixture.target, "task.close")
            .await
            .unwrap();
        let actual: BTreeMap<_, _> = spec.env.into_iter().collect();
        let mut expected = BTreeMap::from([
            (
                OsString::from("PATH"),
                fixture.env.var("PATH").unwrap().into(),
            ),
            ("HOME".into(), fixture.env.var("HOME").unwrap().into()),
            ("ATM_TEAM".into(), "test-team".into()),
            ("ATM_PHASE".into(), "bk".into()),
        ]);
        if cfg!(windows) {
            for key in ["SystemRoot", "USERPROFILE"] {
                expected.insert(key.into(), fixture.env.var(key).unwrap().into());
            }
        }
        assert_eq!(actual, expected);
        assert_eq!(
            spec.args,
            [
                fixture.target.module_path.as_os_str(),
                std::ffi::OsStr::new("task.close")
            ]
        );
        assert_eq!(spec.cwd, fixture.target.cwd);
    }

    #[tokio::test]
    async fn stdin_is_the_request_and_custom_is_byte_identical() {
        let fixture = Fixture::new();
        let request = fixture.request("task.close");
        assert_eq!(fixture.run().await.unwrap().reply, HookReply::PassThrough);
        let raw = fs::read(fixture.target.cwd.join("fixture_request.json")).unwrap();
        assert_eq!(raw, serde_json::to_vec(&request).unwrap());
        assert_eq!(
            serde_json::to_string(&fixture.json("fixture_request.json")["custom"]).unwrap(),
            serde_json::to_string(&fixture.target.custom).unwrap()
        );
        assert_eq!(fixture.json("fixture_argv.json"), json!(["task.close"]));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runner_passes_daemon_path_and_home_values() {
        let output = environment_probe_output().await;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("BK3_ENVIRONMENT_PROOF_COMPLETE"));
    }

    async fn environment_probe_output() -> SpawnedOutput {
        let mut env: Vec<_> = std::env::vars_os()
            .filter(|(name, _)| name != "ATM_BK3_LEAK_SENTINEL")
            .collect();
        env.push((OsString::from("ATM_BK3_LEAK_SENTINEL"), OsString::from("1")));
        spawn_with_deadline(
            SpawnSpec {
                program: std::env::current_exe().unwrap(),
                args: vec![
                    "--exact".into(),
                    "hook_runner::tests::environment_child_probe".into(),
                    "--ignored".into(),
                    "--nocapture".into(),
                ],
                env,
                cwd: std::env::current_dir().unwrap(),
            },
            &[],
            Duration::from_secs(60),
        )
        .await
        .unwrap()
    }

    #[ignore = "runs only in the sentinel child process"]
    #[tokio::test]
    async fn environment_child_probe() {
        if ProcessEnvSource.var("ATM_BK3_LEAK_SENTINEL").as_deref() != Some("1") {
            return;
        }
        let fixture = Fixture::new();
        fixture.run().await.unwrap();
        let environment = fixture.json("fixture_env.json");
        for key in ["PATH", "HOME"] {
            assert_eq!(environment[key], fixture.env.var(key).unwrap());
        }
        assert_eq!(environment["ATM_TEAM"], "test-team");
        assert_eq!(environment["ATM_PHASE"], "bk");
        assert!(environment.get("ATM_BK3_LEAK_SENTINEL").is_none());
        assert_eq!(
            Path::new(&fs::read_to_string(fixture.target.cwd.join("fixture_cwd.txt")).unwrap())
                .canonicalize()
                .unwrap(),
            fixture.target.cwd.canonicalize().unwrap()
        );
        println!("BK3_ENVIRONMENT_PROOF_COMPLETE");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_runner_works_without_home_and_with_mixed_case_path() {
        let mut fixture = Fixture::new();
        fixture.env.0.remove("HOME");
        let path = fixture.env.0.remove("PATH").unwrap();
        fixture.env.0.insert("Path".into(), path);
        assert_eq!(fixture.run().await.unwrap().reply, HookReply::PassThrough);
    }

    #[tokio::test]
    async fn nonzero_exit_returns_hook_exit_with_stderr_tail() {
        let fixture = Fixture::new();
        fixture
            .script("import sys\nsys.stderr.write('prefix' + 'x' * 600 + 'TAIL')\nsys.exit(3)\n");
        let error = fixture.run().await.unwrap_err();
        assert_eq!(error.code(), AtmErrorCode::HookExit);
        assert!(error.message().starts_with("HOOK.EXIT: exit 3: "));
        assert_eq!(
            error.message(),
            AtmError::hook_exit(HookExitCause::Exit {
                code: 3,
                stderr_tail: format!("{}TAIL", "x".repeat(508)),
            })
            .message()
        );
        assert!(!error.message().contains("prefix"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn hook_killed_by_signal_reports_signal() {
        let fixture = Fixture::new();
        fixture.script("import os,signal\nos.kill(os.getpid(),signal.SIGKILL)\n");
        assert_eq!(
            fixture.run().await.unwrap_err().message(),
            AtmError::hook_exit(HookExitCause::Signal(9)).message()
        );
    }

    #[tokio::test]
    async fn module_exception_returns_hook_exit_with_traceback_tail() {
        let fixture = Fixture::new();
        fixture.script("raise ValueError('boom')\n");
        let error = fixture.run().await.unwrap_err();
        assert_eq!(error.code(), AtmErrorCode::HookExit);
        assert!(error.message().contains("ValueError: boom"));
    }

    #[tokio::test]
    async fn not_json_stdout_is_malformed() {
        let fixture = Fixture::new();
        fixture.script("print('not json')\n");
        assert_eq!(
            fixture.run().await.unwrap_err().code(),
            AtmErrorCode::HookMalformed
        );
    }

    #[tokio::test]
    async fn python3_missing_returns_hook_exit() {
        let mut fixture = Fixture::new();
        fixture
            .env
            .0
            .insert("PATH".into(), fixture.target.cwd.to_str().unwrap().into());
        let error = fixture.run().await.unwrap_err();
        assert_eq!(error.code(), AtmErrorCode::HookExit);
        assert!(error.message().contains(if cfg!(windows) {
            "python not found on PATH"
        } else {
            "python3 not found on PATH"
        }));
        fixture.no_spawn();
    }

    #[tokio::test]
    async fn module_path_missing_returns_hook_exit() {
        let fixture = Fixture::new();
        fs::remove_file(&fixture.target.module_path).unwrap();
        let error = fixture.run().await.unwrap_err();
        assert!(error.message().starts_with(&format!(
            "HOOK.EXIT: cannot run {}: ",
            fixture.target.module_path.display()
        )));
        fixture.no_spawn();
    }

    #[tokio::test]
    async fn cwd_missing_returns_hook_exit() {
        let mut fixture = Fixture::new();
        fixture.target.cwd = fixture.target.cwd.join("missing");
        assert!(
            fixture
                .run()
                .await
                .unwrap_err()
                .message()
                .starts_with(&format!(
                    "HOOK.EXIT: cannot run {}: ",
                    fixture.target.cwd.display()
                ))
        );
        fixture.no_spawn();
    }

    #[tokio::test(start_paused = true)]
    async fn zero_remaining_budget_returns_timeout_without_spawn() {
        let fixture = Fixture::new();
        let error = fixture
            .runner()
            .run(
                &fixture.target,
                &fixture.request("task.close"),
                RequestDeadline::after(Duration::ZERO),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error.message(),
            AtmError::hook_timeout(
                &fixture.target.phase,
                "request budget exhausted before the hook ran"
            )
            .message()
        );
        fixture.no_spawn();
    }

    #[tokio::test(start_paused = true)]
    async fn doctor_expired_deadline_times_out_without_spawn() {
        let fixture = Fixture::new();
        let error = fixture
            .runner()
            .doctor(&fixture.target, RequestDeadline::after(Duration::ZERO))
            .await
            .unwrap_err();
        assert_eq!(
            error.message(),
            AtmError::hook_timeout(&fixture.target.phase, "doctor budget exhausted").message()
        );
        fixture.no_spawn();
    }

    #[tokio::test(start_paused = true)]
    async fn saturated_runner_times_out_without_spawn() {
        let mut fixture = Fixture::new();
        fixture.target.timeout_ms = 2500;
        let error = DaemonHookRunner::with_limit(&fixture.env, 0)
            .run(
                &fixture.target,
                &fixture.request("task.close"),
                RequestDeadline::after(Duration::from_secs(10)),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error.message(),
            AtmError::hook_timeout(&fixture.target.phase, "hook concurrency limit reached")
                .message()
        );
        fixture.no_spawn();
    }

    #[tokio::test]
    async fn runner_timeout_returns_hook_timeout() {
        let mut fixture = Fixture::new();
        // Leave about 1.1 s of runner budget after the 500 ms action reserve.
        // This is deliberately smaller than the configured timeout so the
        // real process failure proves execute passed its effective budget
        // through, with enough startup headroom for the child process.
        fixture.target.timeout_ms = 5_000;
        fixture.script("import threading\nthreading.Event().wait()\n");
        let error = fixture
            .runner()
            .run(
                &fixture.target,
                &fixture.request("task.close"),
                RequestDeadline::after(Duration::from_millis(1_600)),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), AtmErrorCode::HookTimeout);
        assert!(error.message().starts_with("HOOK.TIMEOUT: phase bk: "));
        assert!(
            error.message().contains("(limited by request deadline)"),
            "expected the effective request budget to limit the process timeout, got: {}",
            error.message()
        );
    }

    #[test]
    fn timeout_reports_effective_budget_and_binding_limit() {
        let phase = PhaseId::parse("bk").unwrap();
        let configured_timeout = runner_budget(200, Duration::from_secs(60)).unwrap();
        assert_eq!(configured_timeout, Duration::from_millis(200));
        let configured_timeout_error = spawn_failure_error(
            &phase,
            200,
            configured_timeout,
            SpawnFailure::TimedOut { after_ms: 200 },
        );
        assert_eq!(
            configured_timeout_error.message(),
            concat!(
                "HOOK.TIMEOUT: phase bk: no reply within 200 ms (limited by configured timeout)\n",
                "  Recovery: Make the hook reply within its timeout or raise the timeout within the budget, then retry."
            )
        );

        let request_deadline = runner_budget(30000, Duration::from_millis(600)).unwrap();
        assert_eq!(request_deadline, Duration::from_millis(100));
        let request_deadline_error = spawn_failure_error(
            &phase,
            30000,
            request_deadline,
            SpawnFailure::TimedOut { after_ms: 75 },
        );
        assert_eq!(
            request_deadline_error.message(),
            concat!(
                "HOOK.TIMEOUT: phase bk: no reply within 75 ms (limited by request deadline)\n",
                "  Recovery: Make the hook reply within its timeout or raise the timeout within the budget, then retry."
            )
        );
    }

    #[test]
    fn spawn_failure_keeps_non_timeout_error_unchanged() {
        let phase = PhaseId::parse("bk").unwrap();
        let expected = AtmError::hook_malformed("invalid reply");
        let actual = spawn_failure_error(
            &phase,
            30000,
            Duration::from_millis(100),
            SpawnFailure::Failed(expected.clone()),
        );
        assert_eq!(actual.code(), expected.code());
        assert_eq!(actual.message(), expected.message());
    }

    #[tokio::test]
    async fn module_exiting_without_reading_stdin_reports_its_exit_code() {
        let mut fixture = Fixture::new();
        fixture
            .target
            .custom
            .insert("padding".into(), Value::String("x".repeat(128 * 1024)));
        fixture.script("import sys\nsys.exit(3)\n");
        assert!(
            fixture
                .run()
                .await
                .unwrap_err()
                .message()
                .starts_with("HOOK.EXIT: exit 3")
        );
    }

    #[tokio::test]
    async fn module_never_reading_stdin_times_out() {
        let mut fixture = Fixture::new();
        fixture.target.timeout_ms = 200;
        fixture
            .target
            .custom
            .insert("padding".into(), Value::String("x".repeat(128 * 1024)));
        fixture.script("import threading\nthreading.Event().wait()\n");
        assert_eq!(
            fixture.run().await.unwrap_err().code(),
            AtmErrorCode::HookTimeout
        );
    }

    #[tokio::test]
    async fn stdout_over_cap_is_malformed() {
        let fixture = Fixture::new();
        fixture.script("import sys,threading\nsys.stdout.write('x' * ((1 << 20) + 1))\nsys.stdout.flush()\nthreading.Event().wait()\n");
        let error = fixture.run().await.unwrap_err();
        assert_eq!(error.code(), AtmErrorCode::HookMalformed);
        assert!(error.message().contains("stdout exceeds"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn spawn_until_kill_event_inherits_hook_fields() {
        let capture = KillEventCapture::default();
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let fixture = Fixture::new();
        fixture.script("import sys,threading\nsys.stdout.write('x' * ((1 << 20) + 1))\nsys.stdout.flush()\nthreading.Event().wait()\n");

        let error = fixture
            .run()
            .await
            .expect_err("oversized output kills the hook");
        assert_eq!(error.code(), AtmErrorCode::HookMalformed);
        let events = capture.0.lock().expect("capture lock");
        assert_eq!(events.len(), 1, "exactly one spawn_until kill event");
        let fields = &events[0].0;
        assert_eq!(fields.get("team").map(String::as_str), Some("test-team"));
        assert_eq!(fields.get("phase").map(String::as_str), Some("bk"));
        assert_eq!(fields.get("hook").map(String::as_str), Some("task.close"));
        assert_eq!(fields.get("budget_ms").map(String::as_str), Some("30000"));
        assert_eq!(fields.get("outcome").map(String::as_str), Some("killed"));
        assert!(fields.get("elapsed_ms").unwrap().parse::<u64>().is_ok());
    }

    #[tokio::test]
    async fn doctor_sends_team_phase_custom_and_returns_findings() {
        let fixture = Fixture::new();
        fixture.reply(json!({"success":true,"message":"ok","findings":[{"severity":"warning","code":"TEST","message":"check"}]}));
        let registration: HookRegistration = serde_json::from_value(json!({
            "team":"test-team", "phase":"bk", "hook":"task.close", "module_path":fixture.target.module_path,
            "cwd":fixture.target.cwd, "timeout_ms":30000, "custom":fixture.target.custom,
            "active":true, "registered_by":"test-agent", "registered_at":"2026-10-18T00:00:00Z", "disabled_at":null
        })).unwrap();
        let schedule: CronSchedule = serde_json::from_value(json!({
            "id":1, "generation":1, "team":"test-team", "phase":"bk", "name":"goal", "cron_expr":"* * * * * *",
            "module_path":fixture.target.module_path, "cwd":fixture.target.cwd, "args":[], "timeout_ms":30000,
            "jitter_s":0, "custom":fixture.target.custom, "stop":{"kind":"hook"}, "active":true,
            "created_by":"test-agent", "created_at":"2026-10-18T00:00:00Z", "last_run_at":null, "last_outcome":null, "ended_at":null
        })).unwrap();
        for target in [HookTarget::from(&registration), HookTarget::from(&schedule)] {
            let reply = fixture
                .runner()
                .doctor(&target, RequestDeadline::after(Duration::from_secs(60)))
                .await
                .unwrap();
            assert!(reply.success);
            assert_eq!(reply.findings.len(), 1);
            assert_eq!(
                fixture.json("fixture_request.json"),
                serde_json::to_value(HookDoctorRequest::for_target(&target)).unwrap()
            );
            assert_eq!(fixture.json("fixture_argv.json"), json!(["doctor"]));
        }
        fixture.reply(json!({"ok":1}));
        assert_eq!(
            fixture
                .runner()
                .doctor(
                    &fixture.target,
                    RequestDeadline::after(Duration::from_secs(60))
                )
                .await
                .unwrap_err()
                .code(),
            AtmErrorCode::HookMalformed
        );
    }

    #[tokio::test]
    async fn fixture_dispatches_task_and_cron_entrypoints() {
        let fixture = Fixture::new();
        let request = fixture.request("task.start");
        assert_eq!(
            fixture
                .runner()
                .run(
                    &fixture.target,
                    &request,
                    RequestDeadline::after(Duration::from_secs(60))
                )
                .await
                .unwrap()
                .reply,
            HookReply::NotApplicable
        );
        fs::write(fixture.target.cwd.join("fixture_hooks.txt"), "task.start\n").unwrap();
        assert_eq!(
            fixture
                .runner()
                .run(
                    &fixture.target,
                    &request,
                    RequestDeadline::after(Duration::from_secs(60))
                )
                .await
                .unwrap()
                .reply,
            HookReply::PassThrough
        );
        assert_eq!(
            fixture
                .runner()
                .run(
                    &fixture.target,
                    &fixture.request("cron.tick"),
                    RequestDeadline::after(Duration::from_secs(60))
                )
                .await
                .unwrap()
                .reply,
            HookReply::PassThrough
        );
    }

    #[tokio::test]
    async fn stop_parses_both_shapes_and_refuses_missing_reason() {
        let fixture = Fixture::new();
        for (value, stop, reason) in [
            (json!({"stop":false}), false, None),
            (json!({"stop":true,"reason":"done"}), true, Some("done")),
        ] {
            fixture.reply(value);
            let reply = fixture
                .runner()
                .stop(
                    &fixture.target,
                    &fixture.request("cron.stop"),
                    RequestDeadline::after(Duration::from_secs(60)),
                )
                .await
                .unwrap();
            assert_eq!(
                reply,
                StopReply {
                    stop,
                    reason: reason.map(str::to_owned)
                }
            );
            assert_eq!(fixture.json("fixture_argv.json"), json!(["cron.stop"]));
            assert_eq!(fixture.json("fixture_request.json")["hook"], "cron.stop");
        }
        fixture.reply(json!({"stop":true}));
        assert_eq!(
            fixture
                .runner()
                .stop(
                    &fixture.target,
                    &fixture.request("cron.stop"),
                    RequestDeadline::after(Duration::from_secs(60))
                )
                .await
                .unwrap_err()
                .code(),
            AtmErrorCode::HookMalformed
        );
    }
}
