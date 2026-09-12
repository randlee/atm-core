//! Durable task-start handoff after a task reminder reaches its delivery backend.

use std::path::Path;
use std::sync::Arc;

use atm_core::LocalServiceRuntime;
use atm_core::boundary::{MemberKey, ReminderOutcome, TaskOp, TaskRow, TaskState, TaskStore};
use atm_core::observability::NullObservability;
use atm_core::send::{
    NudgeMode, SendMessageSource, WriteRequest, render_task_started_template,
    write_mail_with_runtime,
};
use atm_core::types::IsoTimestamp;

use crate::herdr_queue_wake::run_blocking;

/// Records the emitted reminder, then starts an assigned task exactly once.
/// A failed start write leaves the durable reminder audit in place so a later
/// owed-head pass can retry the idempotent task operation without another nudge.
pub(crate) async fn complete_task_handoff(
    runtime: &LocalServiceRuntime,
    task_store: &Arc<dyn TaskStore + Send + Sync>,
    daemon_home: &Path,
    member: &MemberKey,
    row: &TaskRow,
    now: IsoTimestamp,
) -> Result<TaskRow, atm_core::error::AtmError> {
    let store = Arc::clone(task_store);
    let member = member.clone();
    let task_id = row.task_id.clone();
    let recorded = run_blocking(move || {
        store.record_reminder(&member, &task_id, now, ReminderOutcome::Emitted)
    })
    .await?;
    start_assigned_task(runtime, daemon_home, &recorded).await?;
    Ok(recorded)
}

pub(crate) async fn start_assigned_task(
    runtime: &LocalServiceRuntime,
    daemon_home: &Path,
    row: &TaskRow,
) -> Result<(), atm_core::error::AtmError> {
    if row.state != TaskState::Assigned {
        return Ok(());
    }
    let runtime = runtime.clone();
    let daemon_home = daemon_home.to_path_buf();
    let row = row.clone();
    run_blocking(move || {
        let body = render_task_started_template(row.task_id.as_str(), row.assignee.as_str())?;
        let mut request = WriteRequest::new(
            daemon_home.clone(),
            daemon_home,
            atm_core::boundary::DAEMON_ACTOR_NAME.parse()?,
            &format!("{}@{}", row.assigner, row.team),
            row.team.clone(),
            SendMessageSource::Inline(body),
            Some(format!("task_started:{}", row.task_id)),
            false,
            Some(row.task_id.clone()),
            false,
        )?
        .with_nudge_mode(NudgeMode::Deferred);
        request.task_op = Some(TaskOp::Start);
        write_mail_with_runtime(request, &NullObservability, &runtime).map(|_| ())
    })
    .await
}
