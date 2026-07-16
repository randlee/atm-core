use crate::SqlitePeerSecurityStore;
use atm_storage::contract::{
    AllowedHostName, LocalPeerIdentityRow, PeerSecurityMode, PeerSecuritySettingsRow,
    PeerSecurityStore, SetPeerSecurityModeCommand, TrustedPeerRow, UpsertTrustedPeerCommand,
};
use atm_storage::{AtmError, IsoTimestamp};
use rcgen::generate_simple_self_signed;
use rusqlite::{OptionalExtension, params};

impl SqlitePeerSecurityStore {
    pub(crate) fn new(db: std::sync::Arc<crate::shared_db::SharedDb>) -> Self {
        Self { db }
    }
}

impl atm_storage::contract::sealed::Sealed for SqlitePeerSecurityStore {}

impl PeerSecurityStore for SqlitePeerSecurityStore {
    fn load_security_settings(&self) -> Result<PeerSecuritySettingsRow, AtmError> {
        self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT mode, updated_by, updated_at
                     FROM daemon_peer_security_settings
                     WHERE singleton_key = 1;",
                    [],
                    |row| {
                        Ok(StoredPeerSecuritySettingsRow {
                            mode: row.get(0)?,
                            updated_by: row.get(1)?,
                            updated_at: row.get(2)?,
                        })
                    },
                )
                .optional()
                .map_err(|error| {
                    self.db
                        .error("failed to load daemon peer security settings", error)
                })?
                .map(decode_security_settings_row)
                .transpose()
                .map(|row| {
                    row.unwrap_or(PeerSecuritySettingsRow {
                        mode: PeerSecurityMode::InsecureAllowed,
                        updated_by: None,
                        updated_at: None,
                    })
                })
        })
    }

    fn set_security_mode(
        &self,
        command: SetPeerSecurityModeCommand,
    ) -> Result<PeerSecuritySettingsRow, AtmError> {
        let now = IsoTimestamp::now();
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO daemon_peer_security_settings(
                        singleton_key,
                        mode,
                        updated_by,
                        updated_at
                    ) VALUES (1, ?1, ?2, ?3)
                    ON CONFLICT(singleton_key) DO UPDATE SET
                        mode = excluded.mode,
                        updated_by = excluded.updated_by,
                        updated_at = excluded.updated_at;",
                    params![command.mode.as_str(), command.updated_by, now.to_string()],
                )
                .map_err(|error| {
                    self.db
                        .error("failed to upsert daemon peer security settings", error)
                })?;
            Ok(PeerSecuritySettingsRow {
                mode: command.mode,
                updated_by: Some(command.updated_by),
                updated_at: Some(now),
            })
        })
    }

    fn load_local_identity(&self) -> Result<Option<LocalPeerIdentityRow>, AtmError> {
        self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT certificate_der,
                            private_key_der,
                            fingerprint_sha256,
                            created_at,
                            updated_at
                     FROM daemon_local_peer_identity
                     WHERE singleton_key = 1;",
                    [],
                    |row| {
                        Ok(StoredLocalPeerIdentityRow {
                            certificate_der: row.get(0)?,
                            private_key_der: row.get(1)?,
                            fingerprint_sha256: row.get(2)?,
                            created_at: row.get(3)?,
                            updated_at: row.get(4)?,
                        })
                    },
                )
                .optional()
                .map_err(|error| {
                    self.db
                        .error("failed to load daemon local peer identity", error)
                })?
                .map(decode_local_peer_identity_row)
                .transpose()
        })
    }

    fn load_or_create_local_identity(&self) -> Result<LocalPeerIdentityRow, AtmError> {
        if let Some(existing) = self.load_local_identity()? {
            return Ok(existing);
        }
        let now = IsoTimestamp::now();
        let certified_key =
            generate_simple_self_signed(vec!["localhost".to_string(), "atm-peer.local".to_string()])
                .map_err(|error| {
                    AtmError::daemon_unavailable(format!(
                        "failed to generate daemon local peer identity: {error}"
                    ))
                    .with_recovery(
                        "Retry the daemon security command after confirming the host cryptography provider can generate a self-signed peer certificate.",
                    )
                })?;
        let certificate_der = certified_key.cert.der().to_vec();
        let private_key_der = certified_key.signing_key.serialize_der();
        let fingerprint_sha256 = atm_storage::sha256_hex(&certificate_der);
        let row = LocalPeerIdentityRow::new(
            certificate_der,
            private_key_der,
            fingerprint_sha256,
            now.clone(),
            now,
        )?;
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO daemon_local_peer_identity(
                        singleton_key,
                        certificate_der,
                        private_key_der,
                        fingerprint_sha256,
                        created_at,
                        updated_at
                    ) VALUES (1, ?1, ?2, ?3, ?4, ?5)
                    ON CONFLICT(singleton_key) DO NOTHING;",
                    params![
                        row.certificate_der(),
                        row.private_key_der(),
                        row.fingerprint_sha256(),
                        row.created_at().to_string(),
                        row.updated_at().to_string(),
                    ],
                )
                .map_err(|error| {
                    self.db
                        .error("failed to persist daemon local peer identity", error)
                })?;
            load_local_identity_row(transaction, &self.db)?.ok_or_else(|| {
                AtmError::mailbox_read(
                    "daemon local peer identity was persisted but could not be reloaded",
                )
                .with_recovery(
                    "Inspect the daemon_local_peer_identity table for a partially written row before retrying local identity generation.",
                )
            })
        })
    }

    fn list_trusted_peers(&self) -> Result<Vec<TrustedPeerRow>, AtmError> {
        self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT host_name,
                            fingerprint_sha256,
                            display_name,
                            approved_by,
                            approved_at,
                            updated_at
                     FROM daemon_trusted_peers
                     ORDER BY host_name ASC;",
                )
                .map_err(|error| {
                    self.db
                        .error("failed to prepare daemon trusted peer list", error)
                })?;
            let rows = statement
                .query_map([], |row| {
                    Ok(StoredTrustedPeerRow {
                        host_name: row.get(0)?,
                        fingerprint_sha256: row.get(1)?,
                        display_name: row.get(2)?,
                        approved_by: row.get(3)?,
                        approved_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                })
                .map_err(|error| {
                    self.db
                        .error("failed to execute daemon trusted peer list", error)
                })?;
            let mut result = Vec::new();
            for row in rows {
                result.push(decode_trusted_peer_row(row.map_err(|error| {
                    self.db
                        .error("failed to decode daemon trusted peer row", error)
                })?)?);
            }
            Ok(result)
        })
    }

    fn load_trusted_peer(
        &self,
        host: &AllowedHostName,
    ) -> Result<Option<TrustedPeerRow>, AtmError> {
        self.db
            .with_connection(|connection| load_trusted_peer_row(connection, &self.db, host))
    }

    fn upsert_trusted_peer(
        &self,
        command: UpsertTrustedPeerCommand,
    ) -> Result<TrustedPeerRow, AtmError> {
        let now = IsoTimestamp::now();
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO daemon_trusted_peers(
                        host_name,
                        fingerprint_sha256,
                        display_name,
                        approved_by,
                        approved_at,
                        updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    ON CONFLICT(host_name) DO UPDATE SET
                        fingerprint_sha256 = excluded.fingerprint_sha256,
                        display_name = excluded.display_name,
                        approved_by = excluded.approved_by,
                        approved_at = excluded.approved_at,
                        updated_at = excluded.updated_at;",
                    params![
                        command.host_name().as_str(),
                        command.fingerprint_sha256(),
                        command.display_name(),
                        command.approved_by(),
                        now.to_string(),
                        now.to_string(),
                    ],
                )
                .map_err(|error| {
                    self.db.error("failed to upsert daemon trusted peer", error)
                })?;
            load_trusted_peer_row(transaction, &self.db, command.host_name())?.ok_or_else(|| {
                AtmError::mailbox_read(
                    "daemon trusted peer row was written but could not be reloaded",
                )
                .with_recovery(
                    "Inspect the daemon_trusted_peers table for a partially written row before retrying the trust command.",
                )
            })
        })
    }

    fn remove_trusted_peer(&self, host: &AllowedHostName) -> Result<bool, AtmError> {
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "DELETE FROM daemon_trusted_peers WHERE host_name = ?1;",
                    params![host.as_str()],
                )
                .map(|rows| rows > 0)
                .map_err(|error| {
                    self.db
                        .error("failed to remove daemon trusted peer row", error)
                })
        })
    }
}

struct StoredPeerSecuritySettingsRow {
    mode: String,
    updated_by: String,
    updated_at: String,
}

struct StoredLocalPeerIdentityRow {
    certificate_der: Vec<u8>,
    private_key_der: Vec<u8>,
    fingerprint_sha256: String,
    created_at: String,
    updated_at: String,
}

struct StoredTrustedPeerRow {
    host_name: String,
    fingerprint_sha256: String,
    display_name: Option<String>,
    approved_by: String,
    approved_at: String,
    updated_at: String,
}

fn load_local_identity_row(
    connection: &rusqlite::Connection,
    db: &crate::shared_db::SharedDb,
) -> Result<Option<LocalPeerIdentityRow>, AtmError> {
    connection
        .query_row(
            "SELECT certificate_der,
                    private_key_der,
                    fingerprint_sha256,
                    created_at,
                    updated_at
             FROM daemon_local_peer_identity
             WHERE singleton_key = 1;",
            [],
            |row| {
                Ok(StoredLocalPeerIdentityRow {
                    certificate_der: row.get(0)?,
                    private_key_der: row.get(1)?,
                    fingerprint_sha256: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|error| db.error("failed to load daemon local peer identity row", error))?
        .map(decode_local_peer_identity_row)
        .transpose()
}

fn load_trusted_peer_row(
    connection: &rusqlite::Connection,
    db: &crate::shared_db::SharedDb,
    host: &AllowedHostName,
) -> Result<Option<TrustedPeerRow>, AtmError> {
    connection
        .query_row(
            "SELECT host_name,
                    fingerprint_sha256,
                    display_name,
                    approved_by,
                    approved_at,
                    updated_at
             FROM daemon_trusted_peers
             WHERE host_name = ?1;",
            params![host.as_str()],
            |row| {
                Ok(StoredTrustedPeerRow {
                    host_name: row.get(0)?,
                    fingerprint_sha256: row.get(1)?,
                    display_name: row.get(2)?,
                    approved_by: row.get(3)?,
                    approved_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|error| db.error("failed to load daemon trusted peer row", error))?
        .map(decode_trusted_peer_row)
        .transpose()
}

fn decode_security_settings_row(
    raw: StoredPeerSecuritySettingsRow,
) -> Result<PeerSecuritySettingsRow, AtmError> {
    Ok(PeerSecuritySettingsRow {
        mode: raw.mode.parse()?,
        updated_by: Some(raw.updated_by),
        updated_at: Some(parse_timestamp(
            raw.updated_at,
            "daemon_peer_security_settings.updated_at",
        )?),
    })
}

fn decode_local_peer_identity_row(
    raw: StoredLocalPeerIdentityRow,
) -> Result<LocalPeerIdentityRow, AtmError> {
    LocalPeerIdentityRow::new(
        raw.certificate_der,
        raw.private_key_der,
        normalize_fingerprint(raw.fingerprint_sha256)?,
        parse_timestamp(raw.created_at, "daemon_local_peer_identity.created_at")?,
        parse_timestamp(raw.updated_at, "daemon_local_peer_identity.updated_at")?,
    )
}

fn decode_trusted_peer_row(raw: StoredTrustedPeerRow) -> Result<TrustedPeerRow, AtmError> {
    TrustedPeerRow::new(
        raw.host_name.parse().map_err(|error| {
            AtmError::validation(format!(
                "failed to parse daemon_trusted_peers.host_name `{}`: {error}",
                raw.host_name
            ))
            .with_recovery(
                "Repair the malformed daemon_trusted_peers.host_name row before retrying the query.",
            )
        })?,
        normalize_fingerprint(raw.fingerprint_sha256)?,
        raw.display_name,
        raw.approved_by,
        parse_timestamp(raw.approved_at, "daemon_trusted_peers.approved_at")?,
        parse_timestamp(raw.updated_at, "daemon_trusted_peers.updated_at")?,
    )
}

fn parse_timestamp(raw: String, field: &str) -> Result<IsoTimestamp, AtmError> {
    raw.parse::<IsoTimestamp>().map_err(|error| {
        AtmError::validation(format!("failed to parse {field}: {error}")).with_recovery(format!(
            "Repair the malformed {field} row before retrying the daemon security query."
        ))
    })
}

fn normalize_fingerprint(raw: String) -> Result<String, AtmError> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(AtmError::validation(format!(
            "daemon peer fingerprint `{}` must be 64 hexadecimal characters",
            raw
        ))
        .with_recovery(
            "Repair the malformed daemon security fingerprint row before retrying the query.",
        ));
    }
    Ok(normalized)
}
