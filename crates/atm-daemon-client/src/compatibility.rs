use std::time::Duration;

use atm_core::protocol::{
    CompatibilityPreflight, CompatibilityVerdict, HttpApiVersion, ReleaseVersion, RequestEnvelope,
    ResponseEnvelope,
};
use atm_storage::AtmError;

use crate::{DaemonLocalIpcEndpoint, exchange_request};

pub struct Unverified;
pub struct VersionVerified {
    daemon_release: ReleaseVersion,
    daemon_schema_version: u16,
    daemon_http_api_version: HttpApiVersion,
}

/// A typestate guard for same-host write dispatch. Transport integration owns
/// construction; only a verified connection can be used for writes.
///
/// ```compile_fail
/// use atm_core::protocol::{CompatibilityPreflight, HttpApiVersion, ReleaseVersion};
/// use atm_daemon_client::Connection;
///
/// let mut connection = Connection::new(CompatibilityPreflight {
///     client_release: ReleaseVersion::parse("1.3.1").unwrap(),
///     cli_schema_version: 1,
///     http_api_version: HttpApiVersion::parse("1.0.0").unwrap(),
/// });
/// let endpoint_path = std::env::temp_dir().join("atm-daemon.sock");
/// let endpoint = atm_daemon_client::DaemonLocalIpcEndpoint::new(endpoint_path).unwrap();
/// let request = atm_core::protocol::RequestEnvelope::CompatibilityPreflight(CompatibilityPreflight {
///         client_release: ReleaseVersion::parse("1.3.1").unwrap(),
///         cli_schema_version: 1,
///         http_api_version: HttpApiVersion::parse("1.0.0").unwrap(),
///     });
/// let _ = connection.dispatch_write(&endpoint, request, std::time::Duration::from_secs(3));
/// ```
pub struct Connection<State> {
    preflight: CompatibilityPreflight,
    state: State,
}

impl Connection<Unverified> {
    pub fn new(preflight: CompatibilityPreflight) -> Self {
        Self {
            preflight,
            state: Unverified,
        }
    }

    pub fn verify_compatibility(
        self,
        daemon_release: ReleaseVersion,
        daemon_schema_version: u16,
        daemon_http_api_version: HttpApiVersion,
    ) -> Result<Connection<VersionVerified>, AtmError> {
        Ok(Connection {
            preflight: self.preflight,
            state: VersionVerified {
                daemon_release,
                daemon_schema_version,
                daemon_http_api_version,
            },
        })
    }
}

impl Connection<VersionVerified> {
    pub fn daemon_release(&self) -> &ReleaseVersion {
        &self.state.daemon_release
    }

    pub fn daemon_schema_version(&self) -> u16 {
        self.state.daemon_schema_version
    }

    pub fn daemon_http_api_version(&self) -> &HttpApiVersion {
        &self.state.daemon_http_api_version
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

#[deprecated(
    note = "legacy synchronous IPC compatibility exchange; use atm-http-runtime's Tokio client"
)]
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
        CompatibilityVerdict::Compatible {
            daemon_release,
            daemon_schema_version,
            daemon_http_api_version,
        } => connection.verify_compatibility(
            daemon_release,
            daemon_schema_version,
            daemon_http_api_version,
        ),
        CompatibilityVerdict::Incompatible {
            client_release,
            daemon_release,
            client_schema_version,
            daemon_schema_version,
            client_http_api_version,
            daemon_http_api_version,
            code,
        } => Err(AtmError::new(
            code,
            format!(
                "ATM compatibility mismatch: client release {client_release}, daemon release {daemon_release}; schema {client_schema_version}/{daemon_schema_version}; HTTP API {client_http_api_version}/{daemon_http_api_version}"
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{CompatibilityPreflight, Connection, HttpApiVersion, ReleaseVersion, Unverified};
    use atm_core::protocol::CLI_SCHEMA_VERSION;

    #[test]
    fn matching_versions_transition_to_verified_connection() {
        let version = ReleaseVersion::parse("1.3.1").expect("version");
        let connection = Connection::<Unverified>::new(CompatibilityPreflight {
            client_release: version.clone(),
            cli_schema_version: CLI_SCHEMA_VERSION,
            http_api_version: HttpApiVersion::current(),
        })
        .verify_compatibility(version, CLI_SCHEMA_VERSION, HttpApiVersion::current())
        .expect("compatible");
        assert_eq!(connection.daemon_release().to_string(), "1.3.1");
    }

    #[test]
    fn compatibility_preflight_retains_canonical_request_shape() {
        let preflight = CompatibilityPreflight {
            client_release: ReleaseVersion::parse("1.3.1").expect("version"),
            cli_schema_version: CLI_SCHEMA_VERSION,
            http_api_version: HttpApiVersion::current(),
        };
        let request =
            atm_core::protocol::RequestEnvelope::CompatibilityPreflight(preflight.clone());
        let (_, path) = atm_core::api::endpoint_for(&request);
        assert_eq!(path, "/v1/atm/compatibility");
    }
}
