//! Authorized SQLite-side provenance projection for the cron transition seam.
//!
//! BK-10 owns SQL materialization. This private row wrapper exists now so the
//! contract has one real external adapter implementation to compile against.

use atm_storage::contract::sealed;
use atm_storage::{
    AgentName, CronCommittedNoticeRecord, CronLifecycleEvent, CronName, CronNoticeKey, TeamName,
};

#[allow(
    dead_code,
    reason = "BK-10 owns SQLite transition materialization; BK-9 freezes its authorized record projection"
)]
struct SqliteCommittedCronNoticeRecord {
    key: CronNoticeKey,
    team: TeamName,
    recipient: AgentName,
    name: CronName,
    event: CronLifecycleEvent,
}

impl sealed::Sealed for SqliteCommittedCronNoticeRecord {}

impl CronCommittedNoticeRecord for SqliteCommittedCronNoticeRecord {
    fn key(&self) -> CronNoticeKey {
        self.key
    }

    fn team(&self) -> &TeamName {
        &self.team
    }

    fn recipient(&self) -> &AgentName {
        &self.recipient
    }

    fn name(&self) -> &CronName {
        &self.name
    }

    fn event(&self) -> &CronLifecycleEvent {
        &self.event
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use atm_storage::roles::TEAM_ATM_DEV;
    use atm_storage::{CronDiagnosticDetail, CronGeneration, CronScheduleId};

    use super::*;

    #[test]
    fn authorized_adapter_record_projects_an_opaque_intent() {
        let record = SqliteCommittedCronNoticeRecord {
            key: CronNoticeKey {
                schedule_id: CronScheduleId::new(17),
                generation: CronGeneration::FIRST,
                transition_seq: 2,
            },
            team: TeamName::from_str(TEAM_ATM_DEV).expect("team"),
            recipient: AgentName::from_str("creator").expect("recipient"),
            name: CronName::parse("nightly").expect("name"),
            event: CronLifecycleEvent::Stopped {
                message: CronDiagnosticDetail::parse("completed".into()).expect("detail"),
            },
        };

        let intent = atm_storage::CronNoticeIntent::from_committed(&record);

        assert_eq!(intent.key(), record.key);
        assert_eq!(intent.recipient(), &record.recipient);
    }
}
