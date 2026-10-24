//! External compilation proof for the authorized SQLite committed-notice adapter.

use std::str::FromStr;

use atm_storage::contract::sealed;
use atm_storage::{
    AgentName, CronCommittedNoticeRecord, CronDiagnosticDetail, CronGeneration, CronLifecycleEvent,
    CronName, CronNoticeIntent, CronScheduleId, TeamName,
};

struct AuthorizedSqliteRecord {
    key: atm_storage::CronNoticeKey,
    team: TeamName,
    recipient: AgentName,
    name: CronName,
    event: CronLifecycleEvent,
}

impl sealed::Sealed for AuthorizedSqliteRecord {}

impl CronCommittedNoticeRecord for AuthorizedSqliteRecord {
    fn key(&self) -> atm_storage::CronNoticeKey {
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

#[test]
fn authorized_external_adapter_compiles_and_projects_an_opaque_notice() {
    let record = AuthorizedSqliteRecord {
        key: atm_storage::CronNoticeKey {
            schedule_id: CronScheduleId::new(17),
            generation: CronGeneration::FIRST,
            transition_seq: 2,
        },
        team: TeamName::from_str(atm_storage::roles::TEAM_ATM_DEV).expect("team"),
        recipient: AgentName::from_str("creator").expect("recipient"),
        name: CronName::parse("nightly").expect("name"),
        event: CronLifecycleEvent::Stopped {
            message: CronDiagnosticDetail::parse("completed".into()).expect("detail"),
        },
    };

    let intent = CronNoticeIntent::from_committed(&record);

    assert_eq!(intent.key(), record.key);
    assert_eq!(intent.team(), &record.team);
    assert_eq!(intent.recipient(), &record.recipient);
}
