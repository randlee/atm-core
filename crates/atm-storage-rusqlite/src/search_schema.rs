//! SQLite-owned FTS projections for the AN.5 search capability.
//!
//! The tables below are external-content FTS5 indexes.  Projection writes are
//! performed by the same SQLite transaction that changes their durable source
//! rows; triggers keep the virtual table synchronized with its projection.

use serde_json::Value;

use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};

// Keep incremental FTS5 merge work out of the canonical write hot path for a
// larger burst, while retaining bounded automatic maintenance and immediate
// transactional index visibility. Higher values defer more merge work to a
// later insert; 8 is the smallest measured improvement over SQLite's default
// of 4 on the M5 projection workload.
const MESSAGE_FTS_AUTOMERGE: i64 = 8;

const SEARCH_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS mail_message_search_documents (
    search_rowid INTEGER PRIMARY KEY,
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    message_id TEXT NULL,
    message_at TEXT NOT NULL,
    body_text TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    tags TEXT NOT NULL DEFAULT '',
    var_values TEXT NOT NULL DEFAULT '',
    from_agent TEXT NOT NULL,
    UNIQUE(team, agent, message_key)
);
CREATE VIRTUAL TABLE IF NOT EXISTS mail_messages_fts USING fts5(
    body_text, summary, tags, var_values, from_agent,
    content='mail_message_search_documents',
    content_rowid='search_rowid',
    tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER IF NOT EXISTS mail_message_search_documents_ai
AFTER INSERT ON mail_message_search_documents BEGIN
    INSERT INTO mail_messages_fts(rowid, body_text, summary, tags, var_values, from_agent)
    VALUES (new.search_rowid, new.body_text, new.summary, new.tags, new.var_values, new.from_agent);
END;
CREATE TRIGGER IF NOT EXISTS mail_message_search_documents_ad
AFTER DELETE ON mail_message_search_documents BEGIN
    INSERT INTO mail_messages_fts(mail_messages_fts, rowid, body_text, summary, tags, var_values, from_agent)
    VALUES ('delete', old.search_rowid, old.body_text, old.summary, old.tags, old.var_values, old.from_agent);
END;
CREATE TRIGGER IF NOT EXISTS mail_message_search_documents_au
AFTER UPDATE ON mail_message_search_documents BEGIN
    INSERT INTO mail_messages_fts(mail_messages_fts, rowid, body_text, summary, tags, var_values, from_agent)
    VALUES ('delete', old.search_rowid, old.body_text, old.summary, old.tags, old.var_values, old.from_agent);
    INSERT INTO mail_messages_fts(rowid, body_text, summary, tags, var_values, from_agent)
    VALUES (new.search_rowid, new.body_text, new.summary, new.tags, new.var_values, new.from_agent);
END;

CREATE TABLE IF NOT EXISTS message_template_search_documents (
    search_rowid INTEGER PRIMARY KEY,
    template_sha TEXT UNIQUE NOT NULL,
    content_text TEXT NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS message_templates_fts USING fts5(
    content_text,
    content='message_template_search_documents',
    content_rowid='search_rowid',
    tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER IF NOT EXISTS message_template_search_documents_ai
AFTER INSERT ON message_template_search_documents BEGIN
    INSERT INTO message_templates_fts(rowid, content_text)
    VALUES (new.search_rowid, new.content_text);
END;
CREATE TRIGGER IF NOT EXISTS message_template_search_documents_ad
AFTER DELETE ON message_template_search_documents BEGIN
    INSERT INTO message_templates_fts(message_templates_fts, rowid, content_text)
    VALUES ('delete', old.search_rowid, old.content_text);
END;
CREATE TRIGGER IF NOT EXISTS message_template_search_documents_au
AFTER UPDATE ON message_template_search_documents BEGIN
    INSERT INTO message_templates_fts(message_templates_fts, rowid, content_text)
    VALUES ('delete', old.search_rowid, old.content_text);
    INSERT INTO message_templates_fts(rowid, content_text)
    VALUES (new.search_rowid, new.content_text);
END;

CREATE TABLE IF NOT EXISTS search_projection_schema (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    version INTEGER NOT NULL
);
"#;

pub(crate) fn ensure_schema(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), atm_storage::AtmError> {
    connection.execute_batch(SEARCH_DDL).map_err(|error| {
        sqlite_error(
            target,
            "failed to initialize SQLite search projection schema",
            error,
        )
    })?;
    configure_message_fts_automerge(connection, target)?;
    let current: Option<i64> = connection
        .query_row(
            "SELECT version FROM search_projection_schema WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to inspect SQLite search projection version",
                error,
            )
        })?;
    if current != Some(1) {
        rebuild(connection, target)?;
        connection
            .execute(
                "INSERT INTO search_projection_schema(singleton, version) VALUES (1, 1)
                 ON CONFLICT(singleton) DO UPDATE SET version = excluded.version",
                [],
            )
            .map_err(|error| {
                sqlite_error(
                    target,
                    "failed to record SQLite search projection version",
                    error,
                )
            })?;
    }
    Ok(())
}

/// Configure a persistent, bounded FTS5 merge policy for message projections.
///
/// This is intentionally an FTS configuration command rather than a schema
/// revision: it changes neither the source rows nor their immediate
/// transactional projection/search visibility, and is applied idempotently to
/// both new and existing databases during backend initialization.
fn configure_message_fts_automerge(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), atm_storage::AtmError> {
    connection
        .execute(
            "INSERT INTO mail_messages_fts(mail_messages_fts, rank) VALUES ('automerge', ?1)",
            rusqlite::params![MESSAGE_FTS_AUTOMERGE],
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to configure SQLite message-search FTS merge policy",
                error,
            )
        })?;
    Ok(())
}

pub(crate) fn rebuild(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), atm_storage::AtmError> {
    connection
        .execute("DELETE FROM mail_message_search_documents", [])
        .map_err(|error| {
            sqlite_error(target, "failed to clear message search projection", error)
        })?;
    connection
        .execute("DELETE FROM message_template_search_documents", [])
        .map_err(|error| {
            sqlite_error(target, "failed to clear template search projection", error)
        })?;
    let mut templates = connection
        .prepare("SELECT template_sha, content_text FROM message_templates")
        .map_err(|error| {
            sqlite_error(target, "failed to prepare template search backfill", error)
        })?;
    let rows = templates
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| sqlite_error(target, "failed to query template search backfill", error))?;
    for row in rows {
        let (sha, text) = row.map_err(|error| {
            sqlite_error(target, "failed to decode template search backfill", error)
        })?;
        sync_template_projection(connection, target, &sha, &text)?;
    }
    let mut messages = connection
        .prepare(
            "SELECT team, agent, message_key, message_id, message_at, message_text,
                summary, tags_json, vars_json, from_agent
         FROM mail_messages",
        )
        .map_err(|error| {
            sqlite_error(target, "failed to prepare message search backfill", error)
        })?;
    let rows = messages
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
            ))
        })
        .map_err(|error| sqlite_error(target, "failed to query message search backfill", error))?;
    for row in rows {
        let row = row.map_err(|error| {
            sqlite_error(target, "failed to decode message search backfill", error)
        })?;
        sync_message_projection_values(connection, target, row)?;
    }
    Ok(())
}

pub(crate) fn sync_template_projection(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
    template_sha: &str,
    content_text: &str,
) -> Result<(), atm_storage::AtmError> {
    connection.execute(
        "INSERT INTO message_template_search_documents(template_sha, content_text) VALUES (?1, ?2)
         ON CONFLICT(template_sha) DO UPDATE SET content_text = excluded.content_text",
        rusqlite::params![template_sha, content_text],
    ).map_err(|error| sqlite_error(target, "failed to synchronize template search projection", error))?;
    Ok(())
}

/// Synchronize the searchable projection for an ordinary message that was
/// just inserted through the writer lane.  Its canonical fields are already
/// present in the admitted request, so avoid a redundant row read on every
/// successful durable write.
///
/// This deliberately groups the writer's canonical values at the call
/// boundary.  Keeping the data together makes it harder for a future caller
/// to pass a field from a different message while preserving the no-reread
/// fast path.
pub(crate) struct InsertedMessageProjection<'a> {
    pub(crate) team: &'a str,
    pub(crate) agent: &'a str,
    pub(crate) message_key: &'a str,
    pub(crate) message_id: Option<&'a str>,
    pub(crate) message_at: &'a str,
    pub(crate) message_text: &'a str,
    pub(crate) summary: Option<&'a str>,
    pub(crate) tags_json: &'a str,
    pub(crate) from_agent: &'a str,
}

pub(crate) fn sync_inserted_message_projection(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
    projection: InsertedMessageProjection<'_>,
) -> Result<(), atm_storage::AtmError> {
    sync_message_projection_fields(
        connection,
        target,
        projection.team,
        projection.agent,
        projection.message_key,
        projection.message_id,
        projection.message_at,
        projection.message_text,
        projection.summary,
        Some(projection.tags_json),
        None,
        Some(projection.from_agent),
    )
}

pub(crate) fn sync_message_projection_by_key(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
    message_key: &str,
) -> Result<(), atm_storage::AtmError> {
    let row = connection
        .query_row(
            "SELECT team, agent, message_key, message_id, message_at, message_text,
                summary, tags_json, vars_json, from_agent
         FROM mail_messages WHERE message_key = ?1",
            rusqlite::params![message_key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .optional()
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to load message for search synchronization",
                error,
            )
        })?;
    if let Some(row) = row {
        sync_message_projection_values(connection, target, row)?;
    }
    Ok(())
}

type MessageProjectionRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn sync_message_projection_values(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
    (
        team,
        agent,
        message_key,
        message_id,
        message_at,
        message_text,
        summary,
        tags_json,
        vars_json,
        from_agent,
    ): MessageProjectionRow,
) -> Result<(), atm_storage::AtmError> {
    sync_message_projection_fields(
        connection,
        target,
        &team,
        &agent,
        &message_key,
        message_id.as_deref(),
        message_at.as_deref().unwrap_or_default(),
        message_text.as_deref().unwrap_or_default(),
        summary.as_deref(),
        tags_json.as_deref(),
        vars_json.as_deref(),
        from_agent.as_deref(),
    )
}

#[allow(clippy::too_many_arguments)]
fn sync_message_projection_fields(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
    team: &str,
    agent: &str,
    message_key: &str,
    message_id: Option<&str>,
    message_at: &str,
    message_text: &str,
    summary: Option<&str>,
    tags_json: Option<&str>,
    vars_json: Option<&str>,
    from_agent: Option<&str>,
) -> Result<(), atm_storage::AtmError> {
    let tags = tags_json
        .map(flatten_json_text)
        .transpose()?
        .unwrap_or_default();
    let var_values = vars_json
        .map(flatten_json_text)
        .transpose()?
        .unwrap_or_default();
    let mut statement = connection.prepare_cached(
        "INSERT INTO mail_message_search_documents(
             team, agent, message_key, message_id, message_at, body_text, summary, tags, var_values, from_agent
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(team, agent, message_key) DO UPDATE SET
             message_id = excluded.message_id, message_at = excluded.message_at,
             body_text = excluded.body_text, summary = excluded.summary, tags = excluded.tags,
             var_values = excluded.var_values, from_agent = excluded.from_agent",
    ).map_err(|error| sqlite_error(target, "failed to cache message search projection", error))?;
    statement
        .execute(rusqlite::params![
            team,
            agent,
            message_key,
            message_id,
            message_at,
            message_text,
            summary.unwrap_or_default(),
            tags,
            var_values,
            from_agent.unwrap_or_default(),
        ])
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to synchronize message search projection",
                error,
            )
        })?;
    Ok(())
}

pub(crate) fn delete_message_projection(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
    message_key: &str,
) -> Result<(), atm_storage::AtmError> {
    connection
        .execute(
            "DELETE FROM mail_message_search_documents WHERE message_key = ?1",
            rusqlite::params![message_key],
        )
        .map_err(|error| {
            sqlite_error(target, "failed to delete message search projection", error)
        })?;
    Ok(())
}

#[cfg(test)]
type ProjectionSnapshotRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);

#[cfg(test)]
pub(crate) fn projection_snapshot(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<Vec<ProjectionSnapshotRow>, atm_storage::AtmError> {
    let mut statement = connection
        .prepare(
            "SELECT team, agent, message_key, COALESCE(message_id, ''), body_text, summary, tags, var_values
             FROM mail_message_search_documents
             ORDER BY team, agent, message_key",
        )
        .map_err(|error| sqlite_error(target, "failed to prepare search projection snapshot", error))?;
    statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })
        .map_err(|error| sqlite_error(target, "failed to query search projection snapshot", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| sqlite_error(target, "failed to decode search projection snapshot", error))
}

#[cfg(test)]
pub(crate) fn template_projection_snapshot(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<Vec<(String, String)>, atm_storage::AtmError> {
    let mut statement = connection
        .prepare(
            "SELECT template_sha, content_text
             FROM message_template_search_documents
             ORDER BY template_sha ASC",
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to prepare template projection snapshot",
                error,
            )
        })?;
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to query template projection snapshot",
                error,
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to decode template projection snapshot",
                error,
            )
        })
}

/// Flatten JSON deterministically: object keys sort, arrays retain order, and
/// scalar values preserve their JSON scalar representation without permitting
/// caller-provided query syntax to reach the FTS compiler.
pub(crate) fn flatten_json_text(raw: &str) -> Result<String, atm_storage::AtmError> {
    let value: Value = serde_json::from_str(raw).map_err(|_| {
        atm_storage::AtmError::validation("stored search JSON projection is invalid")
    })?;
    let mut values = Vec::new();
    flatten_value(&value, &mut values);
    Ok(values.join(" "))
}

fn flatten_value(value: &Value, values: &mut Vec<String>) {
    match value {
        Value::Null => values.push("null".to_owned()),
        Value::Bool(value) => values.push(value.to_string()),
        Value::Number(value) => values.push(value.to_string()),
        Value::String(value) => values.push(value.clone()),
        Value::Array(items) => {
            for item in items {
                flatten_value(item, values);
            }
        }
        Value::Object(items) => {
            let mut keys: Vec<_> = items.keys().collect();
            keys.sort_unstable();
            for key in keys {
                flatten_value(&items[key], values);
            }
        }
    }
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::flatten_json_text;

    #[test]
    fn json_flattening_is_key_sorted_and_array_stable() {
        assert_eq!(
            flatten_json_text(r#"{"z":["last", {"b":2,"a":1}], "a":true}"#).expect("valid JSON"),
            "true last 1 2"
        );
    }
}
