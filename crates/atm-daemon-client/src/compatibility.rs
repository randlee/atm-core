use std::fmt;
use std::marker::PhantomData;

use atm_storage::{AtmError, AtmErrorCode, AtmErrorKind};

/// Normalized release identity used by the client/daemon compatibility gate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ReleaseVersion(String);

impl ReleaseVersion {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, AtmError> {
        let value = value
            .as_ref()
            .trim()
            .strip_prefix('v')
            .unwrap_or(value.as_ref().trim());
        let mut parts = value.split('.');
        let valid = (0..3).all(|_| {
            parts.next().is_some_and(|part| {
                !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
            })
        }) && parts.next().is_none();
        if !valid {
            return Err(AtmError::new_with_code(
                AtmErrorCode::ClientDaemonVersionIncompatible,
                AtmErrorKind::DaemonUnavailable,
                format!("invalid ATM release version `{value}`"),
            )
            .with_recovery(
                "Install a matching released atm and atm-daemon pair before retrying.",
            ));
        }
        Ok(Self(value.to_string()))
    }

    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("package version must be semver")
    }
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompatibilityPreflight {
    pub client_release: ReleaseVersion,
    pub wire_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CompatibilityVerdict {
    Compatible {
        daemon_release: ReleaseVersion,
    },
    Incompatible {
        client_release: ReleaseVersion,
        daemon_release: ReleaseVersion,
        code: AtmErrorCode,
    },
}

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
