use std::marker::PhantomData;
use std::time::Duration;

use atm_core::protocol::{
    CompatibilityPreflight, CompatibilityVerdict, ReleaseVersion, RequestEnvelope, ResponseEnvelope,
};
use atm_storage::{AtmError, AtmErrorCode};

use crate::{DaemonLocalIpcEndpoint, exchange_request};

pub struct Unverified;
pub struct VersionVerified {
    daemon_release: ReleaseVersion,
}

/// A typestate guard for same-host write dispatch. Transport integration owns
/// construction; only a verified connection can be used for writes.
///
/// ```compile_fail
/// use atm_core::protocol::{CompatibilityPreflight, ReleaseVersion};
/// use atm_daemon_client::Connection;
///
/// let mut connection = Connection::new(CompatibilityPreflight {
///     client_release: ReleaseVersion::parse("1.3.1").unwrap(),
///     wire_version: 1,
/// });
/// let endpoint = atm_daemon_client::DaemonLocalIpcEndpoint::new("/tmp/atm-daemon.sock".into()).unwrap();
/// let request = atm_core::protocol::RequestEnvelope::CompatibilityPreflight(CompatibilityPreflight {
///         client_release: ReleaseVersion::parse("1.3.1").unwrap(),
///         wire_version: 1,
///     }),
/// let _ = connection.dispatch_write(&endpoint, request, std::time::Duration::from_secs(3));
/// ```
pub struct Connection<State> {
    preflight: CompatibilityPreflight,
    state: State,
    _marker: PhantomData<State>,
}

impl Connection<Unverified> {
    pub fn new(preflight: CompatibilityPreflight) -> Self {
        Self {
            preflight,
            state: Unverified,
            _marker: PhantomData,
        }
    }

    pub fn verify_compatibility(
        self,
        daemon_release: ReleaseVersion,
    ) -> Result<Connection<VersionVerified>, AtmError> {
        if self.preflight.client_release != daemon_release {
            return Err(AtmError::new(
                AtmErrorCode::ClientDaemonVersionIncompatible,
                format!(
                    "ATM client release {} is incompatible with daemon release {}",
                    self.preflight.client_release, daemon_release
                ),
            ));
        }
        Ok(Connection {
            preflight: self.preflight,
            state: VersionVerified { daemon_release },
            _marker: PhantomData,
        })
    }
}

impl Connection<VersionVerified> {
    pub fn daemon_release(&self) -> &ReleaseVersion {
        &self.state.daemon_release
    }

    pub fn dispatch_write(
        &mut self,
        endpoint: &DaemonLocalIpcEndpoint,
        request: RequestEnvelope,
        request_deadline: Duration,
    ) -> Result<ResponseEnvelope, AtmError> {
        exchange_request(endpoint, &request, request_deadline)
    }
}

pub fn verify_connection_compatibility(
    endpoint: &DaemonLocalIpcEndpoint,
    preflight: CompatibilityPreflight,
    request_deadline: Duration,
) -> Result<Connection<VersionVerified>, AtmError> {
    let response = exchange_request(
        endpoint,
        &RequestEnvelope::CompatibilityPreflight(preflight.clone()),
        request_deadline,
    )?;
    let verdict = match response {
        ResponseEnvelope::CompatibilityVerdict(verdict) => verdict,
        other => {
            return Err(AtmError::daemon_unavailable(format!(
                "daemon returned an unexpected response for compatibility preflight: {other:?}"
            )));
        }
    };
    let connection = Connection::<Unverified>::new(preflight.clone());
    match verdict {
        CompatibilityVerdict::Compatible { daemon_release } => {
            connection.verify_compatibility(daemon_release)
        }
        CompatibilityVerdict::Incompatible {
            client_release,
            daemon_release,
            code,
        } => Err(AtmError::new(
            code,
            format!(
                "ATM client release {client_release} is incompatible with daemon release {daemon_release}"
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{CompatibilityPreflight, Connection, ReleaseVersion, Unverified};

    #[test]
    fn matching_versions_transition_to_verified_connection() {
        let version = ReleaseVersion::parse("1.3.1").expect("version");
        let connection = Connection::<Unverified>::new(CompatibilityPreflight {
            client_release: version.clone(),
            wire_version: 1,
        })
        .verify_compatibility(version)
        .expect("compatible");
        assert_eq!(connection.daemon_release().to_string(), "1.3.1");
    }

    #[test]
    fn compatibility_preflight_retains_canonical_request_shape() {
        let preflight = CompatibilityPreflight {
            client_release: ReleaseVersion::parse("1.3.1").expect("version"),
            wire_version: 1,
        };
        let request =
            atm_core::protocol::RequestEnvelope::CompatibilityPreflight(preflight.clone());
        let (_, path) = atm_core::api::endpoint_for(&request);
        assert_eq!(path, "/v1/atm/compatibility");
    }
}
