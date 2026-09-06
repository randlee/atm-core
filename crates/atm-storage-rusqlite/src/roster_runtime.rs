//! Write-through, RAM-backed roster mirror.
//!
//! This module owns the concrete [`RosterStore`] decorator that keeps a
//! runtime-owned in-memory roster mirror synchronized with every durable
//! roster write, plus the [`RosterRuntimeMirror`] handle every roster
//! consumer reads through after startup hydration.
//!
//! Design ruling: every roster durable write updates RAM in the same
//! operation; ephemeral per-member state (e.g. Herdr wake-pending) lives
//! only in RAM and mutates on observed state changes; every roster consumer
//! reads RAM, never the durable store, outside of startup hydration or an
//! explicit control-plane reload. A read racing a concurrent mutation is
//! intentionally not ordered beyond memory safety.
//!
//! This decorator is backend-agnostic (it wraps any `Arc<dyn RosterStore>`),
//! but boundary policy (`boundaries/atm-storage-rusqlite/roster-store-sqlite.toml`)
//! authorizes this crate as the sole implementation site for the write-through
//! `RosterStore` impl; construct it only through
//! [`build_write_through_roster`], which every composition root (including
//! test-support callers) must use instead of hand-rolling an equivalent type.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use atm_storage::contract::{RosterMember, RosterMemberEphemeralState, RosterRuntimeMirror};
use atm_storage::types::{AgentName, TeamName};
use atm_storage::{AtmError, RosterSnapshot, RosterStore};

/// One team's write-through roster mirror: the durable roster columns plus
/// the ephemeral per-member state layered on top of them. The whole record
/// is replaced as one `Arc` on every mutation, so readers observe either the
/// entries-before or the entries-after a write, never a torn mix of the two.
#[derive(Clone, Debug, Default)]
struct TeamRosterRecord {
    entries: Arc<[RosterMember]>,
    ephemeral: Arc<BTreeMap<AgentName, RosterMemberEphemeralState>>,
}

impl TeamRosterRecord {
    fn from_durable(entries: Vec<RosterMember>) -> Self {
        let ephemeral = entries
            .iter()
            .map(|entry| {
                (
                    entry.agent_name.clone(),
                    RosterMemberEphemeralState::default(),
                )
            })
            .collect();
        Self {
            entries: entries.into(),
            ephemeral: Arc::new(ephemeral),
        }
    }

    /// Builds the record a durable roster replacement produces: retained
    /// members keep their ephemeral state, new members start with the
    /// default (empty) ephemeral state, and removed members' ephemeral state
    /// is dropped with them.
    fn replaced_with(&self, members: &[RosterMember]) -> Self {
        let ephemeral = members
            .iter()
            .map(|member| {
                let state = self
                    .ephemeral
                    .get(&member.agent_name)
                    .copied()
                    .unwrap_or_default();
                (member.agent_name.clone(), state)
            })
            .collect();
        Self {
            entries: members.to_vec().into(),
            ephemeral: Arc::new(ephemeral),
        }
    }
}

/// Recovers from a poisoned lock by taking the guarded value as-is.
///
/// A panic while a roster lock is held can only happen from a bug in this
/// module's own (infallible, non-panicking-by-design) critical sections; the
/// data behind the lock is never left semantically inconsistent by a
/// panic mid-mutation here, because every mutation builds its replacement
/// value before installing it. Treating poison as recoverable keeps one
/// caller's panic from wedging every other roster reader/writer for the rest
/// of the process lifetime.
fn recover_poison<T>(poisoned: std::sync::PoisonError<T>) -> T {
    poisoned.into_inner()
}

/// Runtime-owned, write-through in-memory roster mirror: one record per
/// team, holding durable roster columns plus the ephemeral state columns
/// that never appear in the database.
///
/// Locking is two-level and per-team: the outer map lock is held only long
/// enough to look up (or insert/remove) one team's record `Arc`; the actual
/// mutation happens on that team's own inner lock. An ephemeral update, or a
/// durable-write RAM sync, for one team therefore never blocks a read or
/// mutation for any other team.
#[derive(Default)]
struct RosterRuntimeState {
    teams: RwLock<BTreeMap<TeamName, Arc<RwLock<TeamRosterRecord>>>>,
}

impl RosterRuntimeState {
    fn team_lock(&self, team: &TeamName) -> Option<Arc<RwLock<TeamRosterRecord>>> {
        let teams = self.teams.read().unwrap_or_else(recover_poison);
        teams.get(team).cloned()
    }

    fn team_lock_or_insert(&self, team: &TeamName) -> Arc<RwLock<TeamRosterRecord>> {
        if let Some(existing) = self.team_lock(team) {
            return existing;
        }
        let mut teams = self.teams.write().unwrap_or_else(recover_poison);
        Arc::clone(
            teams
                .entry(team.clone())
                .or_insert_with(|| Arc::new(RwLock::new(TeamRosterRecord::default()))),
        )
    }

    /// Seeds one team's record straight from a durable read. Used only at
    /// startup hydration and at an explicit reload re-hydration; never on a
    /// per-request or per-tick path.
    fn hydrate(&self, team: &TeamName, entries: Vec<RosterMember>) {
        let lock = self.team_lock_or_insert(team);
        let mut record = lock.write().unwrap_or_else(recover_poison);
        *record = TeamRosterRecord::from_durable(entries);
    }

    /// Discards every team's RAM state. Used only immediately before a
    /// reload re-hydration so a team removed durably out-of-band cannot
    /// survive in RAM past the reload.
    fn clear(&self) {
        let mut teams = self.teams.write().unwrap_or_else(recover_poison);
        teams.clear();
    }

    /// Replaces one team's *durable* roster columns in RAM in the same
    /// operation as the durable write that produced `members`. An empty
    /// roster drops the team entirely, mirroring the durable store's
    /// `list_teams` semantics (a team with zero roster rows is not
    /// enumerable).
    fn replace_roster(&self, team: &TeamName, members: &[RosterMember]) {
        if members.is_empty() {
            let mut teams = self.teams.write().unwrap_or_else(recover_poison);
            teams.remove(team);
            return;
        }
        let lock = self.team_lock_or_insert(team);
        let mut record = lock.write().unwrap_or_else(recover_poison);
        *record = record.replaced_with(members);
    }

    fn load_team_roster(&self, team: &TeamName) -> Vec<RosterMember> {
        self.team_lock(team)
            .map(|lock| lock.read().unwrap_or_else(recover_poison).entries.to_vec())
            .unwrap_or_default()
    }

    fn load_roster_member(&self, team: &TeamName, agent: &AgentName) -> Option<RosterMember> {
        let lock = self.team_lock(team)?;
        let record = lock.read().unwrap_or_else(recover_poison);
        record
            .entries
            .iter()
            .find(|member| &member.agent_name == agent)
            .cloned()
    }

    fn list_teams(&self) -> Vec<TeamName> {
        let teams = self.teams.read().unwrap_or_else(recover_poison);
        teams.keys().cloned().collect()
    }

    fn ephemeral_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Option<RosterMemberEphemeralState> {
        let lock = self.team_lock(team)?;
        let record = lock.read().unwrap_or_else(recover_poison);
        record.ephemeral.get(agent).copied()
    }

    /// Mutates one member's ephemeral state in RAM only. Returns `false`
    /// without effect when the team or member is not present in the current
    /// snapshot; there is nothing durable to attach ephemeral state to.
    fn set_ephemeral_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
        mutate: impl FnOnce(&mut RosterMemberEphemeralState),
    ) -> bool {
        let Some(lock) = self.team_lock(team) else {
            return false;
        };
        let mut record = lock.write().unwrap_or_else(recover_poison);
        if !record
            .entries
            .iter()
            .any(|member| &member.agent_name == agent)
        {
            return false;
        }
        let mut ephemeral = (*record.ephemeral).clone();
        let state = ephemeral.entry(agent.clone()).or_default();
        mutate(state);
        record.ephemeral = Arc::new(ephemeral);
        true
    }
}

/// Hydrates the RAM roster mirror from the durable roster store.
///
/// Fail-closed: a durable read failure aborts hydration and returns an
/// error to the caller instead of leaving RAM silently short of a team for
/// the process lifetime. A partially populated `state` on error is
/// discarded by the caller (startup construction fails outright; an
/// explicit reload clears RAM before retrying hydration, see
/// [`WriteThroughRosterView::reload_from_durable`]).
fn hydrate_roster_runtime_from_durable(
    durable: &Arc<dyn RosterStore + Send + Sync>,
    state: &RosterRuntimeState,
) -> Result<(), AtmError> {
    let teams = durable.list_teams()?;
    for team in teams {
        let snapshot = durable.load_roster(&team)?;
        state.hydrate(&team, snapshot.members);
    }
    Ok(())
}

/// The single write-through seam for the durable roster store: every write
/// updates the runtime-owned RAM mirror in the same operation, and every
/// read is served from RAM. Construct through [`build_write_through_roster`].
#[derive(Clone)]
struct WriteThroughRosterView {
    durable: Arc<dyn RosterStore + Send + Sync>,
    state: Arc<RosterRuntimeState>,
}

impl atm_storage::contract::sealed::Sealed for WriteThroughRosterView {}

impl RosterStore for WriteThroughRosterView {
    fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError> {
        Ok(RosterSnapshot {
            team_name: team.clone(),
            members: self.state.load_team_roster(team),
            refreshed_at: None,
        })
    }

    fn save_roster(&self, roster: &RosterSnapshot) -> Result<(), AtmError> {
        self.durable.save_roster(roster)?;
        self.state
            .replace_roster(&roster.team_name, &roster.members);
        Ok(())
    }

    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
        Ok(self.state.list_teams())
    }
}

impl RosterRuntimeMirror for WriteThroughRosterView {
    fn load_team_roster(&self, team: &TeamName) -> Vec<RosterMember> {
        self.state.load_team_roster(team)
    }

    fn load_roster_member(&self, team: &TeamName, agent: &AgentName) -> Option<RosterMember> {
        self.state.load_roster_member(team, agent)
    }

    fn list_teams(&self) -> Vec<TeamName> {
        self.state.list_teams()
    }

    fn ephemeral_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Option<RosterMemberEphemeralState> {
        self.state.ephemeral_state(team, agent)
    }

    fn set_herdr_wake_pending(&self, team: &TeamName, agent: &AgentName, pending: bool) -> bool {
        self.state.set_ephemeral_state(team, agent, |ephemeral| {
            ephemeral.herdr_wake_pending = pending
        })
    }

    fn reload_from_durable(&self) -> Result<(), AtmError> {
        // Fail-closed like startup hydration: clear first, so a caller that
        // ignores an error is never left with a stale-but-plausible mix of
        // pre-reload and partially-reloaded state.
        self.state.clear();
        hydrate_roster_runtime_from_durable(&self.durable, &self.state)
    }
}

/// Paired write-through [`RosterStore`] handle and [`RosterRuntimeMirror`]
/// read handle returned by [`build_write_through_roster`], over the same
/// backing RAM state.
pub type WriteThroughRosterHandles = (
    Arc<dyn RosterStore + Send + Sync>,
    Arc<dyn RosterRuntimeMirror + Send + Sync>,
);

/// Builds the write-through roster seam from a durable [`RosterStore`]:
/// hydrates the RAM mirror once (fail-closed), then returns the paired
/// write-through [`RosterStore`] handle and [`RosterRuntimeMirror`] read
/// handle over the same backing state.
///
/// Every composition root that assembles a durable roster store --
/// production or test -- must call this rather than handing the raw durable
/// store to a roster consumer directly, so every roster read after startup
/// comes from RAM.
///
/// # Errors
/// Returns an error, and installs nothing, if the durable roster store
/// cannot be enumerated or read during hydration.
pub fn build_write_through_roster(
    durable: Arc<dyn RosterStore + Send + Sync>,
) -> Result<WriteThroughRosterHandles, AtmError> {
    let state = Arc::new(RosterRuntimeState::default());
    hydrate_roster_runtime_from_durable(&durable, &state)?;
    let view = Arc::new(WriteThroughRosterView { durable, state });
    Ok((view.clone(), view))
}

#[cfg(test)]
mod tests {
    use super::*;
    use atm_storage::contract::{AgentType, RosterHarness, RosterMemberKind};
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeDurableRoster {
        rosters: Mutex<BTreeMap<TeamName, Vec<RosterMember>>>,
        fail_list_teams: std::sync::atomic::AtomicBool,
        fail_load_roster: std::sync::atomic::AtomicBool,
    }

    impl atm_storage::contract::sealed::Sealed for FakeDurableRoster {}

    impl RosterStore for FakeDurableRoster {
        fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError> {
            if self
                .fail_load_roster
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err(AtmError::validation(
                    "simulated durable roster read failure",
                ));
            }
            let rosters = self.rosters.lock().unwrap();
            Ok(RosterSnapshot {
                team_name: team.clone(),
                members: rosters.get(team).cloned().unwrap_or_default(),
                refreshed_at: None,
            })
        }

        fn save_roster(&self, roster: &RosterSnapshot) -> Result<(), AtmError> {
            let mut rosters = self.rosters.lock().unwrap();
            if roster.members.is_empty() {
                rosters.remove(&roster.team_name);
            } else {
                rosters.insert(roster.team_name.clone(), roster.members.clone());
            }
            Ok(())
        }

        fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
            if self
                .fail_list_teams
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err(AtmError::validation(
                    "simulated durable roster list failure",
                ));
            }
            let rosters = self.rosters.lock().unwrap();
            Ok(rosters.keys().cloned().collect())
        }
    }

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

    #[test]
    fn hydrates_existing_durable_teams_at_construction() {
        let durable = Arc::new(FakeDurableRoster::default());
        durable
            .save_roster(&RosterSnapshot {
                team_name: TeamName::from_validated("atm-dev"),
                members: vec![member("atm-dev", "dev-1")],
                refreshed_at: None,
            })
            .unwrap();
        let (_store, mirror) = build_write_through_roster(durable).unwrap();
        assert_eq!(
            mirror.load_team_roster(&TeamName::from_validated("atm-dev")),
            vec![member("atm-dev", "dev-1")]
        );
    }

    #[test]
    fn construction_fails_closed_when_durable_list_teams_errors() {
        let durable = Arc::new(FakeDurableRoster::default());
        durable
            .fail_list_teams
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let result = build_write_through_roster(durable);
        assert!(result.is_err(), "startup hydration must fail closed");
    }

    #[test]
    fn construction_fails_closed_when_durable_load_roster_errors() {
        let durable = Arc::new(FakeDurableRoster::default());
        durable
            .save_roster(&RosterSnapshot {
                team_name: TeamName::from_validated("atm-dev"),
                members: vec![member("atm-dev", "dev-1")],
                refreshed_at: None,
            })
            .unwrap();
        durable
            .fail_load_roster
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let result = build_write_through_roster(durable);
        assert!(result.is_err(), "startup hydration must fail closed");
    }

    #[test]
    fn save_roster_updates_ram_in_the_same_operation() {
        let durable = Arc::new(FakeDurableRoster::default());
        let (store, mirror) = build_write_through_roster(durable).unwrap();
        let team = TeamName::from_validated("atm-dev");
        store
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: vec![member("atm-dev", "dev-1")],
                refreshed_at: None,
            })
            .unwrap();
        assert_eq!(
            mirror.load_team_roster(&team),
            vec![member("atm-dev", "dev-1")]
        );
    }

    #[test]
    fn ephemeral_state_mutates_independently_of_durable_columns() {
        let durable = Arc::new(FakeDurableRoster::default());
        durable
            .save_roster(&RosterSnapshot {
                team_name: TeamName::from_validated("atm-dev"),
                members: vec![member("atm-dev", "dev-1")],
                refreshed_at: None,
            })
            .unwrap();
        let (_store, mirror) = build_write_through_roster(durable).unwrap();
        let team = TeamName::from_validated("atm-dev");
        let agent = AgentName::from_validated("dev-1");
        assert_eq!(
            mirror.ephemeral_state(&team, &agent),
            Some(RosterMemberEphemeralState::default())
        );
        assert!(mirror.set_herdr_wake_pending(&team, &agent, true));
        assert_eq!(
            mirror.ephemeral_state(&team, &agent),
            Some(RosterMemberEphemeralState {
                herdr_wake_pending: true
            })
        );
    }

    #[test]
    fn set_ephemeral_state_is_a_no_op_for_an_absent_member() {
        let durable = Arc::new(FakeDurableRoster::default());
        let (_store, mirror) = build_write_through_roster(durable).unwrap();
        let team = TeamName::from_validated("atm-dev");
        let agent = AgentName::from_validated("dev-1");
        assert!(!mirror.set_herdr_wake_pending(&team, &agent, true));
    }

    #[test]
    fn reload_from_durable_re_derives_ram_and_drops_removed_teams() {
        let durable = Arc::new(FakeDurableRoster::default());
        let team = TeamName::from_validated("atm-dev");
        durable
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: vec![member("atm-dev", "dev-1")],
                refreshed_at: None,
            })
            .unwrap();
        let (store, mirror) =
            build_write_through_roster(Arc::clone(&durable) as Arc<dyn RosterStore + Send + Sync>)
                .unwrap();
        // Out-of-band durable mutation that bypasses the write-through seam.
        durable
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: Vec::new(),
                refreshed_at: None,
            })
            .unwrap();
        assert_eq!(
            mirror.load_team_roster(&team),
            vec![member("atm-dev", "dev-1")]
        );
        store.list_teams().unwrap(); // exercise through the RosterStore facade too
        mirror.reload_from_durable().unwrap();
        assert!(mirror.load_team_roster(&team).is_empty());
    }

    #[test]
    fn reload_from_durable_fails_closed_on_durable_error() {
        let durable = Arc::new(FakeDurableRoster::default());
        let (_store, mirror) =
            build_write_through_roster(Arc::clone(&durable) as Arc<dyn RosterStore + Send + Sync>)
                .unwrap();
        durable
            .fail_list_teams
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(mirror.reload_from_durable().is_err());
    }
}
