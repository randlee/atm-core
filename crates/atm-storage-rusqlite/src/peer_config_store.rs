use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU16;
use std::sync::Arc;

use atm_storage::{
    CertificateFingerprint, HostName, HttpsInterface, LocalCertificate, PeerAliasKey,
    PeerConfigStore, PeerDirectory, PrivateKeyRef, TrustedPeer, validate_canonical_peer_host,
};
use rusqlite::{OptionalExtension, params};

use crate::SqlitePeerConfigStore;
use crate::shared_db::SharedDb;

impl SqlitePeerConfigStore {
    pub(crate) fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }
}

impl atm_storage::contract::sealed::Sealed for SqlitePeerConfigStore {}

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
                    fingerprint: parse_fingerprint(fingerprint)?,
                    private_key_ref: parse_private_key_ref(private_key_ref)?,
                })
            })
            .transpose()
    }

    fn save_local_certificate(
        &self,
        certificate: &LocalCertificate,
    ) -> Result<(), atm_storage::AtmError> {
        let fingerprint = certificate.fingerprint.as_str();
        let private_key_ref = certificate.private_key_ref.as_str();
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
                    "SELECT host, fingerprint, enabled, https_port
                     FROM peer_trusted_peers ORDER BY host",
                )
                .map_err(|error| self.db.error("failed to prepare trusted-peer query", error))?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, u16>(3)?,
                    ))
                })
                .map_err(|error| self.db.error("failed to query trusted peers", error))?
                .map(|row| {
                    let (host, fingerprint, enabled, https_port) =
                        row.map_err(|error| self.db.error("failed to read trusted peer", error))?;
                    Ok(TrustedPeer {
                        host: parse_host(&host)?,
                        fingerprint: parse_fingerprint(fingerprint)?,
                        enabled: enabled != 0,
                        https_port: NonZeroU16::new(https_port).ok_or_else(|| {
                            atm_storage::AtmError::validation("stored HTTPS peer port was zero")
                        })?,
                    })
                })
                .collect()
        })
    }

    fn trusted_peer(&self, host: &HostName) -> Result<Option<TrustedPeer>, atm_storage::AtmError> {
        let peer = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT fingerprint, enabled, https_port FROM peer_trusted_peers WHERE host = ?1",
                    params![host.as_str()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, u16>(2)?)),
                )
                .optional()
                .map_err(|error| self.db.error("failed to load trusted peer", error))
        })?;
        peer.map(|(fingerprint, enabled, https_port)| {
            Ok(TrustedPeer {
                host: host.clone(),
                fingerprint: parse_fingerprint(fingerprint)?,
                enabled: enabled != 0,
                https_port: NonZeroU16::new(https_port).ok_or_else(|| {
                    atm_storage::AtmError::validation("stored HTTPS peer port was zero")
                })?,
            })
        })
        .transpose()
    }

    fn save_trusted_peer(&self, peer: &TrustedPeer) -> Result<(), atm_storage::AtmError> {
        validate_canonical_peer_host(&peer.host)?;
        let fingerprint = peer.fingerprint.as_str();
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO peer_trusted_peers(host, fingerprint, enabled, https_port)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(host) DO UPDATE SET
                         fingerprint = excluded.fingerprint,
                         enabled = excluded.enabled, https_port = excluded.https_port",
                    params![
                        peer.host.as_str(),
                        fingerprint,
                        i64::from(peer.enabled),
                        peer.https_port.get()
                    ],
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

    fn peer_directory(&self) -> Result<PeerDirectory, atm_storage::AtmError> {
        PeerDirectory::from_configuration(self.list_trusted_peers()?, self.list_peer_aliases()?)
    }

    fn list_peer_aliases(&self) -> Result<Vec<(PeerAliasKey, HostName)>, atm_storage::AtmError> {
        self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT alias_kind, alias_value, canonical_host
                     FROM peer_aliases ORDER BY alias_kind, alias_value",
                )
                .map_err(|error| self.db.error("failed to prepare peer-alias query", error))?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(|error| self.db.error("failed to query peer aliases", error))?
                .map(|row| {
                    let (kind, value, canonical_host) =
                        row.map_err(|error| self.db.error("failed to read peer alias", error))?;
                    Ok((
                        parse_peer_alias(&kind, &value)?,
                        parse_host(&canonical_host)?,
                    ))
                })
                .collect()
        })
    }

    fn save_peer_alias(
        &self,
        alias: PeerAliasKey,
        canonical_host: HostName,
    ) -> Result<(), atm_storage::AtmError> {
        validate_canonical_peer_host(&canonical_host)?;
        let alias_kind = alias.alias_kind();
        let alias_value = alias.alias_value();
        self.db.with_connection(|connection| {
            let canonical_enabled = connection
                .query_row(
                    "SELECT enabled FROM peer_trusted_peers WHERE host = ?1",
                    params![canonical_host.as_str()],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|error| self.db.error("failed to load canonical trusted peer", error))?;
            if canonical_enabled != Some(1) {
                return Err(atm_storage::AtmError::peer_config_validation(format!(
                    "peer alias `{alias}` requires an enabled trusted canonical peer `{canonical_host}`"
                )));
            }
            if let PeerAliasKey::Host(host) = &alias {
                let synthesized_alias_exists = connection
                    .query_row(
                        "SELECT EXISTS(
                            SELECT 1 FROM peer_trusted_peers WHERE host = ?1 AND enabled = 1
                         )",
                        params![host.as_str()],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(|error| {
                        self.db
                            .error("failed to validate synthesized peer alias", error)
                    })?;
                if synthesized_alias_exists != 0 {
                    return Err(atm_storage::AtmError::peer_config_validation(format!(
                        "peer alias `{alias}` duplicates a synthesized canonical-host alias"
                    )));
                }
            }
            connection
                .execute(
                    "INSERT INTO peer_aliases(alias_kind, alias_value, canonical_host)
                     VALUES (?1, ?2, ?3)",
                    params![alias_kind, alias_value, canonical_host.as_str()],
                )
                .map(|_| ())
                .map_err(|error| self.db.error("failed to save peer alias", error))
        })
    }

    fn remove_peer_alias(&self, alias: &PeerAliasKey) -> Result<bool, atm_storage::AtmError> {
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "DELETE FROM peer_aliases WHERE alias_kind = ?1 AND alias_value = ?2",
                    params![alias.alias_kind(), alias.alias_value()],
                )
                .map(|count| count > 0)
                .map_err(|error| self.db.error("failed to remove peer alias", error))
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
    let host = value.parse().map_err(|error| {
        atm_storage::AtmError::validation(format!("invalid stored peer host `{value}`: {error}"))
    })?;
    validate_canonical_peer_host(&host)?;
    Ok(host)
}

fn parse_peer_alias(kind: &str, value: &str) -> Result<PeerAliasKey, atm_storage::AtmError> {
    match kind {
        "host" => {
            if value.parse::<IpAddr>().is_ok() {
                return Err(atm_storage::AtmError::peer_config_validation(format!(
                    "stored host peer alias `{value}` must not be an IP literal"
                )));
            }
            parse_host(value).map(PeerAliasKey::Host)
        }
        "ip" => value
            .parse::<IpAddr>()
            .map(PeerAliasKey::Ip)
            .map_err(|error| {
                atm_storage::AtmError::peer_config_validation(format!(
                    "stored IP peer alias `{value}` is invalid: {error}"
                ))
            }),
        _ => Err(atm_storage::AtmError::peer_config_validation(format!(
            "stored peer alias kind `{kind}` is invalid"
        ))),
    }
}

fn parse_fingerprint(value: String) -> Result<CertificateFingerprint, atm_storage::AtmError> {
    value.parse()
}

fn parse_private_key_ref(value: String) -> Result<PrivateKeyRef, atm_storage::AtmError> {
    value.parse()
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use atm_storage::{HostName, HttpsInterface, LocalCertificate, PeerAliasKey, TrustedPeer};
    use rusqlite::Connection;

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
                fingerprint: "sha256:test".parse().expect("fingerprint"),
                private_key_ref: "keychain://atm/test".parse().expect("key reference"),
            })
            .expect("save certificate");
        store
            .save_trusted_peer(&TrustedPeer {
                host: "peer.example".parse().expect("host"),
                fingerprint: "sha256:peer".parse().expect("fingerprint"),
                enabled: true,
                https_port: std::num::NonZeroU16::new(43101).expect("non-zero"),
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
                .fingerprint
                .as_str(),
            "sha256:test"
        );
        assert_eq!(store.list_trusted_peers().expect("peers").len(), 1);
    }

    #[test]
    fn peer_configuration_rejects_blank_secret_references_and_fingerprints() {
        assert!(" ".parse::<atm_storage::CertificateFingerprint>().is_err());
        assert!(" ".parse::<atm_storage::PrivateKeyRef>().is_err());
    }

    #[test]
    fn pre_ak3_database_opens_with_an_empty_peer_alias_table() {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "atm-pre-ak3-peer-aliases-{}-{unique_suffix}.sqlite3",
            std::process::id()
        ));
        let legacy_schema = crate::shared_db::DB_MIGRATIONS.replacen(
            "CREATE TABLE IF NOT EXISTS peer_aliases (\n    alias_kind TEXT NOT NULL CHECK(alias_kind IN ('host', 'ip')),\n    alias_value TEXT NOT NULL,\n    canonical_host TEXT NOT NULL REFERENCES peer_trusted_peers(host) ON DELETE CASCADE,\n    UNIQUE(alias_kind, alias_value)\n);\n\n",
            "",
            1,
        );
        Connection::open(&path)
            .expect("open pre-AK.3 fixture")
            .execute_batch(&legacy_schema)
            .expect("create pre-AK.3 fixture schema");

        let backend = crate::SqliteStorageBackend::new(&path).expect("open migrated fixture");
        assert!(
            backend
                .peer_config_store()
                .list_peer_aliases()
                .expect("read migrated alias table")
                .is_empty()
        );
        drop(backend);
        std::fs::remove_file(&path).expect("remove temporary pre-AK.3 fixture");
    }

    #[test]
    fn peer_configuration_rejects_ip_literal_canonical_hosts_but_accepts_ip_aliases() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.peer_config_store();
        let peer = TrustedPeer {
            host: "127.0.0.1".parse().expect("host syntax"),
            fingerprint: "sha256:peer".parse().expect("fingerprint"),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("port"),
        };
        let error = store
            .save_trusted_peer(&peer)
            .expect_err("an address cannot be the stable canonical peer identity");
        assert!(error.message().contains("must not be an IP literal"));

        let canonical: HostName = "rand-m5.local".parse().expect("canonical host");
        store
            .save_trusted_peer(&TrustedPeer {
                host: canonical.clone(),
                ..peer
            })
            .expect("DNS canonical peer");
        store
            .save_peer_alias("127.0.0.1".parse().expect("IP alias"), canonical)
            .expect("IP aliases remain valid peer lookup inputs");
    }

    #[test]
    fn peer_aliases_require_enabled_canonical_peer_and_cascade_on_removal() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.peer_config_store();
        let canonical: HostName = "rand-m5.local".parse().expect("canonical host");
        let alias: PeerAliasKey = "192.168.128.82".parse().expect("IP alias");

        assert!(
            store
                .save_peer_alias(alias.clone(), canonical.clone())
                .is_err()
        );

        store
            .save_trusted_peer(&TrustedPeer {
                host: canonical.clone(),
                fingerprint: "sha256:m5".parse().expect("fingerprint"),
                enabled: true,
                https_port: std::num::NonZeroU16::new(43101).expect("port"),
            })
            .expect("save canonical peer");
        store
            .save_peer_alias(alias.clone(), canonical.clone())
            .expect("save alias");
        assert_eq!(store.list_peer_aliases().expect("aliases").len(), 1);
        assert_eq!(
            store
                .peer_directory()
                .expect("directory")
                .normalize(&alias)
                .expect("endpoint")
                .canonical_host,
            canonical
        );

        store
            .remove_trusted_peer(&canonical)
            .expect("remove canonical peer");
        assert!(
            store
                .list_peer_aliases()
                .expect("aliases after cascade")
                .is_empty()
        );
    }

    #[test]
    fn peer_aliases_reject_duplicates_and_synthesized_canonical_host_aliases() {
        let backend = crate::SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.peer_config_store();
        let canonical: HostName = "rand-m5.local".parse().expect("canonical host");
        store
            .save_trusted_peer(&TrustedPeer {
                host: canonical.clone(),
                fingerprint: "sha256:m5".parse().expect("fingerprint"),
                enabled: true,
                https_port: std::num::NonZeroU16::new(43101).expect("port"),
            })
            .expect("save canonical peer");
        let alias: PeerAliasKey = "192.168.128.82".parse().expect("IP alias");
        store
            .save_peer_alias(alias.clone(), canonical.clone())
            .expect("save alias");
        assert!(store.save_peer_alias(alias, canonical.clone()).is_err());
        assert!(
            store
                .save_peer_alias(canonical.as_str().parse().expect("host alias"), canonical,)
                .is_err()
        );
    }
}
