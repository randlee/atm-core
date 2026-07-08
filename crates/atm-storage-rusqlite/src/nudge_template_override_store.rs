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
                    params![team.as_str(), template_kind_value(kind)],
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
}

fn template_kind_value(kind: BuiltInNudgeTemplateKind) -> &'static str {
    match kind {
        BuiltInNudgeTemplateKind::Delivery => "delivery",
        BuiltInNudgeTemplateKind::DeliveryAck => "delivery_ack",
        BuiltInNudgeTemplateKind::DeliveryTask => "delivery_task",
        BuiltInNudgeTemplateKind::DeliveryTaskAck => "delivery_task_ack",
        BuiltInNudgeTemplateKind::Acknowledge => "acknowledge",
        BuiltInNudgeTemplateKind::AcknowledgeTask => "acknowledge_task",
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
    use super::template_kind_value;
    use atm_core::boundary::BuiltInNudgeTemplateKind;

    #[test]
    fn sqlite_override_store_loads_saved_override_row() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        backend
            .message_store
            .db
            .with_connection(|connection| {
                connection
                    .execute(
                        "INSERT INTO team_nudge_template_overrides(
                            team_name, template_kind, template_body, updated_at
                         ) VALUES (?1, ?2, ?3, ?4);",
                        rusqlite::params![
                            "test-team",
                            template_kind_value(BuiltInNudgeTemplateKind::DeliveryAck),
                            "<atm/>",
                            chrono::Utc::now().to_rfc3339(),
                        ],
                    )
                    .map_err(|error| {
                        backend.message_store.db.error("insert override row", error)
                    })?;
                Ok(())
            })
            .expect("insert");

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
}
