use std::net::SocketAddr;
use std::sync::Arc;

use atm_storage::{HostName, HttpsInterface, LocalCertificate, PeerConfigStore, TrustedPeer};
use rusqlite::{OptionalExtension, params};

use crate::SqlitePeerConfigStore;
use crate::shared_db::SharedDb;

impl SqlitePeerConfigStore {
    pub(crate) fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }
}

impl PeerConfigStore for SqlitePeerConfigStore {
    fn list_interfaces(&self) -> Result<Vec<HttpsInterface>, atm_storage::AtmError> {
        self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT bind_addr, advertise_host, enabled
                     FROM peer_https_interfaces ORDER BY bind_addr",
                )
                .map_err(|error| {
                    self.db
                        .error("failed to prepare HTTPS interface query", error)
                })?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .map_err(|error| self.db.error("failed to query HTTPS interfaces", error))?
                .map(|row| {
                    let (bind_addr, advertise_host, enabled) = row
                        .map_err(|error| self.db.error("failed to read HTTPS interface", error))?;
                    Ok(HttpsInterface {
                        bind_addr: parse_bind_addr(&bind_addr)?,
                        advertise_host: parse_host(&advertise_host)?,
                        enabled: enabled != 0,
                    })
                })
                .collect()
        })
    }

    fn save_interface(&self, interface: &HttpsInterface) -> Result<(), atm_storage::AtmError> {
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO peer_https_interfaces(bind_addr, advertise_host, enabled)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(bind_addr) DO UPDATE SET
                         advertise_host = excluded.advertise_host,
                         enabled = excluded.enabled",
                    params![
                        interface.bind_addr.to_string(),
                        interface.advertise_host.as_str(),
                        i64::from(interface.enabled),
                    ],
                )
                .map(|_| ())
                .map_err(|error| self.db.error("failed to save HTTPS interface", error))
        })
    }

    fn remove_interface(&self, bind_addr: SocketAddr) -> Result<bool, atm_storage::AtmError> {
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "DELETE FROM peer_https_interfaces WHERE bind_addr = ?1",
                    params![bind_addr.to_string()],
                )
                .map(|count| count > 0)
                .map_err(|error| self.db.error("failed to remove HTTPS interface", error))
        })
    }

    fn local_certificate(&self) -> Result<Option<LocalCertificate>, atm_storage::AtmError> {
        let certificate = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT fingerprint, private_key_ref
                     FROM peer_local_certificate WHERE singleton = 1",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()
                .map_err(|error| self.db.error("failed to load local certificate", error))
        })?;
        certificate
            .map(|(fingerprint, private_key_ref)| {
                Ok(LocalCertificate {
                    fingerprint: require_non_blank(fingerprint, "certificate fingerprint")?,
                    private_key_ref: require_non_blank(
                        private_key_ref,
                        "certificate key reference",
                    )?,
                })
            })
            .transpose()
    }

    fn save_local_certificate(
        &self,
        certificate: &LocalCertificate,
    ) -> Result<(), atm_storage::AtmError> {
        let fingerprint =
            require_non_blank(certificate.fingerprint.clone(), "certificate fingerprint")?;
        let private_key_ref = require_non_blank(
            certificate.private_key_ref.clone(),
            "certificate key reference",
        )?;
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO peer_local_certificate(singleton, fingerprint, private_key_ref)
                     VALUES (1, ?1, ?2)
                     ON CONFLICT(singleton) DO UPDATE SET
                         fingerprint = excluded.fingerprint,
                         private_key_ref = excluded.private_key_ref",
                    params![fingerprint, private_key_ref],
                )
                .map(|_| ())
                .map_err(|error| self.db.error("failed to save local certificate", error))
        })
    }

    fn list_trusted_peers(&self) -> Result<Vec<TrustedPeer>, atm_storage::AtmError> {
        self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT host, fingerprint, enabled
                     FROM peer_trusted_peers ORDER BY host",
                )
                .map_err(|error| self.db.error("failed to prepare trusted-peer query", error))?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .map_err(|error| self.db.error("failed to query trusted peers", error))?
                .map(|row| {
                    let (host, fingerprint, enabled) =
                        row.map_err(|error| self.db.error("failed to read trusted peer", error))?;
                    Ok(TrustedPeer {
                        host: parse_host(&host)?,
                        fingerprint: require_non_blank(fingerprint, "trusted-peer fingerprint")?,
                        enabled: enabled != 0,
                    })
                })
                .collect()
        })
    }

    fn trusted_peer(&self, host: &HostName) -> Result<Option<TrustedPeer>, atm_storage::AtmError> {
        let peer = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT fingerprint, enabled FROM peer_trusted_peers WHERE host = ?1",
                    params![host.as_str()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()
                .map_err(|error| self.db.error("failed to load trusted peer", error))
        })?;
        peer.map(|(fingerprint, enabled)| {
            Ok(TrustedPeer {
                host: host.clone(),
                fingerprint: require_non_blank(fingerprint, "trusted-peer fingerprint")?,
                enabled: enabled != 0,
            })
        })
        .transpose()
    }

    fn save_trusted_peer(&self, peer: &TrustedPeer) -> Result<(), atm_storage::AtmError> {
        let fingerprint = require_non_blank(peer.fingerprint.clone(), "trusted-peer fingerprint")?;
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO peer_trusted_peers(host, fingerprint, enabled)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(host) DO UPDATE SET
                         fingerprint = excluded.fingerprint,
                         enabled = excluded.enabled",
                    params![peer.host.as_str(), fingerprint, i64::from(peer.enabled)],
                )
                .map(|_| ())
                .map_err(|error| self.db.error("failed to save trusted peer", error))
        })
    }

    fn remove_trusted_peer(&self, host: &HostName) -> Result<bool, atm_storage::AtmError> {
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "DELETE FROM peer_trusted_peers WHERE host = ?1",
                    params![host.as_str()],
                )
                .map(|count| count > 0)
                .map_err(|error| self.db.error("failed to remove trusted peer", error))
        })
    }
}

fn parse_bind_addr(value: &str) -> Result<SocketAddr, atm_storage::AtmError> {
    value.parse().map_err(|error| {
        atm_storage::AtmError::validation(format!(
            "invalid stored HTTPS bind address `{value}`: {error}"
        ))
    })
}

fn parse_host(value: &str) -> Result<HostName, atm_storage::AtmError> {
    value.parse().map_err(|error| {
        atm_storage::AtmError::validation(format!("invalid stored peer host `{value}`: {error}"))
    })
}

fn require_non_blank(value: String, subject: &str) -> Result<String, atm_storage::AtmError> {
    if value.trim().is_empty() {
        return Err(atm_storage::AtmError::validation(format!(
            "{subject} must not be blank"
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use atm_storage::{HttpsInterface, LocalCertificate, TrustedPeer};

    #[test]
    fn peer_configuration_round_trips_without_private_key_material() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.peer_config_store();
        let interface = HttpsInterface {
            bind_addr: "127.0.0.1:43101".parse().expect("bind address"),
            advertise_host: "localhost".parse().expect("host"),
            enabled: true,
        };
        store.save_interface(&interface).expect("save interface");
        store
            .save_local_certificate(&LocalCertificate {
                fingerprint: "sha256:test".to_string(),
                private_key_ref: "keychain://atm/test".to_string(),
            })
            .expect("save certificate");
        store
            .save_trusted_peer(&TrustedPeer {
                host: "peer.example".parse().expect("host"),
                fingerprint: "sha256:peer".to_string(),
                enabled: true,
            })
            .expect("save peer");

        assert_eq!(
            store.list_interfaces().expect("interfaces"),
            vec![interface]
        );
        assert_eq!(
            store
                .local_certificate()
                .expect("certificate")
                .expect("present")
                .fingerprint,
            "sha256:test"
        );
        assert_eq!(store.list_trusted_peers().expect("peers").len(), 1);
    }

    #[test]
    fn peer_configuration_rejects_blank_secret_references_and_fingerprints() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.peer_config_store();
        assert!(
            store
                .save_local_certificate(&LocalCertificate {
                    fingerprint: " ".to_string(),
                    private_key_ref: "keychain://atm/test".to_string(),
                })
                .is_err()
        );
        assert!(
            store
                .save_trusted_peer(&TrustedPeer {
                    host: "peer.example".parse().expect("host"),
                    fingerprint: " ".to_string(),
                    enabled: true,
                })
                .is_err()
        );
    }
}
