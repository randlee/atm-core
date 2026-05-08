use super::SqliteRosterStore;
use super::{deserialize_json, serialize_json};
use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::schema::TeamConfig;
use atm_core::types::IsoTimestamp;
use rusqlite::{OptionalExtension, params};

impl boundary::sealed::Sealed for SqliteRosterStore {}

impl boundary::RosterStore for SqliteRosterStore {
    fn replace_roster(
        &self,
        request: boundary::RosterStoreReplaceRosterRequest,
    ) -> Result<boundary::RosterStoreReplaceRosterResponse, AtmError> {
        let previous_member_count = self
            .load_roster(boundary::RosterStoreLoadRosterRequest {
                team: request.team.clone(),
            })
            .ok()
            .map(|response| response.roster.members.len() as u64)
            .unwrap_or(0);
        let roster_json = serialize_json(&request.roster, "roster-store snapshot")?;
        let updated_at = chrono::Utc::now().to_rfc3339();
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO rosters(team, roster_json, source, updated_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(team) DO UPDATE SET
                       roster_json = excluded.roster_json,
                       source = excluded.source,
                       updated_at = excluded.updated_at;",
                    params![
                        request.team.as_str(),
                        roster_json,
                        request.source.clone(),
                        updated_at.clone(),
                    ],
                )
                .map_err(|error| {
                    self.db
                        .error("failed to replace roster-store snapshot", error)
                })?;
            transaction
                .execute(
                    "DELETE FROM team_roster WHERE team_name = ?1;",
                    params![request.team.as_str()],
                )
                .map_err(|error| self.db.error("failed to clear team-roster projection", error))?;
            for member in &request.roster.members {
                let member_json = serialize_json(member, "team-roster member")?;
                let pane_id = (!member.tmux_pane_id.is_empty()).then(|| member.tmux_pane_id.clone());
                transaction
                    .execute(
                        "INSERT INTO team_roster(team_name, agent_name, member_json, source, recipient_pane_id, pid, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
                        params![
                            request.team.as_str(),
                            member.name.as_str(),
                            member_json,
                            request.source.clone(),
                            pane_id,
                            Option::<i64>::None,
                            updated_at.clone(),
                        ],
                    )
                    .map_err(|error| self.db.error("failed to replace team-roster member projection", error))?;
            }
            Ok(())
        })?;

        Ok(boundary::RosterStoreReplaceRosterResponse {
            team: request.team,
            previous_member_count,
            current_member_count: request.roster.members.len() as u64,
            replaced: true,
        })
    }

    fn load_roster(
        &self,
        request: boundary::RosterStoreLoadRosterRequest,
    ) -> Result<boundary::RosterStoreLoadRosterResponse, AtmError> {
        let roster_json = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT roster_json FROM rosters WHERE team = ?1;",
                    params![request.team.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| self.db.error("failed to load roster-store snapshot", error))
        })?;

        let roster_json = roster_json.ok_or_else(|| {
            AtmError::validation(format!(
                "roster-store load failed because team {} has no persisted roster snapshot",
                request.team
            ))
            .with_recovery(
                "Replace the roster through RosterStore::replace_roster before loading it.",
            )
        })?;
        let roster: TeamConfig = deserialize_json(&roster_json, "roster-store snapshot")?;

        Ok(boundary::RosterStoreLoadRosterResponse {
            team: request.team,
            roster,
        })
    }

    fn query_membership(
        &self,
        request: boundary::RosterStoreQueryMembershipRequest,
    ) -> Result<boundary::RosterStoreQueryMembershipResponse, AtmError> {
        let membership = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT member_json, pid
                     FROM team_roster
                     WHERE team_name = ?1 AND agent_name = ?2;",
                    params![request.team.as_str(), request.member.as_str()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<u32>>(1)?)),
                )
                .optional()
                .map_err(|error| {
                    self.db
                        .error("failed to query team-roster membership", error)
                })
        })?;
        let (member_json, pid) = membership
            .map(|(member_json, pid)| (Some(member_json), pid))
            .unwrap_or((None, None));
        let member = member_json
            .as_deref()
            .map(|value| deserialize_json(value, "team-roster member"))
            .transpose()?;
        let is_member = member.is_some();

        Ok(boundary::RosterStoreQueryMembershipResponse {
            team: request.team,
            member,
            is_member,
            pid,
        })
    }

    fn record_heartbeat(
        &self,
        request: boundary::RosterStoreRecordHeartbeatRequest,
    ) -> Result<boundary::RosterStoreRecordHeartbeatResponse, AtmError> {
        let updated_at = request.observed_at.into_inner().to_rfc3339();
        self.db.with_transaction(|transaction| {
            let previous_pid_row = transaction
                .query_row(
                    "SELECT pid
                     FROM team_roster
                     WHERE team_name = ?1 AND agent_name = ?2;",
                    params![request.team.as_str(), request.member.as_str()],
                    |row| row.get::<_, Option<u32>>(0),
                )
                .optional()
                .map_err(|error| self.db.error("failed to query durable roster pid", error))?;
            let Some(previous_pid) = previous_pid_row else {
                return Err(AtmError::agent_not_found(
                    request.member.as_str(),
                    request.team.as_str(),
                ));
            };
            transaction
                .execute(
                    "UPDATE team_roster
                     SET pid = ?3, updated_at = ?4
                     WHERE team_name = ?1 AND agent_name = ?2;",
                    params![
                        request.team.as_str(),
                        request.member.as_str(),
                        request.pid,
                        updated_at,
                    ],
                )
                .map_err(|error| self.db.error("failed to persist durable roster pid", error))?;
            Ok(boundary::RosterStoreRecordHeartbeatResponse {
                team: request.team.clone(),
                member: request.member.clone(),
                previous_pid,
                current_pid: request.pid,
                pid_changed: previous_pid != Some(request.pid),
            })
        })
    }

    fn health_snapshot(
        &self,
        request: boundary::RosterStoreHealthSnapshotRequest,
    ) -> Result<boundary::RosterStoreHealthSnapshotResponse, AtmError> {
        let (member_count, updated_at) = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(1), MAX(updated_at)
                         FROM team_roster
                         WHERE team_name = ?1;",
                    params![request.team.as_str()],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .map_err(|error| {
                    self.db
                        .error("failed to load roster-store health snapshot", error)
                })
        })?;
        let updated_at = updated_at.ok_or_else(|| {
            AtmError::validation(format!(
                "roster-store health failed because team {} has no persisted roster snapshot",
                request.team
            ))
            .with_recovery(
                "Replace the roster through RosterStore::replace_roster before requesting health.",
            )
        })?;
        let refreshed_at = updated_at
            .parse::<chrono::DateTime<chrono::Utc>>()
            .map(IsoTimestamp::from_datetime)
            .map_err(|error| {
                AtmError::validation(format!(
                    "failed to parse roster-store health timestamp: {error}"
                ))
                .with_recovery(
                    "Repair the sqlite-backed roster row or rewrite it through the owning boundary.",
                )
                .with_source(error)
            })?;

        Ok(boundary::RosterStoreHealthSnapshotResponse {
            snapshot: boundary::RosterStoreHealthSnapshot {
                team: request.team,
                member_count: member_count as u64,
                stale: false,
                refreshed_at: Some(refreshed_at),
            },
        })
    }
}
