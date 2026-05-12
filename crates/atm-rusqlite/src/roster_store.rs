use super::SqliteRosterStore;
use super::{deserialize_json, serialize_json};
use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::types::IsoTimestamp;
use rusqlite::{OptionalExtension, params};
use serde_json::{Map, Value};

impl boundary::sealed::Sealed for SqliteRosterStore {}

struct StoredRosterMemberRow {
    member_kind: String,
    harness: String,
    agent_type: String,
    model: String,
    recipient_pane_id: Option<String>,
    metadata_json: String,
}

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
            .map(|response| response.members.len() as u64)
            .unwrap_or(0);
        let updated_at = chrono::Utc::now().to_rfc3339();
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "DELETE FROM team_roster WHERE team_name = ?1;",
                    params![request.team.as_str()],
                ).map_err(|error| self.db.error("failed to clear canonical team roster", error))?;
            for member in &request.members {
                if member.team_name != request.team {
                    return Err(AtmError::validation(format!(
                        "roster-store replace rejected member {} because team_name {} did not match request team {}",
                        member.agent_name, member.team_name, request.team
                    )));
                }
                let metadata_json = serialize_json(&member.metadata_json, "team-roster metadata")?;
                transaction
                    .execute(
                        "INSERT INTO team_roster(
                            team_name,
                            agent_name,
                            member_kind,
                            harness,
                            agent_type,
                            model,
                            metadata_json,
                            source,
                            recipient_pane_id,
                            updated_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10);",
                        params![
                            request.team.as_str(),
                            member.agent_name.as_str(),
                            roster_member_kind_value(member.member_kind),
                            roster_harness_value(member.harness),
                            member.agent_type.clone(),
                            member.model.clone(),
                            metadata_json,
                            request.source.clone(),
                            member.recipient_pane_id.clone(),
                            updated_at.clone(),
                        ],
                    ).map_err(|error| self.db.error("failed to replace canonical team-roster member", error))?;
            }
            Ok(())
        })?;

        Ok(boundary::RosterStoreReplaceRosterResponse {
            team: request.team,
            previous_member_count,
            current_member_count: request.members.len() as u64,
            replaced: true,
        })
    }

    fn load_roster(
        &self,
        request: boundary::RosterStoreLoadRosterRequest,
    ) -> Result<boundary::RosterStoreLoadRosterResponse, AtmError> {
        let members = self.db.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT agent_name, member_kind, harness, agent_type, model, recipient_pane_id, metadata_json
                 FROM team_roster
                 WHERE team_name = ?1
                 ORDER BY agent_name ASC;",
            ).map_err(|error| self.db.error("failed to prepare canonical team-roster load", error))?;
            let rows = statement.query_map(params![request.team.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                ))
            }).map_err(|error| self.db.error("failed to load canonical team-roster rows", error))?;
            let mut members = Vec::new();
            for row in rows {
                let (
                    agent_name,
                    member_kind,
                    harness,
                    agent_type,
                    model,
                    recipient_pane_id,
                    metadata_json,
                ) = row
                    .map_err(|error| self.db.error("failed to decode canonical team-roster row", error))?;
                members.push(build_roster_member(
                    &request.team,
                    agent_name,
                    StoredRosterMemberRow {
                        member_kind,
                        harness,
                        agent_type,
                        model,
                        recipient_pane_id,
                        metadata_json,
                    },
                )?);
            }
            Ok(members)
        })?;

        if members.is_empty() {
            return Err(AtmError::validation(format!(
                "roster-store load failed because team {} has no persisted roster members",
                request.team
            ))
            .with_recovery(
                "Replace the roster through RosterStore::replace_roster before loading it.",
            ));
        }
        Ok(boundary::RosterStoreLoadRosterResponse {
            team: request.team,
            members,
        })
    }

    fn query_membership(
        &self,
        request: boundary::RosterStoreQueryMembershipRequest,
    ) -> Result<boundary::RosterStoreQueryMembershipResponse, AtmError> {
        let membership = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT member_kind, harness, agent_type, model, recipient_pane_id, metadata_json
                     FROM team_roster
                     WHERE team_name = ?1 AND agent_name = ?2;",
                    params![request.team.as_str(), request.member.as_str()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| {
                    self.db
                        .error("failed to query canonical team-roster membership", error)
                })
        })?;
        let member = membership
            .map(
                |(member_kind, harness, agent_type, model, recipient_pane_id, metadata_json)| {
                    build_roster_member(
                        &request.team,
                        request.member.as_str().to_string(),
                        StoredRosterMemberRow {
                            member_kind,
                            harness,
                            agent_type,
                            model,
                            recipient_pane_id,
                            metadata_json,
                        },
                    )
                },
            )
            .transpose()?;
        let is_member = member.is_some();

        Ok(boundary::RosterStoreQueryMembershipResponse {
            team: request.team,
            member,
            is_member,
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
                "roster-store health failed because team {} has no persisted roster members",
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

fn roster_member_kind_value(value: boundary::RosterMemberKind) -> &'static str {
    match value {
        boundary::RosterMemberKind::Permanent => "permanent",
        boundary::RosterMemberKind::Ephemeral => "ephemeral",
    }
}

fn roster_harness_value(value: boundary::RosterHarness) -> &'static str {
    match value {
        boundary::RosterHarness::ClaudeCode => "claude-code",
        boundary::RosterHarness::CodexCli => "codex-cli",
        boundary::RosterHarness::GeminiCli => "gemini-cli",
        boundary::RosterHarness::Opencode => "opencode",
    }
}

fn build_roster_member(
    team: &atm_core::types::TeamName,
    agent_name: String,
    stored: StoredRosterMemberRow,
) -> Result<boundary::RosterMemberRecord, AtmError> {
    let member_kind = match stored.member_kind.as_str() {
        "permanent" => boundary::RosterMemberKind::Permanent,
        "ephemeral" => boundary::RosterMemberKind::Ephemeral,
        other => {
            return Err(AtmError::validation(format!(
                "failed to decode roster member_kind {other} for {}/{}",
                team, agent_name
            )));
        }
    };
    let harness = match stored.harness.as_str() {
        "claude-code" => boundary::RosterHarness::ClaudeCode,
        "codex-cli" => boundary::RosterHarness::CodexCli,
        "gemini-cli" => boundary::RosterHarness::GeminiCli,
        "opencode" => boundary::RosterHarness::Opencode,
        other => {
            return Err(AtmError::validation(format!(
                "failed to decode roster harness {other} for {}/{}",
                team, agent_name
            )));
        }
    };
    let metadata_json: Map<String, Value> =
        deserialize_json(&stored.metadata_json, "team-roster metadata")?;
    Ok(boundary::RosterMemberRecord {
        team_name: team.clone(),
        agent_name: agent_name.parse().map_err(|error| {
            AtmError::validation(format!(
                "failed to decode roster agent_name {agent_name} for {team}: {error}"
            ))
            .with_source(error)
        })?,
        member_kind,
        harness,
        agent_type: stored.agent_type,
        model: stored.model,
        recipient_pane_id: stored.recipient_pane_id,
        metadata_json,
    })
}
