//! Fenced canonical operations admitted through the SQLite writer.

use super::*;

pub(super) fn execute_acknowledgement_with_fence(
    source: &AcknowledgementSource,
    builder: &Arc<dyn AcknowledgementReplyBuilder>,
    provenance: MessageWriteOrigin,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    super::super::message_admission::validate_cron_generation_fence(provenance, connection)?;
    execute_acknowledgement(source, builder, connection, cache, target)
}

pub(super) struct TaskMoveDetails<'a> {
    pub(super) team: &'a TeamName,
    pub(super) task_id: &'a TaskId,
    pub(super) actor: &'a AgentName,
    pub(super) target: &'a MoveTarget,
    pub(super) at: IsoTimestamp,
}

pub(super) fn execute_task_move_with_fence(
    details: TaskMoveDetails<'_>,
    provenance: MessageWriteOrigin,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    super::super::message_admission::validate_cron_generation_fence(provenance, connection)?;
    super::super::task_ops::apply_task_move(
        details.team,
        details.task_id,
        details.actor,
        details.target,
        details.at,
        connection,
        target,
    )
    .map(WriteOpResult::TaskMoved)
}

/// Admits a template-sourced message, then persists its decomposition when
/// the record is newly inserted.
///
/// Returns the same [`WriteOpResult::TemplateMessageAdmission`] outcome the
/// inline `execute` match arm previously produced, with no behavior change.
pub(super) fn execute_admit_template_message(
    admission: &TemplateMessageAdmission,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    admission.validate()?;
    match super::super::message_admission::execute_upsert_message(
        &admission.record,
        admission.provenance,
        connection,
        cache,
        target,
    )? {
        WriteOpResult::UpsertMessage {
            inserted: false,
            existing,
            ..
        } => Ok(WriteOpResult::TemplateMessageAdmission {
            inserted: false,
            existing,
            task_assignee: None,
            queued_position: None,
            reassign_notice: None,
            task_rejection: None,
            task_events: Vec::new(),
        }),
        WriteOpResult::UpsertMessage {
            inserted: true,
            task_assignee,
            queued_position,
            reassign_notice,
            task_rejection,
            task_events,
            ..
        } => {
            let _ =
                execute_decomposed_message_admission(&admission.decomposition, connection, target)?;
            Ok(WriteOpResult::TemplateMessageAdmission {
                inserted: true,
                existing: None,
                task_assignee,
                queued_position,
                reassign_notice,
                task_rejection,
                task_events,
            })
        }
        other => Err(AtmError::daemon_unavailable(format!(
            "sqlite writer returned the wrong result while admitting a template message: {other:?}"
        ))),
    }
}
