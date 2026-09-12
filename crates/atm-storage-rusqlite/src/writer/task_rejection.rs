//! Typed task-rejection construction shared by the writer paths.

use atm_storage::{AtmError, AtmErrorCode};

pub(super) fn task_not_found(detail: impl std::fmt::Display) -> AtmError {
    AtmError::new(AtmErrorCode::TaskNotFound, detail.to_string())
}

pub(super) fn task_already_closed(detail: impl std::fmt::Display) -> AtmError {
    AtmError::new(AtmErrorCode::TaskAlreadyClosed, detail.to_string())
}

pub(super) fn task_not_counterparty(detail: impl std::fmt::Display) -> AtmError {
    AtmError::new(AtmErrorCode::TaskNotCounterparty, detail.to_string())
}

pub(super) fn task_stale_counterparty(detail: impl std::fmt::Display) -> AtmError {
    AtmError::new(AtmErrorCode::TaskStaleCounterparty, detail.to_string())
}

pub(super) fn task_move_invalid(detail: impl std::fmt::Display) -> AtmError {
    AtmError::new(AtmErrorCode::TaskMoveInvalid, detail.to_string())
}

pub(super) fn is_task_rejection(code: AtmErrorCode) -> bool {
    matches!(
        code,
        AtmErrorCode::TaskNotFound
            | AtmErrorCode::TaskAlreadyClosed
            | AtmErrorCode::TaskNotCounterparty
            | AtmErrorCode::TaskStaleCounterparty
            | AtmErrorCode::TaskMoveInvalid
    )
}
