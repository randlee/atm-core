use std::marker::PhantomData;
use std::time::Duration;

use atm_core::protocol::{CompatibilityPreflight, CompatibilityVerdict, ReleaseVersion};
use atm_storage::{AtmError, AtmErrorCode, AtmErrorKind};

use crate::rpc::RpcHeader;
use crate::wire::MessageKind;
use crate::{DaemonLocalIpcEndpoint, RpcEnvelope, exchange_envelope};

pub struct Unverified;
pub struct VersionVerified {
    daemon_release: ReleaseVersion,
}

/// A typestate guard for same-host write dispatch. Transport integration owns
/// construction; only a verified connection can be used for writes.
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
            return Err(AtmError::new_with_code(
                AtmErrorCode::ClientDaemonVersionIncompatible,
                AtmErrorKind::DaemonUnavailable,
                format!(
                    "ATM client release {} is incompatible with daemon release {daemon_release}",
                    self.preflight.client_release
                ),
            )
            .with_recovery(
                "Install matching atm and atm-daemon releases; no request was dispatched.",
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
        request: RpcEnvelope,
        request_deadline: Duration,
    ) -> Result<RpcEnvelope, AtmError> {
        exchange_envelope(endpoint, request, request_deadline)
    }
}

pub fn verify_connection_compatibility(
    endpoint: &DaemonLocalIpcEndpoint,
    preflight: CompatibilityPreflight,
    request_deadline: Duration,
) -> Result<Connection<VersionVerified>, AtmError> {
    let request = RpcEnvelope::encode_body(
        RpcHeader::new(
            crate::RequestId::new(atm_core::protocol::next_request_id().into_inner())?,
            MessageKind::CompatibilityPreflightRequest,
        ),
        &preflight,
    )?;
    let response = exchange_envelope(endpoint, request, request_deadline)?;
    let verdict: CompatibilityVerdict = response.decode_body()?;
    let connection = Connection::<Unverified>::new(preflight.clone());
    match verdict {
        CompatibilityVerdict::Compatible { daemon_release } => {
            connection.verify_compatibility(daemon_release)
        }
        CompatibilityVerdict::Incompatible {
            client_release,
            daemon_release,
            code,
        } => Err(AtmError::new_with_code(
            code,
            AtmErrorKind::DaemonUnavailable,
            format!(
                "ATM client release {client_release} is incompatible with daemon release {daemon_release}"
            ),
        )
        .with_recovery(
            "Install matching atm and atm-daemon releases; no request was dispatched.",
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
}
