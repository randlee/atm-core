use super::SqliteNudgeTemplateOverrideStore;
use crate::shared_db::{SharedDb, SqliteConnection};
use atm_storage::error::AtmError;
use atm_storage::types::{IsoTimestamp, TeamName};
use atm_storage::{
    BuiltInNudgeTemplateKind, NudgeTemplateOverrideStore, TeamNudgeTemplateOverrideMode,
    TeamNudgeTemplateOverrideRow,
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
    fn load_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
    ) -> Result<Option<TeamNudgeTemplateOverrideRow>, AtmError> {
        self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT mode, template_body, updated_at
                     FROM team_nudge_template_overrides
                     WHERE team_name = ?1 AND template_kind = ?2;",
                    params![team.as_str(), kind.as_str()],
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
                    self.db
                        .error("failed to load team nudge template override row", error)
                })?
                .map(|(mode, template_body, updated_at)| {
                    let mode = normalize_loaded_override_mode(
                        connection,
                        self.db.as_ref(),
                        team,
                        kind,
                        mode,
                        template_body,
                    )?;
                    Ok(TeamNudgeTemplateOverrideRow {
                        team_name: team.clone(),
                        kind,
                        mode,
                        updated_at: parse_updated_at(updated_at)?,
                    })
                })
                .transpose()
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

    fn clear_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
    ) -> Result<bool, AtmError> {
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "DELETE FROM team_nudge_template_overrides
                     WHERE team_name = ?1 AND template_kind = ?2;",
                    params![team.as_str(), kind.as_str()],
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
    connection: &SqliteConnection,
    db: &SharedDb,
    team: &TeamName,
    kind: BuiltInNudgeTemplateKind,
    mode: String,
    template_body: String,
) -> Result<TeamNudgeTemplateOverrideMode, AtmError> {
    match mode.as_str() {
        "override" => {
            if template_body.trim().is_empty() {
                connection
                    .execute(
                        "UPDATE team_nudge_template_overrides
                         SET mode = 'disabled'
                         WHERE team_name = ?1 AND template_kind = ?2;",
                        params![team.as_str(), kind.as_str()],
                    )
                    .map_err(|error| {
                        db.error(
                            "failed to normalize legacy empty nudge-template override row",
                            error,
                        )
                    })?;
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
            (BuiltInNudgeTemplateKind::Task, "<task/>"),
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
                BuiltInNudgeTemplateKind::Task,
            )
            .expect("lookup");
        assert!(row.is_none());
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
            .clear_template_override(&team, BuiltInNudgeTemplateKind::DeliveryAck)
            .expect("clear");
        assert!(cleared);
        let missing = backend
            .nudge_template_override_store()
            .load_template_override(&team, BuiltInNudgeTemplateKind::DeliveryAck)
            .expect("lookup");
        assert!(missing.is_none());
    }

    #[test]
    fn sqlite_override_store_migrates_legacy_empty_rows_to_disabled() {
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
    }
}
