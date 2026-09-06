//! Production-path proof for the write-through RAM roster seam
//! (`BOUNDARY-RosterStore-Sqlite-WriteThrough`, PR #1240 finding AW-RAM-I4).
//!
//! The unit tests in `roster_runtime` exercise the decorator over an
//! in-memory fake. This suite instead opens storage the way production does
//! -- `SqliteStorageFactory::open()` against a real temp database -- and
//! proves that the handles it hands back actually serve roster reads from
//! RAM rather than from SQLite.

use std::sync::Arc;

use atm_storage::StorageFactory;
use atm_storage::contract::{
    AgentType, RosterHarness, RosterMember, RosterMemberKind, RosterSnapshot,
};
use atm_storage::types::{AgentName, TeamName};
use atm_storage_rusqlite::{SqliteStorageBackend, SqliteStorageFactory};

const TEST_TEAM: &str = "test-team";
const TEST_AGENT: &str = "test-agent";

fn member(team: &str, agent: &str) -> RosterMember {
    RosterMember {
        team_name: TeamName::from_validated(team.to_string()),
        agent_name: AgentName::from_validated(agent.to_string()),
        member_kind: RosterMemberKind::Permanent,
        harness: RosterHarness::ClaudeCode,
        agent_type: AgentType::default(),
        model: Default::default(),
        recipient_pane_id: None,
        metadata_json: Default::default(),
    }
}

fn snapshot(team: &TeamName, members: Vec<RosterMember>) -> RosterSnapshot {
    RosterSnapshot {
        team_name: team.clone(),
        members,
        refreshed_at: None,
    }
}

/// The production composition root must hand back a roster store whose reads
/// are served from the RAM mirror, and a mirror that observes every durable
/// write in the same operation.
#[test]
fn production_factory_open_serves_roster_reads_from_ram() {
    let root = tempfile::tempdir().expect("temp durable state root");
    let handles = SqliteStorageFactory::host_scoped()
        .open(root.path())
        .expect("production storage open");
    let team = TeamName::from_validated(TEST_TEAM.to_string());

    // A durable write through the production handle updates RAM in the same
    // operation: the mirror observes it with no reload.
    handles
        .roster_store()
        .save_roster(&snapshot(&team, vec![member(TEST_TEAM, TEST_AGENT)]))
        .expect("durable roster write");
    assert_eq!(
        handles.roster_runtime_mirror().load_team_roster(&team),
        vec![member(TEST_TEAM, TEST_AGENT)],
        "the RAM mirror must observe a durable write without a reload"
    );
    assert_eq!(
        handles.roster_runtime_mirror().list_teams(),
        vec![team.clone()]
    );

    // Ephemeral (RAM-only) state exists for the member, which is only
    // possible if the production handle is the write-through decorator and
    // not the raw durable store.
    let agent = AgentName::from_validated(TEST_AGENT.to_string());
    assert!(
        handles
            .roster_runtime_mirror()
            .set_herdr_wake_pending(&team, &agent, true),
        "ephemeral RAM state must exist for a member written through the seam"
    );

    // Now mutate the database out-of-band, behind the mirror's back, through
    // a second raw durable backend on the same file. If production roster
    // reads went to SQLite, they would pick this up; because they are served
    // from RAM, they must not.
    let out_of_band = SqliteStorageBackend::new(root.path().join("mail.db"))
        .expect("second raw durable backend on the same database file");
    Arc::clone(&out_of_band.roster_store())
        .save_roster(&snapshot(&team, Vec::new()))
        .expect("out-of-band durable roster write");
    assert_eq!(
        out_of_band
            .roster_store()
            .load_roster(&team)
            .unwrap()
            .members,
        Vec::new(),
        "sanity: the out-of-band durable write really landed in SQLite"
    );

    assert_eq!(
        handles
            .roster_store()
            .load_roster(&team)
            .expect("roster read")
            .members,
        vec![member(TEST_TEAM, TEST_AGENT)],
        "a production roster read must come from RAM, not from SQLite"
    );
    assert_eq!(
        handles.roster_runtime_mirror().load_team_roster(&team),
        vec![member(TEST_TEAM, TEST_AGENT)]
    );

    // The explicit control-plane reload is the only thing that re-derives RAM
    // from durable state.
    handles
        .roster_runtime_mirror()
        .reload_from_durable()
        .expect("control-plane reload");
    assert!(
        handles
            .roster_runtime_mirror()
            .load_team_roster(&team)
            .is_empty(),
        "an explicit reload must re-derive RAM from the durable store"
    );
}

/// Startup hydration on the production path populates RAM from durable state
/// before any read is served.
#[test]
fn production_factory_open_hydrates_ram_from_durable_state() {
    let root = tempfile::tempdir().expect("temp durable state root");
    let team = TeamName::from_validated(TEST_TEAM.to_string());
    {
        let handles = SqliteStorageFactory::host_scoped()
            .open(root.path())
            .expect("production storage open");
        handles
            .roster_store()
            .save_roster(&snapshot(&team, vec![member(TEST_TEAM, TEST_AGENT)]))
            .expect("durable roster write");
    }

    let reopened = SqliteStorageFactory::host_scoped()
        .open(root.path())
        .expect("production storage reopen");
    assert_eq!(
        reopened.roster_runtime_mirror().load_team_roster(&team),
        vec![member(TEST_TEAM, TEST_AGENT)],
        "a fresh production open must hydrate RAM from the durable roster"
    );
}
