use std::{future::Future, sync::Arc, time::Duration};

use atm_core::types::HostName;
use atm_core::{api::RequestDeadline, error::AtmError};
use atm_cron::{AtmCron, AtmCronDeps, Clock, JitterSource};
use atm_http_runtime::StorageAndNudgeRouter;
use tokio::time::Instant;

use crate::{DaemonWorkers, shutdown_probe};

pub(super) const CRON_WORKER_SHUTDOWN_GRACE: Duration = Duration::from_secs(1);

pub(crate) fn compose_system(
    handler: &StorageAndNudgeRouter,
    runtime: &atm_core::LocalServiceRuntime,
    runner: Arc<dyn atm_core::boundary::HookRunner>,
    dispatcher: Arc<atm_http_runtime::UnhookedRouter>,
    host: Option<HostName>,
) -> Result<Arc<AtmCron>, AtmError> {
    compose_runtime_cron(
        handler,
        runtime,
        runner,
        dispatcher,
        Arc::new(atm_cron::SystemClock),
        Arc::new(atm_cron::SystemJitter),
        host,
    )
}

pub(crate) fn compose_runtime_cron(
    handler: &StorageAndNudgeRouter,
    runtime: &atm_core::LocalServiceRuntime,
    runner: Arc<dyn atm_core::boundary::HookRunner>,
    dispatcher: Arc<atm_http_runtime::UnhookedRouter>,
    clock: Arc<dyn Clock>,
    jitter: Arc<dyn JitterSource>,
    host: Option<HostName>,
) -> Result<Arc<AtmCron>, AtmError> {
    compose_cron(
        handler,
        AtmCronDeps {
            store: runtime.cron_schedule_store()?,
            runner,
            dispatcher,
            clock,
            jitter,
            host,
        },
    )
}

pub(crate) fn compose_cron(
    handler: &StorageAndNudgeRouter,
    deps: AtmCronDeps,
) -> Result<Arc<AtmCron>, AtmError> {
    let cron = Arc::new(AtmCron::new(deps));
    handler.install_cron_scheduler(cron.clone())?;
    Ok(cron)
}

pub(crate) async fn start_cron_worker(
    cron: &Arc<AtmCron>,
    workers: &mut DaemonWorkers,
) -> Result<(), AtmError> {
    workers.cron = Some(cron.start_worker().await?);
    Ok(())
}

pub(crate) async fn run_shutdown_steps(
    listener: impl Future<Output = Result<(), AtmError>>,
    handler: &StorageAndNudgeRouter,
    mut workers: DaemonWorkers,
    deadline: Instant,
) -> Result<(), AtmError> {
    let cron_deadline = deadline.min(Instant::now() + CRON_WORKER_SHUTDOWN_GRACE);
    shutdown_probe::step("cron_worker", cron_deadline, async {
        if let Some(cron) = workers.cron.take() {
            cron.shutdown(cron_deadline).await;
        }
    })
    .await;
    shutdown_probe::step("hook_chains", deadline, async {
        let chains = handler.hook_chains();
        chains.begin_shutdown();
        chains
            .drain(RequestDeadline::after_at(
                deadline.into_std(),
                std::time::Duration::ZERO,
            ))
            .await;
    })
    .await;
    crate::shutdown_remaining_steps(listener, handler, workers, deadline).await
}
