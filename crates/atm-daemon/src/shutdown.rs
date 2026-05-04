use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use atm_core::doctor::{DoctorReport, DoctorRuntimeHealth, DoctorStatus};
use atm_core::error::AtmError;
use atm_core::home;

use crate::{ACCEPT_THREAD_JOIN_TIMEOUT, WORKER_THREAD_JOIN_TIMEOUT};

pub(crate) fn join_accept_thread(
    handle: JoinHandle<()>,
    thread_name: &str,
) -> Result<(), AtmError> {
    let deadline = Instant::now() + ACCEPT_THREAD_JOIN_TIMEOUT;
    while !handle.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    if !handle.is_finished() {
        return Err(AtmError::daemon_runtime(format!(
            "{thread_name} did not stop within {:?}",
            ACCEPT_THREAD_JOIN_TIMEOUT
        )));
    }
    if let Err(payload) = handle.join() {
        return Err(AtmError::daemon_runtime(format!(
            "{thread_name} panicked during shutdown: {}",
            thread_panic_message(payload)
        )));
    }
    Ok(())
}

pub(crate) fn wait_for_inflight_zero_until(inflight: &AtomicUsize, deadline: Instant) {
    while inflight.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
}

pub(crate) fn join_worker_threads(
    worker_threads: &Arc<Mutex<Vec<JoinHandle<()>>>>,
    shutdown_deadline: Instant,
) -> Result<(), AtmError> {
    let handles = {
        let mut handles = worker_threads.lock().map_err(|_| {
            AtmError::daemon_runtime("worker thread registry lock poisoned during shutdown")
        })?;
        std::mem::take(&mut *handles)
    };
    let mut first_error = None;
    for handle in handles {
        let per_thread_deadline =
            (Instant::now() + WORKER_THREAD_JOIN_TIMEOUT).min(shutdown_deadline);
        while !handle.is_finished() && Instant::now() < per_thread_deadline {
            thread::sleep(Duration::from_millis(25));
        }
        if !handle.is_finished() {
            let error = AtmError::daemon_runtime(format!(
                "worker thread did not stop before shutdown deadline (per-thread cap {:?})",
                WORKER_THREAD_JOIN_TIMEOUT
            ));
            first_error.get_or_insert(error);
            continue;
        }
        if let Err(payload) = handle.join() {
            let error = AtmError::daemon_runtime(format!(
                "worker thread panicked during shutdown: {}",
                thread_panic_message(payload)
            ));
            first_error.get_or_insert(error);
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(())
}

pub(crate) fn attach_runtime_health(
    mut report: DoctorReport,
    home_dir: &Path,
    team_name: &atm_core::types::TeamName,
) -> DoctorReport {
    let sqlite_path = home::mail_db_path_from_home(home_dir, team_name).ok();
    report.runtime = Some(DoctorRuntimeHealth {
        singleton_state: DoctorStatus::Healthy,
        singleton_detail: "daemon singleton is owned by the active runtime".to_string(),
        status_cache_state: DoctorStatus::Unavailable,
        status_cache_detail: "status cache health: not yet implemented".to_string(),
        sqlite_runtime_state: if sqlite_path.as_ref().is_some_and(|path| path.exists()) {
            DoctorStatus::Healthy
        } else {
            DoctorStatus::Warning
        },
        sqlite_runtime_detail: sqlite_path
            .map(|path| format!("runtime sees SQLite path {}", path.display()))
            .unwrap_or_else(|| {
                "runtime could not resolve a SQLite path for the active team".to_string()
            }),
    });
    report
}

pub(crate) fn thread_panic_message(payload: Box<dyn std::any::Any + Send + 'static>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "non-string panic payload".to_string()
}
