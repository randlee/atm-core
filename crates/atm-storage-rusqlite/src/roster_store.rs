use super::SqliteRosterStore;
use crate::shared_db::{deserialize_json, serialize_json};
use atm_storage::contract::{
    AgentType, RosterHarness, RosterMember, RosterMemberKind, RosterSnapshot, RosterStore,
    RosterUniqueName,
};
use atm_storage::types::{AgentName, ModelName, PaneId, TeamName};
use atm_storage::{AtmError, roster_unique_name_collision_error};
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
            enforce_roster_unique_names(transaction, roster)?;
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

    fn unique_names(&self) -> Result<Vec<RosterUniqueName>, AtmError> {
        self.db
            .with_connection(|connection| load_unique_names(connection))
    }
}

fn load_unique_names(connection: &rusqlite::Connection) -> Result<Vec<RosterUniqueName>, AtmError> {
    let mut statement = connection
        .prepare(
            "SELECT team_name, agent_name,
                    COALESCE(NULLIF(TRIM(json_extract(metadata_json, '$.alias')), ''), agent_name)
             FROM team_roster
             ORDER BY team_name ASC, agent_name ASC;",
        )
        .map_err(|error| {
            AtmError::validation(format!(
                "failed to prepare database-wide roster unique-name query: {error}"
            ))
        })?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| {
            AtmError::validation(format!(
                "failed to execute database-wide roster unique-name query: {error}"
            ))
        })?;
    rows.map(|row| {
        let (team_name, agent_name, unique_name) = row.map_err(|error| {
            AtmError::validation(format!(
                "failed to decode database-wide roster unique-name row: {error}"
            ))
        })?;
        Ok(RosterUniqueName {
            team_name: team_name.parse().map_err(|error| {
                AtmError::validation(format!(
                    "failed to parse roster unique-name team `{team_name}`: {error}"
                ))
            })?,
            agent_name: agent_name.parse().map_err(|error| {
                AtmError::validation(format!(
                    "failed to parse roster unique-name member `{agent_name}`: {error}"
                ))
            })?,
            unique_name,
        })
    })
    .collect()
}

/// Enforces the database-wide effective roster-name invariant in the same
/// immediate SQLite transaction as the replacement.  Preflight is solely an
/// operator convenience; this remains authoritative for all writers.
fn enforce_roster_unique_names(
    transaction: &rusqlite::Transaction<'_>,
    roster: &RosterSnapshot,
) -> Result<(), AtmError> {
    let mut statement = transaction
        .prepare(
            "SELECT team_name, agent_name,
                    COALESCE(NULLIF(TRIM(json_extract(metadata_json, '$.alias')), ''), agent_name)
             FROM team_roster
             WHERE team_name != ?1;",
        )
        .map_err(|error| {
            AtmError::validation(format!(
                "failed to prepare transactional roster unique-name validation: {error}"
            ))
        })?;
    let mut names = statement
        .query_map(params![roster.team_name.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| {
            AtmError::validation(format!(
                "failed to query transactional roster unique-name validation: {error}"
            ))
        })?
        .map(|row| {
            let (team_name, agent_name, unique_name) = row.map_err(|error| {
                AtmError::validation(format!(
                    "failed to decode transactional roster unique-name validation: {error}"
                ))
            })?;
            Ok(RosterUniqueName {
                team_name: team_name.parse().map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse transactional roster team `{team_name}`: {error}"
                    ))
                })?,
                agent_name: agent_name.parse().map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse transactional roster member `{agent_name}`: {error}"
                    ))
                })?,
                unique_name,
            })
        })
        .collect::<Result<Vec<_>, AtmError>>()?;
    names.extend(roster.members.iter().map(RosterUniqueName::from_member));

    let mut collisions = Vec::new();
    names.sort_by(|left, right| left.unique_name.cmp(&right.unique_name));
    let mut start = 0;
    while start < names.len() {
        let end = names[start + 1..]
            .iter()
            .position(|entry| entry.unique_name != names[start].unique_name)
            .map_or(names.len(), |offset| start + offset + 1);
        if end - start > 1 {
            collisions.extend_from_slice(&names[start..end]);
        }
        start = end;
    }
    if collisions.is_empty() {
        Ok(())
    } else {
        Err(roster_unique_name_collision_error(&collisions))
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

    fn roster_member(team: &str, agent: &str, alias: Option<&str>) -> RosterMember {
        let mut metadata_json = Map::new();
        if let Some(alias) = alias {
            metadata_json.insert("alias".to_owned(), Value::String(alias.to_owned()));
        }
        RosterMember {
            team_name: team.parse().expect("team"),
            agent_name: agent.parse().expect("agent"),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::CodexCli,
            agent_type: AgentType::Worker,
            model: ModelName::default(),
            recipient_pane_id: None,
            metadata_json,
        }
    }

    fn roster(team: &str, members: Vec<RosterMember>) -> RosterSnapshot {
        RosterSnapshot {
            team_name: team.parse().expect("team"),
            members,
            refreshed_at: None,
        }
    }

    fn seed_legacy_cross_team_duplicate(store: &SqliteRosterStore) {
        store
            .save_roster(&roster(
                "team-a",
                vec![roster_member("team-a", "alex", None)],
            ))
            .expect("first roster");
        store
            .db
            .with_connection(|connection| {
                connection
                    .execute(
                        "INSERT INTO team_roster(team_name, agent_name, member_kind, harness, agent_type, model, metadata_json, updated_at)
                         VALUES ('team-b', 'alex', 'permanent', 'codex-cli', 'worker', '', '{}', 'now');",
                        [],
                    )
                    .map_err(|error| {
                        AtmError::validation(format!("seed legacy collision: {error}"))
                    })?;
                Ok(())
            })
            .expect("seed legacy collision");
    }

    #[test]
    fn unique_name_permutations() {
        struct Case {
            id: &'static str,
            existing_name: &'static str,
            existing_alias: Option<&'static str>,
            proposed_team: &'static str,
            proposed_name: &'static str,
            proposed_alias: Option<&'static str>,
            expected_accept: bool,
        }

        let cases = [
            Case {
                id: "unique_name_a01_same_team_canonical_duplicate",
                existing_name: "bob",
                existing_alias: None,
                proposed_team: "team-a",
                proposed_name: "bob",
                proposed_alias: None,
                expected_accept: false,
            },
            Case {
                id: "unique_name_a05_alias_collides_other_team_canonical",
                existing_name: "bob",
                existing_alias: None,
                proposed_team: "team-b",
                proposed_name: "robert",
                proposed_alias: Some("bob"),
                expected_accept: false,
            },
            Case {
                id: "unique_name_a08_alias_collides_other_team_alias",
                existing_name: "robert",
                existing_alias: Some("bob"),
                proposed_team: "team-b",
                proposed_name: "sam",
                proposed_alias: Some("bob"),
                expected_accept: false,
            },
            Case {
                id: "unique_name_a06_canonical_collides_other_team_alias",
                existing_name: "robert",
                existing_alias: Some("bob"),
                proposed_team: "team-b",
                proposed_name: "bob",
                proposed_alias: None,
                expected_accept: false,
            },
            Case {
                id: "unique_name_a07_canonical_collides_same_team_alias",
                existing_name: "robert",
                existing_alias: Some("bob"),
                proposed_team: "team-a",
                proposed_name: "bob",
                proposed_alias: None,
                expected_accept: false,
            },
            Case {
                id: "unique_name_a09_canonical_may_match_aliased_member",
                existing_name: "bob",
                existing_alias: Some("bobby"),
                proposed_team: "team-b",
                proposed_name: "bob",
                proposed_alias: None,
                expected_accept: true,
            },
            Case {
                id: "unique_name_a10_alias_may_match_aliased_member_canonical",
                existing_name: "bob",
                existing_alias: Some("bobby"),
                proposed_team: "team-b",
                proposed_name: "sam",
                proposed_alias: Some("bob"),
                expected_accept: true,
            },
            Case {
                id: "unique_name_a18_whitespace_alias_is_absent",
                existing_name: "alex",
                existing_alias: None,
                proposed_team: "team-b",
                proposed_name: "alex",
                proposed_alias: Some("  "),
                expected_accept: false,
            },
        ];

        for case in cases {
            let store = SqliteStorageBackend::in_memory_for_test()
                .expect(case.id)
                .roster_store;
            store
                .save_roster(&roster(
                    "team-a",
                    vec![roster_member(
                        "team-a",
                        case.existing_name,
                        case.existing_alias,
                    )],
                ))
                .expect(case.id);
            let proposed_members = if case.proposed_team == "team-a" {
                vec![
                    roster_member("team-a", case.existing_name, case.existing_alias),
                    roster_member("team-a", case.proposed_name, case.proposed_alias),
                ]
            } else {
                vec![roster_member(
                    case.proposed_team,
                    case.proposed_name,
                    case.proposed_alias,
                )]
            };
            let result = store.save_roster(&roster(case.proposed_team, proposed_members));
            assert_eq!(result.is_ok(), case.expected_accept, "{}", case.id);
            if !case.expected_accept {
                let error = result.expect_err(case.id);
                assert!(
                    error
                        .message()
                        .contains(&format!("(team-a, {})", case.existing_name)),
                    "{}: {error}",
                    case.id
                );
                assert!(
                    error
                        .message()
                        .contains(&format!("({}, {})", case.proposed_team, case.proposed_name)),
                    "{}: {error}",
                    case.id
                );
                assert!(error.message().contains("--alias"), "{}: {error}", case.id);
            }
        }
    }

    #[test]
    fn unique_name_a26_legacy_collision_is_readable_but_next_write_fails() {
        let store = SqliteStorageBackend::in_memory_for_test()
            .expect("backend")
            .roster_store;
        seed_legacy_cross_team_duplicate(&store);

        assert_eq!(
            store
                .load_roster(&"team-b".parse().expect("team"))
                .expect("read")
                .members
                .len(),
            1
        );
        let error = store
            .save_roster(&roster(
                "team-b",
                vec![roster_member("team-b", "alex", None)],
            ))
            .expect_err("next write must enforce legacy collision");
        assert!(error.message().contains("(team-a, alex)"));
        assert!(error.message().contains("(team-b, alex)"));
    }

    #[test]
    fn unique_name_g03_durable_collision_uses_the_shared_error_contract() {
        let store = SqliteStorageBackend::in_memory_for_test()
            .expect("backend")
            .roster_store;
        store
            .save_roster(&roster(
                "team-a",
                vec![roster_member("team-a", "robert", Some("bob"))],
            ))
            .expect("seed owner");

        let error = store
            .save_roster(&roster(
                "team-b",
                vec![roster_member("team-b", "bob", None)],
            ))
            .expect_err("durable write must reject the collision");
        let expected = roster_unique_name_collision_error(&[
            RosterUniqueName {
                team_name: "team-a".parse().expect("team"),
                agent_name: "robert".parse().expect("agent"),
                unique_name: "bob".to_owned(),
            },
            RosterUniqueName {
                team_name: "team-b".parse().expect("team"),
                agent_name: "bob".parse().expect("agent"),
                unique_name: "bob".to_owned(),
            },
        ]);

        assert_eq!(error.code(), expected.code());
        assert_eq!(error.message(), expected.message());
    }

    #[test]
    fn unique_name_a27_legacy_collision_blocks_an_unrelated_next_write() {
        let store = SqliteStorageBackend::in_memory_for_test()
            .expect("backend")
            .roster_store;
        seed_legacy_cross_team_duplicate(&store);

        let error = store
            .save_roster(&roster(
                "team-b",
                vec![
                    roster_member("team-b", "alex", None),
                    roster_member("team-b", "carol", None),
                ],
            ))
            .expect_err("legacy conflict blocks unrelated write");

        assert!(error.message().contains("(team-a, alex)"));
        assert_eq!(
            store
                .load_roster(&"team-b".parse().expect("team"))
                .expect("roster remains readable")
                .members
                .len(),
            1
        );
    }

    #[test]
    fn unique_name_a28_aliasing_the_legacy_conflict_allows_the_next_write() {
        let store = SqliteStorageBackend::in_memory_for_test()
            .expect("backend")
            .roster_store;
        seed_legacy_cross_team_duplicate(&store);

        store
            .save_roster(&roster(
                "team-b",
                vec![roster_member("team-b", "alex", Some("bobby"))],
            ))
            .expect("alias removes conflict");
        store
            .save_roster(&roster(
                "team-b",
                vec![
                    roster_member("team-b", "alex", Some("bobby")),
                    roster_member("team-b", "carol", None),
                ],
            ))
            .expect("unrelated write succeeds after aliasing");
    }

    #[test]
    fn unique_name_a29_removing_the_legacy_conflict_allows_the_next_write() {
        let store = SqliteStorageBackend::in_memory_for_test()
            .expect("backend")
            .roster_store;
        seed_legacy_cross_team_duplicate(&store);

        store
            .save_roster(&roster(
                "team-b",
                vec![roster_member("team-b", "carol", None)],
            ))
            .expect("removal removes conflict before next write");
    }

    #[test]
    fn unique_name_a19_removing_a_member_frees_its_effective_name() {
        let store = SqliteStorageBackend::in_memory_for_test()
            .expect("backend")
            .roster_store;
        store
            .save_roster(&roster(
                "team-a",
                vec![roster_member("team-a", "bob", Some("bobby"))],
            ))
            .expect("seed aliased member");
        store
            .save_roster(&roster("team-a", vec![]))
            .expect("remove member");

        store
            .save_roster(&roster(
                "team-b",
                vec![roster_member("team-b", "sam", Some("bobby"))],
            ))
            .expect("removed effective name is available");
    }

    #[test]
    fn unique_name_a20_removing_a_team_roster_frees_its_canonical_name() {
        let store = SqliteStorageBackend::in_memory_for_test()
            .expect("backend")
            .roster_store;
        store
            .save_roster(&roster(
                "team-a",
                vec![roster_member("team-a", "bob", None)],
            ))
            .expect("seed team roster");
        store
            .save_roster(&roster("team-a", vec![]))
            .expect("remove team roster");

        store
            .save_roster(&roster(
                "team-b",
                vec![roster_member("team-b", "bob", None)],
            ))
            .expect("removed team canonical name is available");
    }

    #[test]
    fn unique_name_a23_store_rejects_non_cli_canonical_name_collisions() {
        let store = SqliteStorageBackend::in_memory_for_test()
            .expect("backend")
            .roster_store;
        store
            .save_roster(&roster(
                "team-a",
                vec![roster_member("team-a", "bob", None)],
            ))
            .expect("first roster");

        let error = store
            .save_roster(&roster(
                "team-b",
                vec![roster_member("team-b", "bob", None)],
            ))
            .expect_err("store boundary rejects a bypassing caller");
        assert!(error.message().contains("(team-a, bob)"));
        assert!(error.message().contains("(team-b, bob)"));
    }

    #[test]
    fn unique_name_a24_store_rejects_imported_cross_team_collision() {
        let store = SqliteStorageBackend::in_memory_for_test()
            .expect("backend")
            .roster_store;
        store
            .save_roster(&roster(
                "team-a",
                vec![roster_member("team-a", "bob", None)],
            ))
            .expect("first roster");

        let imported = roster("team-b", vec![roster_member("team-b", "bob", None)]);
        let error = store
            .save_roster(&imported)
            .expect_err("roster import uses the same durable enforcement");
        assert!(error.message().contains("(team-a, bob)"));
    }

    #[test]
    fn unique_name_a25_compares_effective_names_case_sensitively() {
        let store = SqliteStorageBackend::in_memory_for_test()
            .expect("backend")
            .roster_store;
        store
            .save_roster(&roster(
                "team-a",
                vec![roster_member("team-a", "Bob", None)],
            ))
            .expect("uppercase canonical name");
        store
            .save_roster(&roster(
                "team-b",
                vec![roster_member("team-b", "bob", None)],
            ))
            .expect("case-distinct name remains available");
    }

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
