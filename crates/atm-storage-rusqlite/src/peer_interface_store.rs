use std::net::IpAddr;

use atm_storage::AtmError;
use atm_storage::contract::{
    AddPeerInterfaceCommand, PeerInterfaceBindingUpdate, PeerInterfaceConfigStore,
    PeerInterfaceKey, PeerInterfaceKind, PeerInterfaceRow, UpdatePeerInterfaceCommand,
};
use rusqlite::{OptionalExtension, params};

use crate::SqlitePeerInterfaceConfigStore;

impl SqlitePeerInterfaceConfigStore {
    pub(crate) fn new(db: std::sync::Arc<crate::shared_db::SharedDb>) -> Self {
        Self { db }
    }
}

impl atm_storage::contract::sealed::Sealed for SqlitePeerInterfaceConfigStore {}

impl PeerInterfaceConfigStore for SqlitePeerInterfaceConfigStore {
    fn add_interface(
        &self,
        command: AddPeerInterfaceCommand,
    ) -> Result<PeerInterfaceRow, AtmError> {
        let now = atm_storage::IsoTimestamp::now();
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO daemon_peer_interfaces(
                        interface_name,
                        bind_addr,
                        advertise_addr,
                        port,
                        interface_kind,
                        enabled,
                        configured_by,
                        configured_at,
                        updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8);",
                    params![
                        command.interface_name,
                        command.bind_addr.to_string(),
                        command.advertise_addr.to_string(),
                        i64::from(command.port),
                        command.interface_kind.as_str(),
                        command.configured_by,
                        now.to_string(),
                        now.to_string(),
                    ],
                )
                .map_err(|error| self.db.error("failed to insert daemon peer interface row", error))?;
            load_row_by_identity(
                transaction,
                &self.db,
                &command.interface_name,
                command.bind_addr,
                command.port,
            )?
            .ok_or_else(|| {
                AtmError::mailbox_read(
                    "daemon peer interface row was inserted but could not be reloaded",
                )
                .with_recovery(
                    "Inspect the daemon_peer_interfaces table for a partially written row before retrying the add command.",
                )
            })
        })
    }

    fn update_interface(
        &self,
        command: UpdatePeerInterfaceCommand,
    ) -> Result<PeerInterfaceRow, AtmError> {
        let now = atm_storage::IsoTimestamp::now();
        self.db.with_transaction(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE daemon_peer_interfaces
                     SET interface_name = ?1,
                         bind_addr = ?2,
                         advertise_addr = ?3,
                         port = ?4,
                         interface_kind = ?5,
                         configured_by = ?6,
                         updated_at = ?7,
                         enabled = COALESCE(?8, enabled)
                     WHERE interface_name = ?9
                       AND bind_addr = ?10
                       AND port = ?11;",
                    params![
                        command.key.interface_name,
                        command.new_bind_addr.to_string(),
                        command.advertise_addr.to_string(),
                        i64::from(command.port),
                        command.interface_kind.as_str(),
                        command.configured_by,
                        now.to_string(),
                        command.enabled.map(i64::from),
                        command.key.interface_name,
                        command.key.bind_addr.to_string(),
                        i64::from(command.key.port),
                    ],
                )
                .map_err(|error| self.db.error("failed to update daemon peer interface row", error))?;
            if changed == 0 {
                return Err(interface_row_missing_error(&command.key, "update"));
            }
            load_row_by_identity(
                transaction,
                &self.db,
                &command.key.interface_name,
                command.new_bind_addr,
                command.port,
            )?
            .ok_or_else(|| {
                AtmError::mailbox_read(
                    "daemon peer interface row was updated but could not be reloaded",
                )
                .with_recovery(
                    "Inspect the daemon_peer_interfaces table for a partially updated row before retrying the update command.",
                )
            })
        })
    }

    fn set_interface_enabled(
        &self,
        key: &PeerInterfaceKey,
        enabled: bool,
    ) -> Result<PeerInterfaceRow, AtmError> {
        let now = atm_storage::IsoTimestamp::now();
        self.db.with_transaction(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE daemon_peer_interfaces
                     SET enabled = ?1,
                         updated_at = ?2
                     WHERE interface_name = ?3
                       AND bind_addr = ?4
                       AND port = ?5;",
                    params![
                        if enabled { 1_i64 } else { 0_i64 },
                        now.to_string(),
                        key.interface_name,
                        key.bind_addr.to_string(),
                        i64::from(key.port),
                    ],
                )
                .map_err(|error| self.db.error("failed to toggle daemon peer interface row", error))?;
            if changed == 0 {
                return Err(interface_row_missing_error(key, "toggle"));
            }
            load_row_by_identity(transaction, &self.db, &key.interface_name, key.bind_addr, key.port)?
                .ok_or_else(|| {
                    AtmError::mailbox_read(
                        "daemon peer interface row was updated but could not be reloaded",
                    )
                    .with_recovery(
                        "Inspect the daemon_peer_interfaces table for the targeted row before retrying the enable or disable command.",
                    )
                })
        })
    }

    fn remove_interface(&self, key: &PeerInterfaceKey) -> Result<bool, AtmError> {
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "DELETE FROM daemon_peer_interfaces
                     WHERE interface_name = ?1
                       AND bind_addr = ?2
                       AND port = ?3;",
                    params![
                        key.interface_name,
                        key.bind_addr.to_string(),
                        i64::from(key.port),
                    ],
                )
                .map(|rows| rows > 0)
                .map_err(|error| {
                    self.db
                        .error("failed to delete daemon peer interface row", error)
                })
        })
    }

    fn list_interfaces(&self) -> Result<Vec<PeerInterfaceRow>, AtmError> {
        self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT interface_id,
                            interface_name,
                            bind_addr,
                            advertise_addr,
                            port,
                            interface_kind,
                            enabled,
                            configured_by,
                            configured_at,
                            updated_at,
                            last_observed_at,
                            refresh_deadline_at,
                            stale_at,
                            last_bound_at,
                            last_bind_error
                     FROM daemon_peer_interfaces
                     ORDER BY enabled DESC, interface_kind ASC, interface_name ASC, bind_addr ASC, port ASC;",
                )
                .map_err(|error| self.db.error("failed to prepare daemon peer interface list", error))?;
            let rows = statement
                .query_map([], |row| {
                    Ok(StoredPeerInterfaceRow {
                        interface_id: row.get(0)?,
                        interface_name: row.get(1)?,
                        bind_addr: row.get(2)?,
                        advertise_addr: row.get(3)?,
                        port: row.get(4)?,
                        interface_kind: row.get(5)?,
                        enabled: row.get(6)?,
                        configured_by: row.get(7)?,
                        configured_at: row.get(8)?,
                        updated_at: row.get(9)?,
                        last_observed_at: row.get(10)?,
                        refresh_deadline_at: row.get(11)?,
                        stale_at: row.get(12)?,
                        last_bound_at: row.get(13)?,
                        last_bind_error: row.get(14)?,
                    })
                })
                .map_err(|error| self.db.error("failed to execute daemon peer interface list", error))?;
            let mut result = Vec::new();
            for row in rows {
                result.push(decode_row(
                    row.map_err(|error| self.db.error("failed to decode daemon peer interface row", error))?,
                )?);
            }
            Ok(result)
        })
    }

    fn record_binding_update(
        &self,
        update: &PeerInterfaceBindingUpdate,
    ) -> Result<PeerInterfaceRow, AtmError> {
        self.db.with_transaction(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE daemon_peer_interfaces
                     SET last_observed_at = COALESCE(?1, last_observed_at),
                         refresh_deadline_at = ?2,
                         stale_at = ?3,
                         last_bound_at = ?4,
                         last_bind_error = ?5,
                         updated_at = COALESCE(?1, updated_at)
                     WHERE interface_name = ?6
                       AND bind_addr = ?7
                       AND port = ?8;",
                    params![
                        update.observed_at.as_ref().map(ToString::to_string),
                        update.refresh_deadline_at.as_ref().map(ToString::to_string),
                        update.stale_at.as_ref().map(ToString::to_string),
                        update.last_bound_at.as_ref().map(ToString::to_string),
                        update.last_bind_error,
                        update.key.interface_name,
                        update.key.bind_addr.to_string(),
                        i64::from(update.key.port),
                    ],
                )
                .map_err(|error| {
                    self.db
                        .error("failed to record daemon peer interface bind state", error)
                })?;
            if changed == 0 {
                return Err(interface_row_missing_error(
                    &update.key,
                    "record bind state",
                ));
            }
            load_row_by_identity(
                transaction,
                &self.db,
                &update.key.interface_name,
                update.key.bind_addr,
                update.key.port,
            )?
            .ok_or_else(|| {
                AtmError::mailbox_read(
                    "daemon peer interface bind-state row was updated but could not be reloaded",
                )
                .with_recovery(
                    "Inspect the daemon_peer_interfaces row before retrying the bind-state update.",
                )
            })
        })
    }
}

struct StoredPeerInterfaceRow {
    interface_id: i64,
    interface_name: String,
    bind_addr: String,
    advertise_addr: String,
    port: i64,
    interface_kind: String,
    enabled: i64,
    configured_by: String,
    configured_at: String,
    updated_at: String,
    last_observed_at: Option<String>,
    refresh_deadline_at: Option<String>,
    stale_at: Option<String>,
    last_bound_at: Option<String>,
    last_bind_error: Option<String>,
}

fn decode_row(row: StoredPeerInterfaceRow) -> Result<PeerInterfaceRow, AtmError> {
    Ok(PeerInterfaceRow {
        interface_id: row.interface_id,
        interface_name: row.interface_name,
        bind_addr: parse_ip(&row.bind_addr, "bind_addr")?,
        advertise_addr: parse_ip(&row.advertise_addr, "advertise_addr")?,
        port: decode_port(row.port)?,
        interface_kind: row.interface_kind.parse::<PeerInterfaceKind>().map_err(|error| {
            AtmError::validation(format!(
                "failed to parse daemon_peer_interfaces.interface_kind `{}`: {error}",
                row.interface_kind
            ))
            .with_recovery(
                "Repair the malformed daemon_peer_interfaces.interface_kind row before retrying the query.",
            )
        })?,
        enabled: match row.enabled {
            0 => false,
            1 => true,
            other => {
                return Err(AtmError::validation(format!(
                    "daemon_peer_interfaces.enabled must be 0 or 1, found {other}"
                ))
                .with_recovery(
                    "Repair the malformed daemon_peer_interfaces.enabled row before retrying the query.",
                ));
            }
        },
        configured_by: row.configured_by,
        configured_at: parse_timestamp(row.configured_at, "configured_at")?,
        updated_at: parse_timestamp(row.updated_at, "updated_at")?,
        last_observed_at: parse_optional_timestamp(row.last_observed_at, "last_observed_at")?,
        refresh_deadline_at: parse_optional_timestamp(
            row.refresh_deadline_at,
            "refresh_deadline_at",
        )?,
        stale_at: parse_optional_timestamp(row.stale_at, "stale_at")?,
        last_bound_at: parse_optional_timestamp(row.last_bound_at, "last_bound_at")?,
        last_bind_error: row.last_bind_error,
    })
}

fn load_row_by_identity(
    connection: &rusqlite::Transaction<'_>,
    db: &crate::shared_db::SharedDb,
    interface_name: &str,
    bind_addr: IpAddr,
    port: u16,
) -> Result<Option<PeerInterfaceRow>, AtmError> {
    connection
        .query_row(
            "SELECT interface_id,
                    interface_name,
                    bind_addr,
                    advertise_addr,
                    port,
                    interface_kind,
                    enabled,
                    configured_by,
                    configured_at,
                    updated_at,
                    last_observed_at,
                    refresh_deadline_at,
                    stale_at,
                    last_bound_at,
                    last_bind_error
             FROM daemon_peer_interfaces
             WHERE interface_name = ?1
               AND bind_addr = ?2
               AND port = ?3;",
            params![interface_name, bind_addr.to_string(), i64::from(port)],
            |row| {
                Ok(StoredPeerInterfaceRow {
                    interface_id: row.get(0)?,
                    interface_name: row.get(1)?,
                    bind_addr: row.get(2)?,
                    advertise_addr: row.get(3)?,
                    port: row.get(4)?,
                    interface_kind: row.get(5)?,
                    enabled: row.get(6)?,
                    configured_by: row.get(7)?,
                    configured_at: row.get(8)?,
                    updated_at: row.get(9)?,
                    last_observed_at: row.get(10)?,
                    refresh_deadline_at: row.get(11)?,
                    stale_at: row.get(12)?,
                    last_bound_at: row.get(13)?,
                    last_bind_error: row.get(14)?,
                })
            },
        )
        .optional()
        .map_err(|error| db.error("failed to reload daemon peer interface row", error))?
        .map(decode_row)
        .transpose()
}

fn parse_ip(raw: &str, field: &str) -> Result<IpAddr, AtmError> {
    raw.parse::<IpAddr>().map_err(|error| {
        AtmError::validation(format!(
            "failed to parse daemon_peer_interfaces.{field} `{raw}`: {error}"
        ))
        .with_recovery(format!(
            "Repair the malformed daemon_peer_interfaces.{field} row before retrying the query."
        ))
    })
}

fn decode_port(raw: i64) -> Result<u16, AtmError> {
    u16::try_from(raw).map_err(|error| {
        AtmError::validation(format!(
            "failed to parse daemon_peer_interfaces.port `{raw}`: {error}"
        ))
        .with_recovery(
            "Repair the malformed daemon_peer_interfaces.port row before retrying the query.",
        )
        .with_source(error)
    })
}

fn parse_timestamp(raw: String, field: &str) -> Result<atm_storage::IsoTimestamp, AtmError> {
    raw.parse::<atm_storage::IsoTimestamp>().map_err(|error| {
        AtmError::validation(format!(
            "failed to parse daemon_peer_interfaces.{field} timestamp: {error}"
        ))
        .with_recovery(format!(
            "Repair the malformed daemon_peer_interfaces.{field} row before retrying the query."
        ))
        .with_source(error)
    })
}

fn parse_optional_timestamp(
    raw: Option<String>,
    field: &str,
) -> Result<Option<atm_storage::IsoTimestamp>, AtmError> {
    raw.map(|value| parse_timestamp(value, field)).transpose()
}

fn interface_row_missing_error(key: &PeerInterfaceKey, action: &str) -> AtmError {
    AtmError::validation(format!(
        "cannot {action} daemon peer interface `{}` at {}:{} because no matching row exists",
        key.interface_name, key.bind_addr, key.port
    ))
    .with_recovery(
        "Use `atm daemon interfaces list` to inspect authoritative rows before retrying the interface mutation.",
    )
}
