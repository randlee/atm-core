use crate::schema::AtmMessageId;
use crate::types::{AgentName, TaskId, TeamName};

use super::WarningEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryPersistenceDisposition {
    Persisted,
    AppendDegraded,
    SqliteFailedRecovered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompanionNudgePlan {
    pub(crate) sender: AgentName,
    pub(crate) sender_team: Option<TeamName>,
    pub(crate) message_id: AtmMessageId,
    pub(crate) requires_ack: bool,
    pub(crate) task_id: Option<TaskId>,
    pub(crate) is_ack: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DeliveryPersistenceResult {
    pub(crate) disposition: DeliveryPersistenceDisposition,
    pub(crate) warnings: Vec<WarningEntry>,
    pub(crate) companion_nudge: Option<CompanionNudgePlan>,
}

impl DeliveryPersistenceResult {
    pub(crate) fn persisted() -> Self {
        Self {
            disposition: DeliveryPersistenceDisposition::Persisted,
            warnings: Vec::new(),
            companion_nudge: None,
        }
    }

    pub(crate) fn append_degraded(warning: WarningEntry) -> Self {
        Self {
            disposition: DeliveryPersistenceDisposition::AppendDegraded,
            warnings: vec![warning],
            companion_nudge: None,
        }
    }

    pub(crate) fn sqlite_failed_recovered(
        warning: WarningEntry,
        companion_nudge: CompanionNudgePlan,
    ) -> Self {
        Self {
            disposition: DeliveryPersistenceDisposition::SqliteFailedRecovered,
            warnings: vec![warning],
            companion_nudge: Some(companion_nudge),
        }
    }
}
