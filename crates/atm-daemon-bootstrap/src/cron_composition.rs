use std::{future::Future, sync::Arc, time::Duration};

use atm_core::types::HostName;
use atm_core::{api::RequestDeadline, error::AtmError};
use atm_cron::{AtmCron, AtmCronDeps, Clock, JitterSource};
use atm_http_runtime::{
    CronLifecycleNudgerAdapter, CronModuleMailDeliveryAdapter, StorageAndNudgeRouter,
};
use tokio::time::Instant;

use crate::{DaemonWorkers, shutdown_probe};

pub(super) const CRON_WORKER_SHUTDOWN_GRACE: Duration = Duration::from_secs(1);

pub(crate) fn compose_system(
    handler: &StorageAndNudgeRouter,
    runtime: &atm_core::LocalServiceRuntime,
    runner: Arc<dyn atm_core::boundary::HookRunner>,
    dispatcher: Arc<atm_http_runtime::UnhookedRouter>,
    host: Option<HostName>,
    cron_runner: Arc<dyn atm_core::boundary::CronHookRunner>,
    lifecycle_nudge_emitter: Arc<dyn atm_core::boundary::CronLifecycleNudgeEmitter>,
) -> Result<Arc<AtmCron>, AtmError> {
    compose_runtime_cron(
        handler,
        runtime,
        runner,
        dispatcher,
        Arc::new(atm_cron::SystemClock),
        Arc::new(atm_cron::SystemJitter),
        host,
        cron_runner,
        lifecycle_nudge_emitter,
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
    cron_runner: Arc<dyn atm_core::boundary::CronHookRunner>,
    lifecycle_nudge_emitter: Arc<dyn atm_core::boundary::CronLifecycleNudgeEmitter>,
) -> Result<Arc<AtmCron>, AtmError> {
    let lifecycle_delivery = Arc::new(CronLifecycleNudgerAdapter::new(
        handler.clone(),
        lifecycle_nudge_emitter,
    ));
    let module_mail_delivery = Arc::new(CronModuleMailDeliveryAdapter::new(handler.clone()));
    compose_cron_with_typed(
        handler,
        AtmCronDeps {
            store: runtime.cron_schedule_store()?,
            runner,
            dispatcher,
            clock,
            jitter,
            host,
        },
        cron_runner,
        runtime.cron_transition_store()?,
        lifecycle_delivery,
        module_mail_delivery,
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

fn compose_cron_with_typed(
    handler: &StorageAndNudgeRouter,
    deps: AtmCronDeps,
    cron_runner: Arc<dyn atm_core::boundary::CronHookRunner>,
    transitions: Arc<dyn atm_core::boundary::CronTransitionStore>,
    lifecycle_delivery: Arc<dyn atm_core::boundary::CronLifecycleNudger>,
    module_mail_delivery: Arc<dyn atm_core::boundary::CronModuleMailDelivery>,
) -> Result<Arc<AtmCron>, AtmError> {
    let cron = Arc::new(
        AtmCron::new(deps).with_typed_execution(atm_cron::CronEngineDeps {
            runner: cron_runner,
            transitions,
            lifecycle_delivery,
            module_mail_delivery,
        }),
    );
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
