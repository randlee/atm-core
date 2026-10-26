//! Private runner mechanics kept apart from the public hook-runner surface.

use std::time::Duration;

use atm_core::atm_temp::EnvSource;
use atm_core::boundary::{HookExitCause, HookTarget};
use atm_core::error::AtmError;

use super::{
    DaemonHookRunner, HookDeadlineSource, SpawnFailure, SpawnSpec, SpawnedOutput, hook_env,
    missing, spawn_with_deadline,
};

#[cfg(feature = "test-support")]
use crate::process_deadline::spawn_until;

struct CapturedEnv<'a>(&'a [(std::ffi::OsString, std::ffi::OsString)]);

impl EnvSource for CapturedEnv<'_> {
    fn var(&self, key: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .and_then(|(_, value)| value.to_str().map(str::to_owned))
    }
}

impl DaemonHookRunner {
    pub(super) async fn spec(
        &self,
        target: &HookTarget,
        hook: &str,
    ) -> Result<SpawnSpec, AtmError> {
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

    pub(super) async fn acquire_permit(
        &self,
        target: &HookTarget,
        hook: &str,
        budget: Duration,
        started: tokio::time::Instant,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, AtmError> {
        let permit = self.permits.clone().acquire_owned();
        match &self.deadline_source {
            HookDeadlineSource::System => tokio::time::timeout(budget, permit)
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
                .map_err(|_| missing("hook semaphore closed")),
            #[cfg(feature = "test-support")]
            HookDeadlineSource::Controlled(deadline) => tokio::select! {
                permit = permit => permit.map_err(|_| missing("hook semaphore closed")),
                _ = deadline.wait_for_advance() => Err(AtmError::hook_timeout(&target.phase, "hook concurrency limit reached")),
            },
        }
    }

    pub(super) async fn spawn(
        &self,
        spec: SpawnSpec,
        stdin: &[u8],
        remaining: Duration,
    ) -> Result<SpawnedOutput, SpawnFailure> {
        match &self.deadline_source {
            HookDeadlineSource::System => spawn_with_deadline(spec, stdin, remaining).await,
            #[cfg(feature = "test-support")]
            HookDeadlineSource::Controlled(deadline) => {
                spawn_until(spec, stdin, deadline.wait_for_process_advance())
                    .await
                    .map_err(|failure| match failure {
                        SpawnFailure::TimedOut { .. } => SpawnFailure::TimedOut {
                            after_ms: remaining.as_millis().min(u128::from(u64::MAX)) as u64,
                        },
                        other => other,
                    })
            }
        }
    }
}
