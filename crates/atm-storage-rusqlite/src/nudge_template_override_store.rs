use super::SqliteNudgeTemplateOverrideStore;
use crate::shared_db::SharedDb;
use atm_storage::error::AtmError;
use atm_storage::types::{IsoTimestamp, TeamName};
use atm_storage::{
    BuiltInNudgeTemplateKind, NudgeTemplateOverrideStore, StaleNudgeTemplateOverrideKind,
    TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow,
};
use rusqlite::{OptionalExtension, params};
use std::sync::Arc;

impl SqliteNudgeTemplateOverrideStore {
    pub(crate) fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }
}

impl atm_storage::contract::sealed::Sealed for SqliteNudgeTemplateOverrideStore {}

impl NudgeTemplateOverrideStore for SqliteNudgeTemplateOverrideStore {
    fn list_stale_template_override_kinds(
        &self,
        team: &TeamName,
    ) -> Result<Vec<StaleNudgeTemplateOverrideKind>, AtmError> {
        let db = Arc::clone(&self.db);
        let team_key = team.clone();
        self.db.read(move |connection| {
            let mut statement = connection
                .prepare(
                    "SELECT template_kind, updated_at
                     FROM team_nudge_template_overrides
                     WHERE team_name = ?1
                     ORDER BY template_kind;",
                )
                .map_err(|error| {
                    db.error(
                        "failed to prepare stale nudge template override query",
                        error,
                    )
                })?;
            let rows = statement
                .query_map(params![team_key.as_str()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| {
                    db.error("failed to list stale nudge template override rows", error)
                })?;
            let mut stale = Vec::new();
            for row in rows {
                let (kind, updated_at) = row.map_err(|error| {
                    db.error("failed to read stale nudge template override row", error)
                })?;
                if kind.parse::<BuiltInNudgeTemplateKind>().is_err() {
                    stale.push(StaleNudgeTemplateOverrideKind {
                        kind,
                        updated_at: parse_updated_at(updated_at)?,
                    });
                }
            }
            Ok(stale)
        })
    }

    fn load_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
    ) -> Result<Option<TeamNudgeTemplateOverrideRow>, AtmError> {
        let db = Arc::clone(&self.db);
        let team_key = team.clone();
        self.db.read(move |connection| {
            connection
                .query_row(
                    "SELECT mode, template_body, updated_at
                     FROM team_nudge_template_overrides
                     WHERE team_name = ?1 AND template_kind = ?2;",
                    params![team_key.as_str(), kind.as_str()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| {
                    db.error("failed to load team nudge template override row", error)
                })?
                .map(|(mode, template_body, updated_at)| {
                    let mode = normalize_loaded_override_mode(mode, template_body)?;
                    Ok(TeamNudgeTemplateOverrideRow {
                        team_name: team_key.clone(),
                        kind,
                        mode,
                        updated_at: parse_updated_at(updated_at)?,
                    })
                })
                .transpose()
        })
    }

    fn list_template_overrides(
        &self,
        team: &TeamName,
    ) -> Result<Vec<TeamNudgeTemplateOverrideRow>, AtmError> {
        let db = Arc::clone(&self.db);
        let team_key = team.clone();
        self.db.read(move |connection| {
            let mut statement = connection
                .prepare(
                    "SELECT template_kind, mode, template_body, updated_at
                     FROM team_nudge_template_overrides
                     WHERE team_name = ?1
                     ORDER BY template_kind;",
                )
                .map_err(|error| db.error("failed to prepare template override query", error))?;
            let rows = statement
                .query_map(params![team_key.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(|error| db.error("failed to list template override rows", error))?;
            let mut overrides = Vec::new();
            for row in rows {
                let (kind, mode, template_body, updated_at) =
                    row.map_err(|error| db.error("failed to read template override row", error))?;
                let Ok(kind) = kind.parse::<BuiltInNudgeTemplateKind>() else {
                    continue;
                };
                overrides.push(TeamNudgeTemplateOverrideRow {
                    team_name: team_key.clone(),
                    kind,
                    mode: normalize_loaded_override_mode(mode, template_body)?,
                    updated_at: parse_updated_at(updated_at)?,
                });
            }
            Ok(overrides)
        })
    }

    fn save_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
        template_body: &str,
    ) -> Result<TeamNudgeTemplateOverrideRow, AtmError> {
        if template_body.trim().is_empty() {
            return Err(AtmError::empty_nudge_template_body());
        }
        let updated_at = IsoTimestamp::now();
        let updated_at_raw = updated_at.to_string();
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO team_nudge_template_overrides(
                        team_name, template_kind, mode, template_body, updated_at
                     ) VALUES (?1, ?2, 'override', ?3, ?4)
                    ON CONFLICT(team_name, template_kind) DO UPDATE SET
                        mode = excluded.mode,
                        template_body = excluded.template_body,
                        updated_at = excluded.updated_at;",
                    params![team.as_str(), kind.as_str(), template_body, &updated_at_raw,],
                )
                .map_err(|error| {
                    self.db
                        .error("failed to save team nudge template override row", error)
                })?;
            Ok(TeamNudgeTemplateOverrideRow {
                team_name: team.clone(),
                kind,
                mode: TeamNudgeTemplateOverrideMode::Override {
                    template_body: template_body.to_string(),
                },
                updated_at,
            })
        })
    }

    fn disable_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
    ) -> Result<TeamNudgeTemplateOverrideRow, AtmError> {
        let updated_at = IsoTimestamp::now();
        let updated_at_raw = updated_at.to_string();
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO team_nudge_template_overrides(
                        team_name, template_kind, mode, template_body, updated_at
                     ) VALUES (?1, ?2, 'disabled', '', ?3)
                    ON CONFLICT(team_name, template_kind) DO UPDATE SET
                        mode = excluded.mode,
                        template_body = excluded.template_body,
                        updated_at = excluded.updated_at;",
                    params![team.as_str(), kind.as_str(), &updated_at_raw],
                )
                .map_err(|error| {
                    self.db
                        .error("failed to disable team nudge template override row", error)
                })?;
            Ok(TeamNudgeTemplateOverrideRow {
                team_name: team.clone(),
                kind,
                mode: TeamNudgeTemplateOverrideMode::Disabled,
                updated_at,
            })
        })
    }

    fn clear_template_override(&self, team: &TeamName, kind: &str) -> Result<bool, AtmError> {
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "DELETE FROM team_nudge_template_overrides
                     WHERE team_name = ?1 AND template_kind = ?2;",
                    params![team.as_str(), kind],
                )
                .map(|count| count > 0)
                .map_err(|error| {
                    self.db
                        .error("failed to clear team nudge template override row", error)
                })
        })
    }
}

fn parse_updated_at(raw: String) -> Result<IsoTimestamp, AtmError> {
    raw.parse::<IsoTimestamp>().map_err(|error| {
        AtmError::validation(format!(
            "failed to parse team_nudge_template_overrides.updated_at `{raw}`: {error}"
        ))
    })
}

fn normalize_loaded_override_mode(
    mode: String,
    template_body: String,
) -> Result<TeamNudgeTemplateOverrideMode, AtmError> {
    match mode.as_str() {
        "override" => {
            if template_body.trim().is_empty() {
                return Ok(TeamNudgeTemplateOverrideMode::Disabled);
            }
            Ok(TeamNudgeTemplateOverrideMode::Override { template_body })
        }
        "disabled" => Ok(TeamNudgeTemplateOverrideMode::Disabled),
        _ => Err(AtmError::validation(format!(
            "failed to parse team_nudge_template_overrides.mode `{mode}`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use atm_storage::BuiltInNudgeTemplateKind;
    use atm_storage::TeamNudgeTemplateOverrideMode;
    use rusqlite::params;

    #[test]
    fn sqlite_override_store_saves_and_loads_override_row() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        let saved = backend
            .nudge_template_override_store()
            .save_template_override(
                &"test-team".parse().expect("team"),
                BuiltInNudgeTemplateKind::DeliveryAck,
                "<atm/>",
            )
            .expect("save");

        let row = backend
            .nudge_template_override_store()
            .load_template_override(
                &"test-team".parse().expect("team"),
                BuiltInNudgeTemplateKind::DeliveryAck,
            )
            .expect("lookup")
            .expect("row");

        assert_eq!(row.team_name.as_str(), "test-team");
        assert_eq!(row.kind, BuiltInNudgeTemplateKind::DeliveryAck);
        assert_eq!(row.template_body(), Some("<atm/>"));
        assert_eq!(saved.team_name, row.team_name);
        assert_eq!(saved.kind, row.kind);
        assert_eq!(saved.template_body(), row.template_body());
    }

    #[test]
    fn sqlite_override_store_round_trips_queue_family_and_task_kinds() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        let team = "test-team".parse().expect("team");
        for (kind, body) in [
            (BuiltInNudgeTemplateKind::Queue, "<queue/>"),
            (BuiltInNudgeTemplateKind::QueueAck, "<queue-ack/>"),
            (BuiltInNudgeTemplateKind::TaskReady, "<task/>"),
        ] {
            backend
                .nudge_template_override_store()
                .save_template_override(&team, kind, body)
                .expect("save");
            let row = backend
                .nudge_template_override_store()
                .load_template_override(&team, kind)
                .expect("lookup")
                .expect("row");
            assert_eq!(row.kind, kind);
            assert_eq!(row.template_body(), Some(body));
        }
    }

    #[test]
    fn sqlite_override_store_returns_none_for_missing_row() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");

        let row = backend
            .nudge_template_override_store()
            .load_template_override(
                &"test-team".parse().expect("team"),
                BuiltInNudgeTemplateKind::Delivery,
            )
            .expect("lookup");

        assert!(row.is_none());
    }

    #[test]
    fn sqlite_override_store_returns_none_for_retired_kind_after_migration() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        let db = backend.shared_db_for_test();
        db.with_connection(|connection| {
            connection
                .execute_batch(
                    "DROP TABLE team_nudge_template_overrides;
                     CREATE TABLE team_nudge_template_overrides (
                         team_name TEXT NOT NULL,
                         template_kind TEXT NOT NULL CHECK(template_kind IN (
                             'delivery', 'delivery_ack', 'delivery_task', 'delivery_task_ack',
                             'acknowledge', 'acknowledge_task'
                         )),
                         template_body TEXT NOT NULL,
                         updated_at TEXT NOT NULL,
                         PRIMARY KEY (team_name, template_kind)
                     );
                     INSERT INTO team_nudge_template_overrides
                         (team_name, template_kind, template_body, updated_at)
                     VALUES ('test-team', 'delivery_task', '<retired/>', '2026-09-05T00:00:00Z');",
                )
                .map_err(|error| db.error("failed to seed retired override row", error))?;
            crate::shared_db::ensure_schema(connection, db.target())
        })
        .expect("migrate retired override row");

        let row = backend
            .nudge_template_override_store()
            .load_template_override(
                &"test-team".parse().expect("team"),
                BuiltInNudgeTemplateKind::TaskReady,
            )
            .expect("lookup");
        assert!(row.is_none());
    }

    #[test]
    fn override_row_with_retired_kind_is_listed_stale_and_never_loaded() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        let db = backend.shared_db_for_test();
        db.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO team_nudge_template_overrides
                         (team_name, template_kind, mode, template_body, updated_at)
                     VALUES ('test-team', 'task', 'override', '<retired/>', '2026-09-12T00:00:00Z');",
                    [],
                )
                .map_err(|error| db.error("failed to seed stale override row", error))?;
            Ok(())
        })
        .expect("seed stale row");

        let team = "test-team".parse().expect("team");
        let stale = backend
            .nudge_template_override_store()
            .list_stale_template_override_kinds(&team)
            .expect("list stale rows");
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].kind, "task");
        assert!(
            backend
                .nudge_template_override_store()
                .load_template_override(&team, BuiltInNudgeTemplateKind::TaskReady)
                .expect("load replacement kind")
                .is_none()
        );
        assert!(
            backend
                .nudge_template_override_store()
                .clear_template_override(&team, "task")
                .expect("clear stale row")
        );
        assert!(
            backend
                .nudge_template_override_store()
                .list_stale_template_override_kinds(&team)
                .expect("list after clear")
                .is_empty()
        );
    }

    #[test]
    fn sqlite_override_store_returns_none_after_override_row_is_deleted() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        let team = "test-team".parse().expect("team");
        let kind = BuiltInNudgeTemplateKind::Delivery;

        backend
            .nudge_template_override_store()
            .save_template_override(&team, kind, "<atm kind=\"override\"/>")
            .expect("save");

        backend
            .nudge_template_override_store
            .db
            .with_connection(|connection| {
                connection
                    .execute(
                        "DELETE FROM team_nudge_template_overrides
                         WHERE team_name = ?1 AND template_kind = ?2;",
                        params![team.as_str(), kind.as_str()],
                    )
                    .map_err(|error| {
                        backend.nudge_template_override_store.db.error(
                            "failed to delete team nudge template override row in test",
                            error,
                        )
                    })?;
                Ok(())
            })
            .expect("delete override row");

        let row = backend
            .nudge_template_override_store()
            .load_template_override(&team, kind)
            .expect("lookup");

        assert!(row.is_none());
    }

    #[test]
    fn sqlite_override_store_rejects_empty_override_body() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");

        let error = backend
            .nudge_template_override_store()
            .save_template_override(
                &"test-team".parse().expect("team"),
                BuiltInNudgeTemplateKind::DeliveryAck,
                "   ",
            )
            .expect_err("empty override");

        assert_eq!(
            error.code(),
            atm_storage::error_codes::AtmErrorCode::EmptyNudgeTemplateBody
        );
    }

    #[test]
    fn sqlite_override_store_disables_and_clears_rows() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        let team = "test-team".parse().expect("team");

        let disabled = backend
            .nudge_template_override_store()
            .disable_template_override(&team, BuiltInNudgeTemplateKind::DeliveryAck)
            .expect("disable");
        assert!(matches!(
            disabled.mode,
            TeamNudgeTemplateOverrideMode::Disabled
        ));

        let row = backend
            .nudge_template_override_store()
            .load_template_override(&team, BuiltInNudgeTemplateKind::DeliveryAck)
            .expect("lookup")
            .expect("row");
        assert!(row.is_disabled());
        assert_eq!(row.template_body(), None);

        let cleared = backend
            .nudge_template_override_store()
            .clear_template_override(&team, BuiltInNudgeTemplateKind::DeliveryAck.as_str())
            .expect("clear");
        assert!(cleared);
        let missing = backend
            .nudge_template_override_store()
            .load_template_override(&team, BuiltInNudgeTemplateKind::DeliveryAck)
            .expect("lookup");
        assert!(missing.is_none());
    }

    #[test]
    fn sqlite_override_store_treats_legacy_empty_rows_as_disabled_without_writing() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        let db = backend.shared_db_for_test();
        db.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO team_nudge_template_overrides(
                            team_name, template_kind, mode, template_body, updated_at
                         ) VALUES (?1, ?2, 'override', '', ?3);",
                    rusqlite::params![
                        "test-team",
                        "delivery_ack",
                        atm_storage::types::IsoTimestamp::now().to_string()
                    ],
                )
                .map_err(|error| db.error("insert legacy row", error))?;
            Ok(())
        })
        .expect("seed legacy row");

        let row = backend
            .nudge_template_override_store()
            .load_template_override(
                &"test-team".parse().expect("team"),
                BuiltInNudgeTemplateKind::DeliveryAck,
            )
            .expect("lookup")
            .expect("row");

        assert!(row.is_disabled());
        let persisted_mode = db
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT mode FROM team_nudge_template_overrides
                         WHERE team_name = ?1 AND template_kind = ?2;",
                        params!["test-team", "delivery_ack"],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(|error| db.error("load legacy mode", error))
            })
            .expect("legacy row remains readable");
        assert_eq!(persisted_mode, "override");
    }
}
