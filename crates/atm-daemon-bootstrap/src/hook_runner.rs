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
    HOOK_ACTION_RESERVE_MS, HookExitCause, HookOutcome, HookRunner, HookTarget, PhaseId, sealed,
};
use atm_core::doctor::hooks::{HookDoctorReply, HookDoctorRequest};
use atm_core::error::AtmError;
use atm_core::hook::{HookReply, HookRequest, StopReply};
use atm_core::types::TeamName;
use tokio::sync::Semaphore;

use crate::process_deadline::{SpawnFailure, SpawnSpec, SpawnedOutput, spawn_with_deadline};

pub const HOOK_CONCURRENCY_MAX: usize = 16;

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

fn env_value(env: &dyn EnvSource, key: &str, windows: bool) -> Option<String> {
    if !windows {
        return env.var(key);
    }
    (0..(1usize << key.len())).find_map(|mask| {
        let variant: String = key
            .bytes()
            .enumerate()
            .map(|(index, byte)| {
                if mask & (1 << index) == 0 {
                    byte.to_ascii_uppercase() as char
                } else {
                    byte.to_ascii_lowercase() as char
                }
            })
            .collect();
        env.var(&variant)
    })
}

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
        match env_value(env, key, windows) {
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
        env_value(env, "PATHEXT", true)
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

    fn spec(&self, target: &HookTarget, hook: &str) -> Result<SpawnSpec, AtmError> {
        for (path, directory) in [
            (target.cwd.clone(), true),
            (target.cwd.join(&target.module_path), false),
        ] {
            let valid = if directory {
                path.is_dir()
            } else {
                path.is_file()
            };
            if !valid {
                return Err(AtmError::hook_exit(HookExitCause::Spawn {
                    path,
                    cause: "path does not exist or has the wrong type".into(),
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
        let remaining = deadline.remaining().unwrap_or_default();
        let budget = if doctor {
            doctor_budget(target.timeout_ms, remaining)
        } else {
            runner_budget(target.timeout_ms, remaining)
        }
        .ok_or_else(|| {
            AtmError::hook_timeout(
                &target.phase,
                if doctor {
                    "doctor budget exhausted"
                } else {
                    "request budget exhausted before the hook ran"
                },
            )
        })?;
        let started = tokio::time::Instant::now();
        let _permit = tokio::time::timeout(budget, self.permits.clone().acquire_owned())
            .await
            .map_err(|_| AtmError::hook_timeout(&target.phase, "hook concurrency limit reached"))?
            .map_err(|_| missing("hook semaphore closed"))?;
        let spec = self.spec(target, hook)?;
        let remaining = remaining_spawn_budget(&target.phase, budget, started.elapsed())?;
        let output = spawn_with_deadline(spec, &stdin, remaining)
            .await
            .map_err(|failure| {
                spawn_failure_error(&target.phase, target.timeout_ms, budget, failure)
            })?;
        check_status(&output)?;
        Ok(output)
    }
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

pub(crate) fn runner_budget(timeout_ms: u32, remaining: Duration) -> Option<Duration> {
    doctor_budget(
        timeout_ms,
        remaining.checked_sub(Duration::from_millis(HOOK_ACTION_RESERVE_MS))?,
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
    use std::collections::BTreeMap;
    use std::fs;

    use atm_core::atm_temp::ProcessEnvSource;
    use atm_core::boundary::{CronSchedule, HookName, HookRegistration};
    use atm_core::error::AtmErrorCode;
    use atm_core::hook::{HOOK_SCHEMA_VERSION, HookCaller};
    use serde_json::{Value, json};
    use tempfile::TempDir;

    use super::*;

    struct TestEnv(BTreeMap<String, String>);

    impl EnvSource for TestEnv {
        fn var(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
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
        let captured: BTreeMap<_, _> = hook_env(&TestEnv(windows.clone()), &team, &phase, true)
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
                hook_env(&TestEnv(values), &team, &phase, true)
                    .unwrap_err()
                    .code(),
                AtmErrorCode::HookExit
            );
        }
    }

    #[test]
    fn configured_environment_has_only_explicit_contract_keys() {
        let fixture = Fixture::new();
        let spec = fixture
            .runner()
            .spec(&fixture.target, "task.close")
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

    #[test]
    fn runner_passes_daemon_path_and_home_values() {
        let output = crate::process_deadline::environment_probe_output();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("BK3_ENVIRONMENT_PROOF_COMPLETE"));
    }

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
        fixture.target.timeout_ms = 200;
        fixture.script("import threading\nthreading.Event().wait()\n");
        let error = fixture.run().await.unwrap_err();
        assert_eq!(error.code(), AtmErrorCode::HookTimeout);
        let after_ms: u64 = error
            .message()
            .strip_prefix("HOOK.TIMEOUT: phase bk: no reply within ")
            .unwrap()
            .split_once(" ms")
            .unwrap()
            .0
            .parse()
            .unwrap();
        assert!(after_ms <= u64::from(fixture.target.timeout_ms));
        assert_eq!(
            error.message(),
            AtmError::hook_timeout(
                &fixture.target.phase,
                &format!("no reply within {after_ms} ms (limited by configured timeout)")
            )
            .message()
        );
    }

    #[test]
    fn timeout_reports_effective_budget_and_binding_limit() {
        let phase = PhaseId::parse("bk").unwrap();
        for (timeout_ms, remaining_ms, after_ms, expected_budget, limit) in [
            (30000, 600, 75, 100, "request deadline"),
            (200, 60000, 175, 200, "configured timeout"),
        ] {
            let budget = runner_budget(timeout_ms, Duration::from_millis(remaining_ms)).unwrap();
            assert_eq!(budget, Duration::from_millis(expected_budget));
            let error = spawn_failure_error(
                &phase,
                timeout_ms,
                budget,
                SpawnFailure::TimedOut { after_ms },
            );
            assert_eq!(
                error.message(),
                AtmError::hook_timeout(
                    &phase,
                    &format!("no reply within {after_ms} ms (limited by {limit})")
                )
                .message()
            );
        }
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
    async fn stdout_over_cap_is_malformed_and_child_reaped() {
        let fixture = Fixture::new();
        fixture.script("import sys,threading\nsys.stdout.write('x' * ((1 << 20) + 1))\nsys.stdout.flush()\nthreading.Event().wait()\n");
        let error = fixture.run().await.unwrap_err();
        assert_eq!(error.code(), AtmErrorCode::HookMalformed);
        assert!(error.message().contains("stdout exceeds"));
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
