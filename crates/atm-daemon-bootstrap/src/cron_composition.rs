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

pub(crate) struct RuntimeCronComposition<'a> {
    pub(crate) handler: &'a StorageAndNudgeRouter,
    pub(crate) runtime: &'a atm_core::LocalServiceRuntime,
    pub(crate) runner: Arc<dyn atm_core::boundary::HookRunner>,
    pub(crate) dispatcher: Arc<atm_http_runtime::UnhookedRouter>,
    pub(crate) host: Option<HostName>,
    pub(crate) cron_runner: Arc<dyn atm_core::boundary::CronHookRunner>,
    pub(crate) lifecycle_nudge_emitter: Arc<dyn atm_core::boundary::CronLifecycleNudgeEmitter>,
}

pub(crate) fn compose_system(
    composition: RuntimeCronComposition<'_>,
) -> Result<Arc<AtmCron>, AtmError> {
    compose_runtime_cron(
        composition,
        Arc::new(atm_cron::SystemClock),
        Arc::new(atm_cron::SystemJitter),
    )
}

fn compose_runtime_cron(
    composition: RuntimeCronComposition<'_>,
    clock: Arc<dyn Clock>,
    jitter: Arc<dyn JitterSource>,
) -> Result<Arc<AtmCron>, AtmError> {
    let lifecycle_delivery = Arc::new(CronLifecycleNudgerAdapter::new(
        composition.handler.clone(),
        composition.lifecycle_nudge_emitter,
    ));
    let module_mail_delivery = Arc::new(CronModuleMailDeliveryAdapter::new(
        composition.handler.clone(),
    ));
    compose_cron_with_typed(
        composition.handler,
        AtmCronDeps {
            store: composition.runtime.cron_schedule_store()?,
            runner: composition.runner,
            dispatcher: composition.dispatcher,
            clock,
            jitter,
            host: composition.host,
        },
        composition.cron_runner,
        composition.runtime.cron_transition_store()?,
        lifecycle_delivery,
        module_mail_delivery,
    )
}

/// Builds the same typed cron path as the daemon while substituting only the
/// deterministic clock and jitter sources owned by test support.
#[cfg(feature = "test-support")]
pub(crate) fn compose_controlled(
    composition: RuntimeCronComposition<'_>,
    clock: Arc<atm_cron::ManualClock>,
) -> Result<Arc<AtmCron>, AtmError> {
    compose_runtime_cron(composition, clock, Arc::new(atm_cron::SeqJitter::default()))
}

#[cfg(test)]
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
