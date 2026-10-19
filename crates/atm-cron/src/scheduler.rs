use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use atm_core::boundary::{
    CronSchedule, CronScheduleStore, CronScheduler, CronStartSpec, HookActionDispatcher,
    HookRunner, NewCronSchedule, PhaseId,
};
use atm_core::{
    error::AtmError,
    types::{HostName, TeamName},
};
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};

use crate::{Clock, JitterSource, expr, jitter, tick, worker};

pub struct AtmCronDeps {
    pub store: Arc<dyn CronScheduleStore>,
    pub runner: Arc<dyn HookRunner>,
    pub dispatcher: Arc<dyn HookActionDispatcher>,
    pub clock: Arc<dyn Clock>,
    pub host: Option<HostName>,
    pub jitter: Arc<dyn JitterSource>,
}

pub struct AtmCron {
    pub(crate) deps: AtmCronDeps,
    reload: watch::Sender<u64>,
    start_lock: tokio::sync::Mutex<()>,
    stopped: Arc<AtomicBool>,
}

impl AtmCron {
    pub fn new(deps: AtmCronDeps) -> Self {
        Self {
            deps,
            reload: watch::channel(0).0,
            start_lock: tokio::sync::Mutex::new(()),
            stopped: Arc::new(AtomicBool::new(false)),
        }
    }

    fn check_running(&self) -> Result<(), AtmError> {
        if self.stopped.load(Ordering::SeqCst) {
            Err(AtmError::daemon_unavailable("cron worker stopped"))
        } else {
            Ok(())
        }
    }

    pub async fn start_worker(self: &Arc<Self>) -> Result<CronWorkerHandle, AtmError> {
        let _lock = self.start_lock.lock().await;
        self.check_running()?;
        if self.reload.receiver_count() != 0 {
            return Err(AtmError::daemon_unavailable("cron worker already started"));
        }
        let reload = self.reload.subscribe();
        let rows = store_call(self.deps.store.clone(), |store| store.active_all()).await?;
        let (shutdown, receiver) = oneshot::channel();
        let (running, settled) = watch::channel(0);
        let cron = self.clone();
        let stopped = self.stopped.clone();
        let worker = tokio::spawn(async move {
            let _guard = Stopped(stopped);
            worker::run(cron, rows, reload, receiver, running).await;
        });
        Ok(CronWorkerHandle {
            shutdown: Mutex::new(Some(shutdown)),
            worker: Mutex::new(Some(worker)),
            settled,
        })
    }

    fn bump(&self) {
        self.reload
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }
}

struct Stopped(Arc<AtomicBool>);

impl Drop for Stopped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

pub(crate) async fn store_call<T: Send + 'static>(
    store: Arc<dyn CronScheduleStore>,
    operation: impl FnOnce(&dyn CronScheduleStore) -> Result<T, AtmError> + Send + 'static,
) -> Result<T, AtmError> {
    tokio::task::spawn_blocking(move || operation(store.as_ref()))
        .await
        .map_err(|error| AtmError::daemon_unavailable(format!("cron store task failed: {error}")))?
}

impl atm_core::boundary::sealed::Sealed for AtmCron {}

#[async_trait]
impl CronScheduler for AtmCron {
    async fn start(&self, spec: CronStartSpec) -> Result<CronSchedule, AtmError> {
        self.check_running()?;
        let _lock = self.start_lock.lock().await;
        self.check_running()?;
        let now = self.deps.clock.now();
        let parsed = expr::parse(&spec.cron_expr, now)?;
        if tick::stop_due(&spec.stop, now) {
            return Err(AtmError::cron_expr_invalid("until is not in the future"));
        }
        let rows = store_call(self.deps.store.clone(), |store| store.active_all()).await?;
        let held: Vec<_> = rows
            .iter()
            .filter(|row| {
                row.cron_expr == spec.cron_expr
                    && !(row.team == spec.team && row.phase == spec.phase && row.name == spec.name)
            })
            .map(|row| row.jitter_s)
            .collect();
        let jitter_s = jitter::assign(&parsed, now, &held, self.deps.jitter.as_ref());
        let schedule = NewCronSchedule {
            team: spec.team,
            phase: spec.phase,
            name: spec.name,
            cron_expr: spec.cron_expr,
            module_path: spec.module_path,
            cwd: spec.cwd,
            args: spec.args,
            timeout_ms: spec.timeout_ms,
            jitter_s,
            custom: spec.custom,
            stop: spec.stop,
            created_by: spec.created_by,
            created_at: now.into(),
        };
        let row = store_call(self.deps.store.clone(), move |store| store.start(schedule)).await?;
        self.bump();
        Ok(row)
    }

    async fn end(
        &self,
        team: &TeamName,
        phase: &PhaseId,
        name: Option<&str>,
    ) -> Result<Vec<CronSchedule>, AtmError> {
        self.check_running()?;
        let (team, phase, name, now) = (
            team.clone(),
            phase.clone(),
            name.map(str::to_owned),
            self.deps.clock.now(),
        );
        let rows = store_call(self.deps.store.clone(), move |store| {
            store.end(&team, &phase, name.as_deref(), now.into())
        })
        .await?;
        self.bump();
        Ok(rows)
    }

    async fn list(
        &self,
        team: Option<&TeamName>,
        phase: Option<&PhaseId>,
    ) -> Result<Vec<CronSchedule>, AtmError> {
        self.check_running()?;
        let (team, phase) = (team.cloned(), phase.cloned());
        store_call(self.deps.store.clone(), move |store| {
            store.list(team.as_ref(), phase.as_ref())
        })
        .await
    }
}

pub struct CronWorkerHandle {
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    settled: watch::Receiver<usize>,
}

impl CronWorkerHandle {
    pub async fn shutdown(&self, deadline: tokio::time::Instant) {
        if let Some(sender) = self
            .shutdown
            .lock()
            .expect("cron shutdown lock poisoned")
            .take()
        {
            let _ = sender.send(());
        }
        let worker = self
            .worker
            .lock()
            .expect("cron worker lock poisoned")
            .take();
        if let Some(mut worker) = worker
            && tokio::time::timeout_at(deadline, &mut worker)
                .await
                .is_err()
        {
            worker.abort();
            let _ = worker.await;
        }
        let mut settled = self.settled.clone();
        let _ = settled.wait_for(|running| *running == 0).await;
    }
}

impl Drop for CronWorkerHandle {
    fn drop(&mut self) {
        if let Some(sender) = self
            .shutdown
            .get_mut()
            .expect("cron shutdown lock poisoned")
            .take()
        {
            let _ = sender.send(());
        }
    }
}
