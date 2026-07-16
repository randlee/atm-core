use atm_storage::AtmError;
use atm_storage::contract::{AllowHostCommand, AllowedHostName, AllowedHostRow, AllowedHostStore};
use rusqlite::{OptionalExtension, params};

use crate::SqliteAllowedHostStore;
use crate::shared_db::SqliteConnection;

impl SqliteAllowedHostStore {
    pub(crate) fn new(db: std::sync::Arc<crate::shared_db::SharedDb>) -> Self {
        Self { db }
    }
}

impl atm_storage::contract::sealed::Sealed for SqliteAllowedHostStore {}

impl AllowedHostStore for SqliteAllowedHostStore {
    fn allow_host(&self, command: AllowHostCommand) -> Result<AllowedHostRow, AtmError> {
        let now = atm_storage::IsoTimestamp::now();
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO daemon_allowed_hosts(
                        host_name,
                        enabled,
                        added_by,
                        added_at,
                        updated_at,
                        disabled_at,
                        note
                    ) VALUES (?1, 1, ?2, ?3, ?4, NULL, ?5)
                    ON CONFLICT(host_name) DO UPDATE SET
                        enabled = 1,
                        added_by = excluded.added_by,
                        updated_at = excluded.updated_at,
                        disabled_at = NULL,
                        note = COALESCE(excluded.note, daemon_allowed_hosts.note);",
                    params![
                        command.host_name.as_str(),
                        command.added_by,
                        now.to_string(),
                        now.to_string(),
                        command.note,
                    ],
                )
                .map_err(|error| self.db.error("failed to upsert daemon allowed host row", error))?;
            load_row(transaction, &self.db, &command.host_name)?.ok_or_else(|| {
                AtmError::mailbox_read(
                    "daemon allowed host row was written but could not be reloaded",
                )
                .with_recovery(
                    "Inspect the daemon_allowed_hosts table for a partially written row before retrying the allow command.",
                )
            })
        })
    }

    fn deny_host(&self, host: &AllowedHostName) -> Result<AllowedHostRow, AtmError> {
        let now = atm_storage::IsoTimestamp::now();
        self.db.with_transaction(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE daemon_allowed_hosts
                     SET enabled = 0,
                         updated_at = ?2,
                         disabled_at = ?2
                     WHERE host_name = ?1;",
                    params![host.as_str(), now.to_string()],
                )
                .map_err(|error| self.db.error("failed to disable daemon allowed host row", error))?;
            if changed == 0 {
                return Err(missing_host_error(host, "deny"));
            }
            load_row(transaction, &self.db, host)?.ok_or_else(|| {
                AtmError::mailbox_read(
                    "daemon allowed host row was disabled but could not be reloaded",
                )
                .with_recovery(
                    "Inspect the daemon_allowed_hosts table for the targeted row before retrying the deny command.",
                )
            })
        })
    }

    fn remove_host(&self, host: &AllowedHostName) -> Result<(), AtmError> {
        self.db.with_transaction(|transaction| {
            let changed = transaction
                .execute(
                    "DELETE FROM daemon_allowed_hosts WHERE host_name = ?1;",
                    params![host.as_str()],
                )
                .map_err(|error| {
                    self.db
                        .error("failed to remove daemon allowed host row", error)
                })?;
            if changed == 0 {
                return Err(missing_host_error(host, "remove"));
            }
            Ok(())
        })
    }

    fn list_hosts(&self) -> Result<Vec<AllowedHostRow>, AtmError> {
        self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT host_name,
                            enabled,
                            added_by,
                            added_at,
                            updated_at,
                            disabled_at,
                            note
                     FROM daemon_allowed_hosts
                     ORDER BY enabled DESC, host_name ASC;",
                )
                .map_err(|error| {
                    self.db
                        .error("failed to prepare daemon allowed host list", error)
                })?;
            let rows = statement
                .query_map([], |row| {
                    Ok(StoredAllowedHostRow {
                        host_name: row.get(0)?,
                        enabled: row.get(1)?,
                        added_by: row.get(2)?,
                        added_at: row.get(3)?,
                        updated_at: row.get(4)?,
                        disabled_at: row.get(5)?,
                        note: row.get(6)?,
                    })
                })
                .map_err(|error| {
                    self.db
                        .error("failed to execute daemon allowed host list", error)
                })?;
            let mut result = Vec::new();
            for row in rows {
                result.push(decode_row(row.map_err(|error| {
                    self.db
                        .error("failed to decode daemon allowed host row", error)
                })?)?);
            }
            Ok(result)
        })
    }

    fn load_host(&self, host: &AllowedHostName) -> Result<Option<AllowedHostRow>, AtmError> {
        self.db
            .with_connection(|connection| load_row(connection, &self.db, host))
    }
}

struct StoredAllowedHostRow {
    host_name: String,
    enabled: i64,
    added_by: String,
    added_at: String,
    updated_at: String,
    disabled_at: Option<String>,
    note: Option<String>,
}

fn load_row(
    connection: &SqliteConnection,
    db: &crate::shared_db::SharedDb,
    host: &AllowedHostName,
) -> Result<Option<AllowedHostRow>, AtmError> {
    connection
        .query_row(
            "SELECT host_name,
                    enabled,
                    added_by,
                    added_at,
                    updated_at,
                    disabled_at,
                    note
             FROM daemon_allowed_hosts
             WHERE host_name = ?1;",
            params![host.as_str()],
            |row| {
                Ok(StoredAllowedHostRow {
                    host_name: row.get(0)?,
                    enabled: row.get(1)?,
                    added_by: row.get(2)?,
                    added_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    disabled_at: row.get(5)?,
                    note: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|error| db.error("failed to load daemon allowed host row", error))?
        .map(decode_row)
        .transpose()
}

fn decode_row(row: StoredAllowedHostRow) -> Result<AllowedHostRow, AtmError> {
    Ok(AllowedHostRow {
        host_name: row.host_name.parse().map_err(|error| {
            AtmError::validation(format!(
                "failed to parse daemon_allowed_hosts.host_name `{}`: {error}",
                row.host_name
            ))
            .with_recovery(
                "Repair the malformed daemon_allowed_hosts.host_name row before retrying the query.",
            )
        })?,
        enabled: decode_enabled(row.enabled)?,
        added_by: row.added_by,
        added_at: parse_timestamp(row.added_at, "added_at")?,
        updated_at: parse_timestamp(row.updated_at, "updated_at")?,
        disabled_at: row
            .disabled_at
            .map(|value| parse_timestamp(value, "disabled_at"))
            .transpose()?,
        note: row.note,
    })
}

fn decode_enabled(raw: i64) -> Result<bool, AtmError> {
    match raw {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(AtmError::validation(format!(
            "daemon_allowed_hosts.enabled must be 0 or 1, found {other}"
        ))
        .with_recovery(
            "Repair the malformed daemon_allowed_hosts.enabled row before retrying the query.",
        )),
    }
}

fn parse_timestamp(raw: String, field: &str) -> Result<atm_storage::IsoTimestamp, AtmError> {
    raw.parse::<atm_storage::IsoTimestamp>().map_err(|error| {
        AtmError::validation(format!(
            "failed to parse daemon_allowed_hosts.{field} timestamp: {error}"
        ))
        .with_recovery(format!(
            "Repair the malformed daemon_allowed_hosts.{field} row before retrying the query."
        ))
    })
}

fn missing_host_error(host: &AllowedHostName, action: &str) -> AtmError {
    AtmError::validation(format!(
        "no daemon allowed host row matched `{}` for {action}",
        host.as_str()
    ))
    .with_recovery(
        "Use `atm daemon hosts list` to inspect the configured rows before retrying the host mutation.",
    )
}
