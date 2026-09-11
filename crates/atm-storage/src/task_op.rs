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

#[cfg(test)]
mod tests {
    use super::{MoveTarget, TaskOp};
    use crate::task_state::TaskCloseOutcome;

    #[test]
    fn task_operations_use_tagged_wire_shape() {
        let value = serde_json::to_value(TaskOp::Start).unwrap();
        assert_eq!(value["op"], "start");
    }

    #[test]
    fn close_operation_carries_typed_outcome_and_optional_reason() {
        let value = serde_json::to_value(TaskOp::Close {
            outcome: TaskCloseOutcome::Refused,
            reason: Some("blocked".to_owned()),
        })
        .unwrap();
        assert_eq!(value["op"], "close");
        assert_eq!(value["outcome"], "refused");
        assert_eq!(value["reason"], "blocked");
    }

    #[test]
    fn placement_targets_round_trip() {
        let target = MoveTarget::Before { task_id: "BA.2".parse().unwrap() };
        let encoded = serde_json::to_string(&target).unwrap();
        let decoded: MoveTarget = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, target);
    }
}
