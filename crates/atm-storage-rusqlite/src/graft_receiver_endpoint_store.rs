use std::sync::Arc;

use atm_storage::contract::{
    GraftEndpointStoreError, GraftReceiverEndpointStore, GraftReceiverLease,
    GraftReceiverRegistration, sealed,
};
use atm_storage::types::{AgentName, LocalCapability, TeamName};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};

use crate::shared_db::SharedDb;

pub(crate) struct SqliteGraftReceiverEndpointStore {
    pub(crate) db: Arc<SharedDb>,
}

impl SqliteGraftReceiverEndpointStore {
    pub(crate) fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }

    fn storage_error(error: atm_storage::AtmError) -> GraftEndpointStoreError {
        GraftEndpointStoreError::Storage(error.to_string())
    }
}

impl sealed::Sealed for SqliteGraftReceiverEndpointStore {}

impl GraftReceiverEndpointStore for SqliteGraftReceiverEndpointStore {
    fn register(
        &self,
        registration: &GraftReceiverRegistration,
        now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError> {
        let capability = registration.capability.to_base64url();
        self.db
            .with_transaction(|transaction| {
                transaction
                    .execute(
                        "INSERT INTO graft_receiver_endpoints
                            (team, agent, endpoint, capability, owner_generation,
                             registered_at, last_seen_at, unreachable_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, NULL)
                         ON CONFLICT(team, agent) DO UPDATE SET
                            endpoint = excluded.endpoint,
                            capability = excluded.capability,
                            owner_generation = excluded.owner_generation,
                            registered_at = excluded.registered_at,
                            last_seen_at = excluded.last_seen_at,
                            unreachable_at = NULL;",
                        params![
                            registration.team.as_str(),
                            registration.agent.as_str(),
                            registration.endpoint.to_string(),
                            capability,
                            registration.owner_generation,
                            now.to_rfc3339(),
                        ],
                    )
                    .map_err(|error| {
                        self.db
                            .error("failed to register graft receiver endpoint", error)
                    })?;
                Ok(())
            })
            .map_err(Self::storage_error)
    }

    fn refresh(
        &self,
        team: &TeamName,
        agent: &AgentName,
        owner_generation: &str,
        now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError> {
        let changed = self
            .db
            .with_transaction(|transaction| {
                transaction
                    .execute(
                        "UPDATE graft_receiver_endpoints
                         SET last_seen_at = ?1, unreachable_at = NULL
                         WHERE team = ?2 AND agent = ?3 AND owner_generation = ?4;",
                        params![
                            now.to_rfc3339(),
                            team.as_str(),
                            agent.as_str(),
                            owner_generation
                        ],
                    )
                    .map_err(|error| {
                        self.db
                            .error("failed to refresh graft receiver endpoint", error)
                    })
            })
            .map_err(Self::storage_error)?;
        if changed == 0 {
            return Err(GraftEndpointStoreError::NotOwner);
        }
        Ok(())
    }

    fn unregister(
        &self,
        team: &TeamName,
        agent: &AgentName,
        owner_generation: &str,
    ) -> Result<(), GraftEndpointStoreError> {
        self.db
            .with_transaction(|transaction| {
                transaction
                    .execute(
                        "DELETE FROM graft_receiver_endpoints
                         WHERE team = ?1 AND agent = ?2 AND owner_generation = ?3;",
                        params![team.as_str(), agent.as_str(), owner_generation],
                    )
                    .map_err(|error| {
                        self.db
                            .error("failed to unregister graft receiver endpoint", error)
                    })?;
                Ok(())
            })
            .map_err(Self::storage_error)
    }

    fn lookup(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<GraftReceiverLease>, GraftEndpointStoreError> {
        self.db
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT endpoint, capability, owner_generation,
                                registered_at, last_seen_at, unreachable_at
                         FROM graft_receiver_endpoints
                         WHERE team = ?1 AND agent = ?2;",
                        params![team.as_str(), agent.as_str()],
                        |row| {
                            let endpoint = row.get::<_, String>(0)?.parse().map_err(|error| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    0,
                                    rusqlite::types::Type::Text,
                                    Box::new(error),
                                )
                            })?;
                            let capability =
                                LocalCapability::parse_base64url(&row.get::<_, String>(1)?)
                                    .map_err(|error| {
                                        rusqlite::Error::FromSqlConversionFailure(
                                            1,
                                            rusqlite::types::Type::Text,
                                            Box::new(std::io::Error::other(error.to_string())),
                                        )
                                    })?;
                            Ok(GraftReceiverLease {
                                endpoint,
                                capability,
                                owner_generation: row.get(2)?,
                                registered_at: parse_timestamp(row.get::<_, String>(3)?)?,
                                last_seen_at: parse_timestamp(row.get::<_, String>(4)?)?,
                                unreachable_since: row
                                    .get::<_, Option<String>>(5)?
                                    .map(parse_timestamp)
                                    .transpose()?,
                            })
                        },
                    )
                    .optional()
                    .map_err(|error| {
                        self.db
                            .error("failed to look up graft receiver endpoint", error)
                    })
            })
            .map_err(Self::storage_error)
    }

    fn mark_unreachable(
        &self,
        team: &TeamName,
        agent: &AgentName,
        owner_generation: &str,
        now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError> {
        let changed = self
            .db
            .with_transaction(|transaction| {
                transaction
                    .execute(
                        "UPDATE graft_receiver_endpoints
                         SET unreachable_at = COALESCE(unreachable_at, ?1)
                         WHERE team = ?2 AND agent = ?3 AND owner_generation = ?4;",
                        params![
                            now.to_rfc3339(),
                            team.as_str(),
                            agent.as_str(),
                            owner_generation
                        ],
                    )
                    .map_err(|error| {
                        self.db
                            .error("failed to mark graft receiver endpoint unreachable", error)
                    })
            })
            .map_err(Self::storage_error)?;
        if changed == 0 {
            return Err(GraftEndpointStoreError::NotOwner);
        }
        Ok(())
    }
}

fn parse_timestamp(value: String) -> Result<DateTime<Utc>, rusqlite::Error> {
    DateTime::parse_from_rfc3339(&value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use atm_storage::types::{AgentName, TeamName};
    use chrono::TimeZone;

    fn names() -> (TeamName, AgentName) {
        (
            TeamName::from_validated("test-team"),
            AgentName::from_validated("test-agent"),
        )
    }

    fn timestamp(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().expect("timestamp")
    }

    fn registration(generation: &str, endpoint: &str) -> GraftReceiverRegistration {
        let (team, agent) = names();
        GraftReceiverRegistration {
            team,
            agent,
            endpoint: endpoint.parse().expect("endpoint"),
            capability: LocalCapability::generate().expect("capability"),
            owner_generation: generation.to_owned(),
        }
    }

    fn store() -> SqliteGraftReceiverEndpointStore {
        SqliteGraftReceiverEndpointStore::new(Arc::new(
            SharedDb::open_in_memory_for_test().expect("sqlite database"),
        ))
    }

    #[test]
    fn register_lookup_refresh_unreachable_and_unregister_round_trip() {
        let store = store();
        let (team, agent) = names();
        let registration = registration("generation-1", "127.0.0.1:43101");
        let registered_at = timestamp(10);
        let refreshed_at = timestamp(20);
        let unreachable_at = timestamp(30);

        store
            .register(&registration, registered_at)
            .expect("register");
        let lease = store.lookup(&team, &agent).expect("lookup").expect("lease");
        assert_eq!(lease.endpoint, registration.endpoint);
        assert_eq!(lease.capability, registration.capability);
        assert_eq!(lease.owner_generation, "generation-1");
        assert_eq!(lease.registered_at, registered_at);
        assert_eq!(lease.last_seen_at, registered_at);
        assert_eq!(lease.unreachable_since, None);

        store
            .mark_unreachable(&team, &agent, "generation-1", unreachable_at)
            .expect("mark unreachable");
        assert_eq!(
            store
                .lookup(&team, &agent)
                .expect("lookup")
                .expect("lease")
                .unreachable_since,
            Some(unreachable_at)
        );
        store
            .refresh(&team, &agent, "generation-1", refreshed_at)
            .expect("refresh");
        let lease = store.lookup(&team, &agent).expect("lookup").expect("lease");
        assert_eq!(lease.last_seen_at, refreshed_at);
        assert_eq!(lease.unreachable_since, None);

        store
            .unregister(&team, &agent, "generation-1")
            .expect("unregister");
        assert_eq!(store.lookup(&team, &agent).expect("lookup"), None);
    }

    #[test]
    fn register_refreshes_matching_generation_and_displaces_other_generations() {
        let store = store();
        let (team, agent) = names();
        let first = registration("generation-1", "127.0.0.1:43101");
        store.register(&first, timestamp(10)).expect("register");

        let mut refreshed = first.clone();
        refreshed.endpoint = "127.0.0.1:43102".parse().expect("endpoint");
        store
            .register(&refreshed, timestamp(20))
            .expect("refresh register");
        let lease = store.lookup(&team, &agent).expect("lookup").expect("lease");
        assert_eq!(lease.endpoint, refreshed.endpoint);
        assert_eq!(lease.registered_at, timestamp(20));

        let displaced = registration("generation-2", "127.0.0.1:43103");
        store.register(&displaced, timestamp(30)).expect("displace");
        let lease = store.lookup(&team, &agent).expect("lookup").expect("lease");
        assert_eq!(lease.endpoint, displaced.endpoint);
        assert_eq!(lease.owner_generation, "generation-2");
        assert_eq!(
            store.refresh(&team, &agent, "generation-1", timestamp(40)),
            Err(GraftEndpointStoreError::NotOwner)
        );
    }

    #[test]
    fn lease_survives_backend_reopen() {
        let directory = tempfile::tempdir().expect("temporary state root");
        let path = directory.path().join("mail.db");
        let registration = registration("generation-1", "127.0.0.1:43101");
        let (team, agent) = names();
        {
            let backend = crate::SqliteStorageBackend::new(&path).expect("backend");
            backend
                .graft_receiver_endpoint_store()
                .register(&registration, timestamp(10))
                .expect("register");
        }
        let backend = crate::SqliteStorageBackend::new(&path).expect("reopened backend");
        let lease = backend
            .graft_receiver_endpoint_store()
            .lookup(&team, &agent)
            .expect("lookup")
            .expect("persisted lease");
        assert_eq!(lease.owner_generation, "generation-1");
        assert_eq!(lease.registered_at, timestamp(10));
    }
}
