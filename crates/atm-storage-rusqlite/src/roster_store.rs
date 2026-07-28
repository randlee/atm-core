use super::SqliteRosterStore;
use crate::shared_db::{deserialize_json, serialize_json};
use atm_storage::AtmError;
use atm_storage::contract::{
    AgentType, RosterHarness, RosterMember, RosterMemberKind, RosterSnapshot, RosterStore,
};
use atm_storage::types::{AgentName, ModelName, PaneId, TeamName};
use rusqlite::params;
use serde_json::{Map, Value};

const MAX_CANONICAL_ROSTER_MEMBERS: usize = 4096;
struct StoredRosterMemberRow {
    member_kind: String,
    harness: String,
    agent_type: String,
    model: String,
    recipient_pane_id: Option<String>,
    metadata_json: String,
}

impl RosterStore for SqliteRosterStore {
    fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError> {
        let members = self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT agent_name, member_kind, harness, agent_type, model, recipient_pane_id, metadata_json
                     FROM team_roster
                     WHERE team_name = ?1
                     ORDER BY agent_name ASC
                     LIMIT ?2;",
                )
                .map_err(|error| self.db.error("failed to prepare canonical team-roster load", error))?;
            let rows = statement
                .query_map(
                    params![team.as_str(), (MAX_CANONICAL_ROSTER_MEMBERS as i64) + 1],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, String>(6)?,
                        ))
                    },
                )
                .map_err(|error| self.db.error("failed to load canonical team-roster rows", error))?;
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
                    team,
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
            if members.len() > MAX_CANONICAL_ROSTER_MEMBERS {
                return Err(AtmError::validation(format!(
                    "roster-store load rejected team {} because persisted roster rows exceeded the canonical cap of {}",
                    team, MAX_CANONICAL_ROSTER_MEMBERS
                ))
                );
            }
            Ok(members)
        })?;

        Ok(RosterSnapshot {
            team_name: team.clone(),
            members,
            refreshed_at: None,
        })
    }

    fn save_roster(&self, roster: &RosterSnapshot) -> Result<(), AtmError> {
        let team = &roster.team_name;
        if roster.members.len() > MAX_CANONICAL_ROSTER_MEMBERS {
            return Err(AtmError::validation(format!(
                "roster-store replace rejected team {} because {} members exceeds the canonical roster cap of {}",
                team,
                roster.members.len(),
                MAX_CANONICAL_ROSTER_MEMBERS
            )));
        }

        let updated_at = chrono::Utc::now().to_rfc3339();
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "DELETE FROM team_roster WHERE team_name = ?1;",
                    params![team.as_str()],
                )
                .map_err(|error| self.db.error("failed to clear canonical team roster", error))?;
            for member in &roster.members {
                if member.team_name != *team {
                    return Err(AtmError::validation(format!(
                        "roster-store replace rejected member {} because team_name {} did not match request team {}",
                        member.agent_name, member.team_name, team
                    ))
                    );
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
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9);",
                        params![
                            team.as_str(),
                            member.agent_name.as_str(),
                            roster_member_kind_value(member.member_kind),
                            roster_harness_value(member.harness),
                            member.agent_type.to_string(),
                            member.model.to_string(),
                            metadata_json,
                            member.recipient_pane_id.as_ref().map(ToString::to_string),
                            updated_at.clone(),
                        ],
                    )
                    .map_err(|error| self.db.error("failed to replace canonical team-roster member", error))?;
            }
            Ok(())
        })
    }

    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
        self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT DISTINCT team_name
                     FROM team_roster
                     ORDER BY team_name ASC;",
                )
                .map_err(|error| {
                    self.db
                        .error("failed to prepare canonical roster team enumeration", error)
                })?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| {
                    self.db
                        .error("failed to execute canonical roster team enumeration", error)
                })?;
            let mut teams = Vec::new();
            for row in rows {
                let raw = row.map_err(|error| {
                    self.db
                        .error("failed to decode canonical roster team row", error)
                })?;
                teams.push(raw.parse::<TeamName>().map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse canonical roster team_name `{raw}`: {error}"
                    ))
                })?);
            }
            Ok(teams)
        })
    }
}

fn build_roster_member(
    team: &TeamName,
    agent_name: String,
    row: StoredRosterMemberRow,
) -> Result<RosterMember, AtmError> {
    let agent_name = agent_name.parse::<AgentName>().map_err(|error| {
        AtmError::validation(format!(
            "failed to parse canonical team-roster agent_name `{agent_name}`: {error}"
        ))
    })?;
    let metadata_json =
        deserialize_json::<Map<String, Value>>(&row.metadata_json, "team-roster metadata")?;
    let model = if row.model.is_empty() {
        ModelName::default()
    } else {
        ModelName::new(row.model).map_err(|error| {
            AtmError::validation(format!(
                "failed to parse canonical team-roster model: {error}"
            ))
        })?
    };
    let recipient_pane_id = row
        .recipient_pane_id
        .map(|pane| {
            PaneId::new(pane.clone()).map_err(|error| {
                AtmError::validation(format!(
                    "failed to parse canonical team-roster recipient_pane_id `{pane}`: {error}"
                ))
            })
        })
        .transpose()?;

    Ok(RosterMember {
        team_name: team.clone(),
        agent_name,
        member_kind: parse_member_kind(&row.member_kind)?,
        harness: parse_harness(&row.harness)?,
        agent_type: AgentType::from(row.agent_type),
        model,
        recipient_pane_id,
        metadata_json,
    })
}

fn parse_member_kind(raw: &str) -> Result<RosterMemberKind, AtmError> {
    match raw {
        "permanent" => Ok(RosterMemberKind::Permanent),
        "ephemeral" => Ok(RosterMemberKind::Ephemeral),
        other => Err(AtmError::validation(format!(
            "failed to parse canonical team-roster member_kind `{other}`"
        ))),
    }
}

fn parse_harness(raw: &str) -> Result<RosterHarness, AtmError> {
    match raw {
        "claude-code" => Ok(RosterHarness::ClaudeCode),
        "codex-cli" => Ok(RosterHarness::CodexCli),
        "gemini-cli" => Ok(RosterHarness::GeminiCli),
        "opencode" => Ok(RosterHarness::Opencode),
        "hermes" => Ok(RosterHarness::Hermes),
        "python-graft" => Ok(RosterHarness::PythonGraft),
        other => Err(AtmError::validation(format!(
            "failed to parse canonical team-roster harness `{other}`"
        ))),
    }
}

fn roster_member_kind_value(kind: RosterMemberKind) -> &'static str {
    match kind {
        RosterMemberKind::Permanent => "permanent",
        RosterMemberKind::Ephemeral => "ephemeral",
    }
}

fn roster_harness_value(harness: RosterHarness) -> &'static str {
    match harness {
        RosterHarness::ClaudeCode => "claude-code",
        RosterHarness::CodexCli => "codex-cli",
        RosterHarness::GeminiCli => "gemini-cli",
        RosterHarness::Opencode => "opencode",
        RosterHarness::Hermes => "hermes",
        RosterHarness::PythonGraft => "python-graft",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SqliteStorageBackend;
    use atm_storage::IsoTimestamp;

    const TEST_WORKER: &str = "worker";

    #[test]
    fn save_and_load_support_python_graft_harnesses() {
        let store = SqliteStorageBackend::in_memory_for_test()
            .expect("backend")
            .roster_store;
        let team: TeamName = "team-a".parse().expect("team");
        let roster = RosterSnapshot {
            team_name: team.clone(),
            members: vec![
                RosterMember {
                    team_name: team.clone(),
                    agent_name: "hermes-agent".parse().expect("agent"),
                    member_kind: RosterMemberKind::Permanent,
                    harness: RosterHarness::Hermes,
                    agent_type: AgentType::Worker,
                    model: ModelName::default(),
                    recipient_pane_id: None,
                    metadata_json: Map::new(),
                },
                RosterMember {
                    team_name: team.clone(),
                    agent_name: "python-agent".parse().expect("agent"),
                    member_kind: RosterMemberKind::Permanent,
                    harness: RosterHarness::PythonGraft,
                    agent_type: AgentType::Worker,
                    model: ModelName::default(),
                    recipient_pane_id: None,
                    metadata_json: Map::new(),
                },
            ],
            refreshed_at: None,
        };

        store.save_roster(&roster).expect("save roster");
        let loaded = store.load_roster(&team).expect("load roster");
        assert_eq!(
            loaded
                .members
                .iter()
                .map(|member| member.harness)
                .collect::<Vec<_>>(),
            vec![RosterHarness::Hermes, RosterHarness::PythonGraft]
        );
    }

    #[test]
    fn save_roster_rejects_mismatched_team_names() {
        let store = SqliteStorageBackend::in_memory_for_test()
            .expect("backend")
            .roster_store;
        let team: TeamName = "team-a".parse().expect("team");
        let other_team: TeamName = "team-b".parse().expect("team");
        let agent: AgentName = TEST_WORKER.parse().expect("agent");
        let roster = RosterSnapshot {
            team_name: team,
            members: vec![RosterMember {
                team_name: other_team,
                agent_name: agent,
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: AgentType::Worker,
                model: ModelName::default(),
                recipient_pane_id: None,
                metadata_json: Map::new(),
            }],
            refreshed_at: Some(IsoTimestamp::now()),
        };

        let error = store.save_roster(&roster).expect_err("mismatch");
        assert!(error.message().contains("did not match request team"));
    }
}
