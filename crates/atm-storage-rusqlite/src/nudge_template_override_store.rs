use super::SqliteNudgeTemplateOverrideStore;
use crate::shared_db::SharedDb;
use atm_core::boundary::{
    BuiltInNudgeTemplateKind, NudgeTemplateOverrideStore, TeamNudgeTemplateOverrideRow,
};
use atm_core::error::AtmError;
use atm_core::types::{IsoTimestamp, TeamName};
use rusqlite::{OptionalExtension, params};
use std::sync::Arc;

impl SqliteNudgeTemplateOverrideStore {
    pub(crate) fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }
}

impl atm_core::boundary::sealed::Sealed for SqliteNudgeTemplateOverrideStore {}

impl NudgeTemplateOverrideStore for SqliteNudgeTemplateOverrideStore {
    fn load_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
    ) -> Result<Option<TeamNudgeTemplateOverrideRow>, AtmError> {
        self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT template_body, updated_at
                     FROM team_nudge_template_overrides
                     WHERE team_name = ?1 AND template_kind = ?2;",
                    params![team.as_str(), kind.as_str()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()
                .map_err(|error| {
                    self.db
                        .error("failed to load team nudge template override row", error)
                })?
                .map(|(template_body, updated_at)| {
                    Ok(TeamNudgeTemplateOverrideRow {
                        team_name: team.clone(),
                        kind,
                        template_body,
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
        let updated_at = IsoTimestamp::now();
        let updated_at_raw = updated_at.to_string();
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO team_nudge_template_overrides(
                        team_name, template_kind, template_body, updated_at
                     ) VALUES (?1, ?2, ?3, ?4)
                    ON CONFLICT(team_name, template_kind) DO UPDATE SET
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
                template_body: template_body.to_string(),
                updated_at,
            })
        })
    }
}

fn parse_updated_at(raw: String) -> Result<IsoTimestamp, AtmError> {
    raw.parse::<chrono::DateTime<chrono::Utc>>()
        .map(IsoTimestamp::from_datetime)
        .map_err(|error| {
            AtmError::validation(format!(
                "failed to parse team_nudge_template_overrides.updated_at `{raw}`: {error}"
            ))
            .with_recovery(
                "Repair the malformed team_nudge_template_overrides.updated_at row before retrying the override lookup.",
            )
        })
}

#[cfg(test)]
mod tests {
    use atm_core::boundary::BuiltInNudgeTemplateKind;
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
        assert_eq!(row.template_body, "<atm/>");
        assert_eq!(saved.team_name, row.team_name);
        assert_eq!(saved.kind, row.kind);
        assert_eq!(saved.template_body, row.template_body);
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
}
