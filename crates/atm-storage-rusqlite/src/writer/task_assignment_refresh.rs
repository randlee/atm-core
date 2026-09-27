//! Same-assignee assignment refresh: an update, not a new assignment.
//!
//! Rand's ruling (2026-09-27, #1619): re-assigning an open task to the agent
//! that already holds it follows the normal assignment path. It appends one
//! `updated` task event carrying the same actor and message fields the
//! `assigned`/`reassigned` events carry, and it resets the reminder budget
//! exactly as `assign`/`reassign`/`reopen` do, so a stalled task is eligible
//! for reminders again. State and queue position are left unchanged here:
//! this is not a reassignment, so the queue is not renumbered and
//! `assigned_at` is not reset. `apply_same_agent_update`
//! (`writer/task_ops.rs`) is the sole caller; it decides, from the caller's
//! `placement`, whether the queue position is *also* moved in the same
//! write (Rand's ruling, 2026-09-27, #1620 follow-up).

use atm_storage::task_state::{QueuePosition, TaskEvent, TaskState, Transition, transition};
use atm_storage::types::{AgentName, TeamName};
use atm_storage::{
    AtmError, AtmMessageId, IsoTimestamp, Message, MoveTarget, TaskEventKind, TaskId, TaskRow,
};

use super::task_ops::{
    TaskAssignmentApplied, append_task_event, insert_at_placement, load_task_row, params,
    queue_order, renumber_queue,
};
use super::task_rejection::task_move_invalid;
use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};

/// Checks whether `record` is a same-agent re-assignment of an already open
/// task: the caller filters on this before doing anything else, since it
/// changes the assignment path entirely (Rand, 2026-09-27, #1619).
pub(super) fn is_same_agent_open_update(row: Option<&TaskRow>, record: &Message) -> bool {
    row.is_some_and(|row| row.state.is_open() && row.assignee == record.agent)
}

/// Refreshes the assignment-message fields and resets the reminder budget
/// for a same-agent update, appending one `updated` event. Does not touch
/// state or queue position; the caller applies any placement-driven move
/// separately in the same write.
pub(super) fn refresh_same_assignment_fields(
    record: &Message,
    task_id: &TaskId,
    row: &TaskRow,
    message_id: AtmMessageId,
    at: IsoTimestamp,
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    connection
        .execute(
            "UPDATE tasks SET assignment_message_id=?3, description=?4, updated_at=?5,
             last_reminded_at=NULL, reminder_count=0, lead_notified_count=0
             WHERE team=?1 AND task_id=?2",
            params![
                record.team.as_str(),
                task_id.as_str(),
                message_id.to_string(),
                record.envelope.text,
                record.envelope.timestamp.to_string()
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to refresh task assignment", error))?;
    append_task_event(
        connection,
        target,
        &record.team,
        task_id,
        &record.agent,
        &at,
        TaskEventKind::Updated,
        Some(row.state.tag()),
        Some(row.state.tag()),
        None,
        &record.envelope.from,
        Some(message_id),
        None,
        None,
        None,
    )
}

/// Applies a same-agent re-assignment of an already open task (Rand's
/// ruling, 2026-09-27, #1619): always refreshes the assignment-message
/// fields, resets the reminder budget, and appends one `updated` event.
///
/// A same-agent update carries a `placement`, which the plain field refresh
/// above cannot express on its own; silently ignoring it is wrong (Rand's
/// ruling, 2026-09-27, #1620 follow-up):
/// - `--preempt` against an already-`assigned` (queued, not active) task
///   pauses the assignee's active task, if any, to `assigned` at position 2
///   (one `paused` event naming this task id) and moves this task to the
///   head, with no separate `moved` event — the same "one `assigned`/here
///   `updated` event only" shape the fresh-assignment preempt path uses.
///   Against an already-`active` task there is nothing to pause (it is
///   already the head) and no move to make.
/// - A plain `--head`/`--before`/`--end` placement against an `assigned`
///   task is honored as an ordinary move, appending one `moved` event
///   alongside the `updated` event. Against an `active` task it is a no-op:
///   the active task is always already at the head and is never
///   repositioned (`docs/requirements.md` Phase BA lifecycle item 6).
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_same_agent_update(
    record: &Message,
    task_id: &TaskId,
    placement: Option<&MoveTarget>,
    row: &TaskRow,
    message_id: AtmMessageId,
    at: IsoTimestamp,
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<TaskAssignmentApplied, AtmError> {
    let paused_task_id =
        if matches!(placement, Some(MoveTarget::Preempt)) && row.state == TaskState::Assigned {
            pause_active_task_for_preempt(record, task_id, connection, target)?
        } else {
            None
        };

    refresh_same_assignment_fields(record, task_id, row, message_id, at, connection, target)?;

    let from = row.position.map_or(1, QueuePosition::get);
    let queued_position = match (placement, row.state) {
        (Some(MoveTarget::Preempt), TaskState::Assigned) => reposition_queue(
            task_id,
            &MoveTarget::Head,
            connection,
            target,
            &record.team,
            &record.agent,
        )?,
        (Some(target_pos @ (MoveTarget::Head | MoveTarget::End)), TaskState::Assigned)
        | (Some(target_pos @ MoveTarget::Before { .. }), TaskState::Assigned) => {
            let to = reposition_queue(
                task_id,
                target_pos,
                connection,
                target,
                &record.team,
                &record.agent,
            )?;
            if to != from {
                append_task_event(
                    connection,
                    target,
                    &record.team,
                    task_id,
                    &record.agent,
                    &at,
                    TaskEventKind::Moved,
                    Some(row.state.tag()),
                    Some(row.state.tag()),
                    None,
                    &record.envelope.from,
                    None,
                    None,
                    None,
                    Some(&format!("{from}→{to}")),
                )?;
            }
            to
        }
        _ => from,
    };

    Ok(TaskAssignmentApplied {
        queued_position,
        is_update: true,
        reassign_notice: None,
        paused_task_id,
    })
}

/// Removes `task_id` from its own queue, re-inserts it at `placement`,
/// renumbers the queue, and returns its new 1-based position. Leaves event
/// recording to the caller, since callers disagree on whether a move here
/// is worth an audit event (preempt is silent; a plain placement is not).
fn reposition_queue(
    task_id: &TaskId,
    placement: &MoveTarget,
    connection: &SqliteConnection,
    target: &SharedDbTarget,
    team: &TeamName,
    assignee: &AgentName,
) -> Result<u32, AtmError> {
    let mut order = queue_order(connection, target, team, assignee)?;
    order.retain(|id| id != task_id);
    insert_at_placement(
        &mut order, task_id, placement, connection, target, team, assignee,
    )?;
    renumber_queue(team, assignee, &order, connection, target)?;
    order
        .iter()
        .position(|id| id == task_id)
        .and_then(|index| u32::try_from(index + 1).ok())
        .ok_or_else(|| task_move_invalid("task reposition produced no queue position"))
}

/// Pauses `record.agent`'s active task, if any, back to `assigned` in place
/// (its queue position is renumbered by the caller's own insertion), and
/// appends the one `paused` event naming `task_id` (the preempting task) in
/// its `detail`. Returns `None`, with no event, when the assignee has no
/// active task or its active task is `task_id` itself — both cases fall
/// through to plain `--head` behavior (issue #1620).
pub(super) fn pause_active_task_for_preempt(
    record: &Message,
    task_id: &TaskId,
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<Option<TaskId>, AtmError> {
    let order = queue_order(connection, target, &record.team, &record.agent)?;
    let Some(active) = order
        .first()
        .map(|id| load_task_row(connection, target, &record.team, id))
        .transpose()?
        .flatten()
        .filter(|row| row.state == TaskState::Active)
    else {
        return Ok(None);
    };
    if active.task_id == *task_id {
        return Ok(None);
    }
    let Transition(next_state) = transition(
        Some(active.state),
        TaskEvent::Paused,
        &active.task_id,
        &record.envelope.from,
        Some(&active.assignee),
        &active.assignee,
    )
    .map_err(|error| error.into_atm_error())?;
    let at = record.envelope.timestamp;
    connection
        .execute(
            "UPDATE tasks SET state=?3, updated_at=?4 WHERE team=?1 AND task_id=?2",
            params![
                record.team.as_str(),
                active.task_id.as_str(),
                next_state.as_str(),
                at.to_string()
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to pause active task", error))?;
    append_task_event(
        connection,
        target,
        &record.team,
        &active.task_id,
        &active.assignee,
        &at,
        TaskEventKind::Paused,
        Some(active.state.tag()),
        Some(next_state.tag()),
        None,
        &record.envelope.from,
        None,
        None,
        None,
        Some(&format!("preempted by {task_id}")),
    )?;
    Ok(Some(active.task_id))
}
