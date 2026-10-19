//! Hook wiring the router carries: the task-command filter, the active hook
//! snapshot, the doctor runner, the cron scheduler slot and the chain tracker.
//!
//! A router built without these runs the un-hooked bodies unchanged.

use std::sync::{Arc, OnceLock};

use atm_core::api::{ApiResponse, AuthenticatedIngress, RequestDeadline};
use atm_core::boundary::{CronScheduler, HookRunner, TaskCallCtx, TaskCommand, TaskCommands};
use atm_core::error::AtmError;
use atm_core::protocol::RequestId;
use atm_core::types::HostName;

use crate::hook_actions::UnhookedRouter;
use crate::hook_chains::ChainTracker;
use crate::hook_filter::ActiveHookSnapshot;
use crate::task_commands::task_caller;

use super::StorageAndNudgeRouter;

#[derive(Clone)]
pub(crate) struct HookWiring {
    /// `None` runs the un-hooked bodies; `Some` routes each classified task command.
    pub task_commands: Option<Arc<dyn TaskCommands>>,
    /// Shared with the filter; `None` makes the registry routes unavailable.
    pub snapshot: Option<Arc<ActiveHookSnapshot>>,
    /// Used by doctor only; the filter holds its own runner.
    pub runner: Option<Arc<dyn HookRunner>>,
    /// Shared by every clone; unset makes cron routes unavailable.
    pub cron: Arc<OnceLock<Arc<dyn CronScheduler>>>,
    pub chains: Arc<ChainTracker>,
    /// Stamped as the caller host of every hook request this router filters.
    pub local_host: Option<HostName>,
}

impl Default for HookWiring {
    fn default() -> Self {
        Self {
            task_commands: None,
            snapshot: None,
            runner: None,
            cron: Arc::new(OnceLock::new()),
            chains: Arc::new(ChainTracker::new()),
            local_host: None,
        }
    }
}

impl StorageAndNudgeRouter {
    #[must_use]
    pub fn with_task_commands(mut self, commands: Arc<dyn TaskCommands>) -> Self {
        self.hooks.task_commands = Some(commands);
        self
    }

    #[must_use]
    pub fn with_hook_runner(mut self, runner: Arc<dyn HookRunner>) -> Self {
        self.hooks.runner = Some(runner);
        self
    }

    #[must_use]
    pub fn with_hook_snapshot(mut self, snapshot: Arc<ActiveHookSnapshot>) -> Self {
        self.hooks.snapshot = Some(snapshot);
        self
    }

    #[must_use]
    pub fn with_local_host(mut self, host: HostName) -> Self {
        self.hooks.local_host = Some(host);
        self
    }

    #[must_use]
    pub fn hook_chains(&self) -> Arc<ChainTracker> {
        Arc::clone(&self.hooks.chains)
    }

    /// Installs the cron scheduler once; a second install is refused.
    pub fn install_cron_scheduler(
        &self,
        scheduler: Arc<dyn CronScheduler>,
    ) -> Result<(), AtmError> {
        self.hooks.cron.set(scheduler).map_err(|_| {
            AtmError::validation_with_recovery(
                "cron scheduler is already installed",
                "install the cron scheduler once during daemon startup",
            )
        })
    }

    /// A clone with no task filter, the only router an action or the filter's inner body may use.
    #[must_use]
    pub fn unhooked(&self) -> UnhookedRouter {
        let mut router = self.clone();
        router.hooks.task_commands = None;
        UnhookedRouter::new(router)
    }

    /// Runs one classified task command through the installed task commands.
    pub(crate) async fn call_task_command(
        &self,
        commands: &dyn TaskCommands,
        command: TaskCommand,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
        request_id: RequestId,
    ) -> Result<ApiResponse, AtmError> {
        let (caller_identity, caller_team) = task_caller(&command);
        let ctx = TaskCallCtx {
            caller_identity,
            caller_team,
            host: self.hooks.local_host.clone(),
            ingress,
            deadline,
            request_id,
        };
        command.call(commands, &ctx).await
    }
}
