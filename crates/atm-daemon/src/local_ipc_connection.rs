use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use atm_core::error::AtmError;

use crate::active_connection_registry::ActiveConnectionRegistry;

pub(crate) fn drain_active_connections_for_shutdown(
    registry: &ActiveConnectionRegistry,
    force_shutdown: &AtomicBool,
    graceful_drain_deadline: Duration,
    force_cancel_deadline: Duration,
    shutdown_started: Instant,
    tracked_dispatch_join_deadline: Duration,
) -> Result<(), AtmError> {
    tracing::info!(
        active_connections = registry.active_connections(),
        active_work_items = registry.active_work_items(),
        "daemon shutdown signal received; starting graceful drain"
    );
    let graceful_deadline = shutdown_started + graceful_drain_deadline;
    let force_cancel_deadline = shutdown_started + force_cancel_deadline;
    while registry.active_work_items() > 0 && Instant::now() < graceful_deadline {
        registry.wait_for_connection_change(
            graceful_deadline.saturating_duration_since(Instant::now()),
        )?;
    }
    if registry.active_work_items() > 0 {
        tracing::info!(
            active_work_items = registry.active_work_items(),
            "daemon graceful drain hit deadline; continuing toward forced cancel"
        );
        force_shutdown.store(true, Ordering::SeqCst);
        registry.interrupt_all();
    } else {
        tracing::info!("daemon graceful drain completed cleanly");
    }
    while registry.active_work_items() > 0 && Instant::now() < force_cancel_deadline {
        registry.wait_for_connection_change(
            force_cancel_deadline.saturating_duration_since(Instant::now()),
        )?;
    }
    registry.join_tracked_dispatches(tracked_dispatch_join_deadline)?;
    let remaining_work_items = registry.active_work_items();
    if remaining_work_items > 0 {
        return Err(AtmError::daemon_unavailable(format!(
            "forced cancel deadline elapsed with {remaining_work_items} active daemon work item(s)"
        )));
    }
    Ok(())
}
