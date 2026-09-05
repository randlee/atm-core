use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};
use atm_storage::error::AtmError;

type TemplateOverrideRow = (String, String, String, String, String);

pub(crate) fn ensure_team_nudge_template_override_columns(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    crate::shared_db::ensure_column(
        connection,
        target,
        "team_nudge_template_overrides",
        "mode",
        "ALTER TABLE team_nudge_template_overrides ADD COLUMN mode TEXT NOT NULL DEFAULT 'override';",
    )?;
    connection
        .execute(
            "UPDATE team_nudge_template_overrides
             SET mode = 'disabled'
             WHERE mode = 'override' AND template_body = '';",
            [],
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to normalize legacy empty nudge-template override rows",
                error,
            )
        })?;
    Ok(())
}

fn load_template_override_rows(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<Vec<TemplateOverrideRow>, AtmError> {
    let mut statement = connection
        .prepare(
            "SELECT team_name, template_kind, mode, template_body, updated_at
             FROM team_nudge_template_overrides;",
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to read nudge-template overrides for migration",
                error,
            )
        })?;
    statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to enumerate nudge-template overrides for migration",
                error,
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to read nudge-template override row for migration",
                error,
            )
        })
}

fn create_seven_kind_override_table(
    transaction: &rusqlite::Transaction<'_>,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    transaction
        .execute_batch(
            "CREATE TABLE team_template_overrides_rebuild (
                team_name TEXT NOT NULL,
                template_kind TEXT NOT NULL
                    CHECK(template_kind IN (
                        'delivery', 'delivery_ack', 'queue', 'queue_ack', 'task',
                        'acknowledge', 'acknowledge_task'
                    )),
                mode TEXT NOT NULL DEFAULT 'override'
                    CHECK(mode IN ('override', 'disabled')),
                template_body TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (team_name, template_kind)
            );",
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to create seven-kind nudge-template override table",
                error,
            )
        })
}

fn copy_seven_kind_override_rows(
    transaction: &rusqlite::Transaction<'_>,
    target: &SharedDbTarget,
    rows: Vec<TemplateOverrideRow>,
) -> Result<(), AtmError> {
    for (team_name, template_kind, mode, template_body, updated_at) in rows {
        if matches!(
            template_kind.as_str(),
            "delivery_task" | "delivery_task_ack"
        ) {
            tracing::warn!(
                team = %team_name,
                kind = %template_kind,
                "dropping retired nudge-template override during seven-kind migration"
            );
            continue;
        }
        if !matches!(
            template_kind.as_str(),
            "delivery"
                | "delivery_ack"
                | "queue"
                | "queue_ack"
                | "task"
                | "acknowledge"
                | "acknowledge_task"
        ) {
            return Err(AtmError::validation(format!(
                "cannot migrate unknown nudge-template kind `{template_kind}` for team `{team_name}`"
            )));
        }
        transaction
            .execute(
                "INSERT INTO team_template_overrides_rebuild
                    (team_name, template_kind, mode, template_body, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5);",
                rusqlite::params![team_name, template_kind, mode, template_body, updated_at],
            )
            .map_err(|error| {
                sqlite_error(
                    target,
                    "failed to copy nudge-template override during seven-kind migration",
                    error,
                )
            })?;
    }
    Ok(())
}

fn replace_override_table(
    transaction: rusqlite::Transaction<'_>,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    transaction
        .execute_batch(
            "DROP TABLE team_nudge_template_overrides;
             ALTER TABLE team_template_overrides_rebuild RENAME TO team_nudge_template_overrides;
             CREATE INDEX IF NOT EXISTS idx_team_nudge_template_overrides_team_name
                 ON team_nudge_template_overrides(team_name);",
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to replace nudge-template override table",
                error,
            )
        })?;
    transaction.commit().map_err(|error| {
        sqlite_error(
            target,
            "failed to commit seven-kind nudge-template override migration",
            error,
        )
    })
}

pub(crate) fn migrate_template_override_kinds_to_seven(
    connection: &mut SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let table_sql = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'team_nudge_template_overrides';",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to inspect nudge-template override schema",
                error,
            )
        })?;
    let Some(table_sql) = table_sql else {
        return Ok(());
    };
    if table_sql.to_ascii_lowercase().contains("'queue'") {
        return Ok(());
    }

    let rows = load_template_override_rows(connection, target)?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to begin nudge-template override migration",
                error,
            )
        })?;
    create_seven_kind_override_table(&transaction, target)?;
    copy_seven_kind_override_rows(&transaction, target, rows)?;
    replace_override_table(transaction, target)
}
