//! Typed task operations and queue placement values.

use serde::{Deserialize, Serialize};

use crate::task_state::TaskCloseOutcome;
use crate::types::TaskId;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum TaskOp {
    Start,
    Close {
        outcome: TaskCloseOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "to")]
pub enum MoveTarget {
    Head,
    End,
    Before { task_id: TaskId },
}

