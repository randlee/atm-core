//! Hook wiring the router carries: the task-command filter, the active hook
//! snapshot, the doctor runner, the cron scheduler slot and the chain tracker.
//!
//! A router built without these runs the un-hooked bodies unchanged.

use std::sync::{Arc, OnceLock};

use atm_core::api::{ApiResponse, AuthenticatedIngress, RequestDeadline};
use atm_core::boundary::{
    CronScheduler, HookActionDispatcher, HookRunner, TaskCallCtx, TaskCommand, TaskCommands,
};
use atm_core::error::AtmError;
use atm_core::protocol::RequestId;
use atm_core::types::HostName;

use crate::hook_actions::UnhookedRouter;
use crate::hook_chains::ChainTracker;
use crate::hook_filter::{ActiveHookSnapshot, HookedTaskCommands};
use crate::task_commands::task_caller;

use super::StorageAndNudgeRouter;

#[derive(Clone)]
pub(crate) struct HookWiring {
    /// `None` runs the un-hooked bodies; `Some` routes each classified task command.
    pub task_commands: Option<Arc<dyn TaskCommands>>,
    /// Set only by `with_hook_filter`, from the filter; `None` makes the registry routes unavailable.
    pub snapshot: Option<Arc<ActiveHookSnapshot>>,
    /// Set only by `with_hook_filter`, from the filter; doctor reads it.
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

    /// Installs `filter` as the task-command body and takes its runner, snapshot
    /// and chain tracker as this router's own. The filter is the only source of
    /// those handles, so a router can never hold a runner, snapshot or tracker
    /// that its filter does not use.
    #[must_use]
    pub fn with_hook_filter<D: HookActionDispatcher + 'static>(
        mut self,
        filter: Arc<HookedTaskCommands<D>>,
    ) -> Self {
        self.hooks.task_commands = Some(Arc::clone(&filter) as Arc<dyn TaskCommands>);
        self.hooks.runner = Some(Arc::clone(filter.runner()));
        self.hooks.snapshot = Some(Arc::clone(filter.snapshot()));
        self.hooks.chains = Arc::clone(filter.chains());
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
