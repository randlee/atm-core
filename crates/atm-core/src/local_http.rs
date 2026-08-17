//! Runtime metadata for authenticated local HTTP ingress.
//!
//! This record is intentionally transport metadata only: it publishes no
//! storage handle, routing decision, or application state.

use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::error::AtmError;
use crate::types::IsoTimestamp;

pub const LOCAL_HTTP_RECORD_FILENAME: &str = "local-http.json";
pub const LOCAL_CAPABILITY_HEADER: &str = "X-ATM-Local-Capability";
pub const LOCAL_CAPABILITY_BYTES: usize = 32;

/// Capability presented by local loopback HTTP clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCapability([u8; LOCAL_CAPABILITY_BYTES]);

impl LocalCapability {
    /// Generates a fresh capability using the operating system RNG.
    pub fn generate() -> Result<Self, AtmError> {
        let mut bytes = [0; LOCAL_CAPABILITY_BYTES];
        getrandom::fill(&mut bytes).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to generate local HTTP capability: {source}"
            ))
        })?;
        Ok(Self(bytes))
    }

    pub fn to_base64url(&self) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn parse_base64url(value: &str) -> Result<Self, AtmError> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|source| {
                AtmError::local_http_capability_invalid("local HTTP capability is not base64url")
                    .with_cause(source)
            })?;
        let bytes: [u8; LOCAL_CAPABILITY_BYTES] = bytes.try_into().map_err(|_| {
            AtmError::local_http_capability_invalid(
                "local HTTP capability must decode to exactly 32 bytes",
            )
        })?;
        Ok(Self(bytes))
    }

    pub fn matches_header(&self, value: &str) -> bool {
        match Self::parse_base64url(value) {
            Ok(candidate) => constant_time_eq(&self.0, &candidate.0),
            Err(_) => false,
        }
    }
}

/// Owner-readable local endpoint publication next to the singleton lock.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalHttpEndpointRecord {
    pub schema_version: u8,
    pub daemon_instance_id: Ulid,
    pub ipv4_loopback: Option<SocketAddr>,
    pub ipv6_loopback: Option<SocketAddr>,
    pub capability_base64url: String,
    pub issued_at: IsoTimestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<IsoTimestamp>,
}

impl LocalHttpEndpointRecord {
    pub fn active(
        daemon_instance_id: Ulid,
        ipv4_loopback: Option<SocketAddr>,
        ipv6_loopback: Option<SocketAddr>,
        capability: &LocalCapability,
    ) -> Self {
        Self {
            schema_version: 1,
            daemon_instance_id,
            ipv4_loopback,
            ipv6_loopback,
            capability_base64url: capability.to_base64url(),
            issued_at: IsoTimestamp::now(),
            revoked_at: None,
        }
    }

    pub fn capability(&self) -> Result<LocalCapability, AtmError> {
        if self.schema_version != 1 {
            return Err(AtmError::local_http_endpoint_schema_unsupported(format!(
                "unsupported local HTTP endpoint record schema version {}",
                self.schema_version
            )));
        }
        if self.revoked_at.is_some() {
            return Err(AtmError::local_http_capability_revoked());
        }
        Self::validate_loopback(self.ipv4_loopback)?;
        Self::validate_loopback(self.ipv6_loopback)?;
        if self.ipv4_loopback.is_none() && self.ipv6_loopback.is_none() {
            return Err(AtmError::local_http_endpoint_missing(
                "local HTTP endpoint record has no loopback endpoint",
            ));
        }
        LocalCapability::parse_base64url(&self.capability_base64url)
    }

    /// Marks a published endpoint unavailable before its runtime file is removed.
    pub fn revoke(&mut self) {
        self.revoked_at = Some(IsoTimestamp::now());
    }

    fn validate_loopback(endpoint: Option<SocketAddr>) -> Result<(), AtmError> {
        if let Some(endpoint) = endpoint
            && !endpoint.ip().is_loopback()
        {
            return Err(AtmError::local_http_endpoint_non_loopback(
                "local HTTP endpoint record contains a non-loopback address",
            ));
        }
        Ok(())
    }
}

pub fn local_http_record_path(home_dir: &Path) -> PathBuf {
    crate::home::host_runtime_dir_from_home(home_dir).join(LOCAL_HTTP_RECORD_FILENAME)
}

/// Reads the instance identifier committed by the current singleton owner.
///
/// The owner record is deliberately checked by a local TCP client before it
/// trusts `local-http.json`: a stale runtime record must not connect a client
/// to a successor daemon instance with a different capability.
pub fn owner_instance_id_for_local_http_record(record_path: &Path) -> Result<Ulid, AtmError> {
    let runtime_dir = record_path.parent().ok_or_else(|| {
        AtmError::local_http_runtime_directory_missing(
            "local HTTP endpoint record has no runtime directory",
        )
    })?;
    let lock_path = runtime_dir.join(crate::home::HOST_RUNTIME_OWNER_LOCK_FILE);
    let record = read_owner_record(&lock_path)?;
    let mut fields = record.trim().splitn(3, ':');
    let _pid = fields.next();
    let _token = fields.next();
    let instance = fields.next().ok_or_else(|| {
        AtmError::daemon_unavailable("daemon owner record has no instance identifier")
    })?;
    instance.parse::<Ulid>().map_err(|source| {
        AtmError::daemon_unavailable("daemon owner record has an invalid instance identifier")
            .with_cause(source)
    })
}

fn read_owner_record(lock_path: &Path) -> Result<String, AtmError> {
    match fs::read_to_string(lock_path) {
        Ok(record) if !record.trim().is_empty() => Ok(record),
        Ok(_) => Err(AtmError::daemon_unavailable(
            "daemon owner record is empty while local HTTP metadata is present",
        )),
        Err(source) if should_read_owner_shadow(&source) => read_owner_shadow_record(lock_path),
        Err(source) => Err(AtmError::daemon_unavailable_with_cause(
            "failed to read daemon owner record for local HTTP metadata",
            source,
        )),
    }
}

#[cfg(windows)]
fn should_read_owner_shadow(_source: &std::io::Error) -> bool {
    true
}

#[cfg(not(windows))]
fn should_read_owner_shadow(source: &std::io::Error) -> bool {
    source.kind() == std::io::ErrorKind::NotFound
}

#[cfg(windows)]
fn read_owner_shadow_record(lock_path: &Path) -> Result<String, AtmError> {
    let shadow_path = lock_path.with_file_name(format!(
        "{}.meta",
        lock_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("owner.lock")
    ));
    fs::read_to_string(&shadow_path).map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            "daemon owner record is unavailable while local HTTP metadata is present",
            source,
        )
    })
}

#[cfg(not(windows))]
fn read_owner_shadow_record(_lock_path: &Path) -> Result<String, AtmError> {
    Err(AtmError::daemon_unavailable(
        "daemon owner record is unavailable while local HTTP metadata is present",
    ))
}

fn constant_time_eq(
    left: &[u8; LOCAL_CAPABILITY_BYTES],
    right: &[u8; LOCAL_CAPABILITY_BYTES],
) -> bool {
    let mut difference = 0_u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::{LOCAL_CAPABILITY_BYTES, LocalCapability, LocalHttpEndpointRecord};
    use crate::error::AtmErrorCode;
    use std::net::SocketAddr;
    use ulid::Ulid;

    #[test]
    fn capability_is_base64url_and_exactly_32_bytes() {
        let capability = LocalCapability::generate().expect("capability");
        let encoded = capability.to_base64url();
        assert!(!encoded.contains('='));
        assert!(capability.matches_header(&encoded));
        assert!(!capability.matches_header("invalid"));
        assert_eq!(
            LocalCapability::parse_base64url(&encoded)
                .expect("decode")
                .0
                .len(),
            LOCAL_CAPABILITY_BYTES
        );
    }

    #[test]
    fn record_rejects_non_loopback_endpoint() {
        let capability = LocalCapability::generate().expect("capability");
        let record = LocalHttpEndpointRecord::active(
            Ulid::new(),
            Some("192.168.1.4:43101".parse::<SocketAddr>().expect("address")),
            None,
            &capability,
        );
        assert_eq!(
            record.capability().expect_err("reject non-loopback").code(),
            AtmErrorCode::LocalHttpEndpointNonLoopback
        );
    }

    #[test]
    fn revoked_record_rejects_its_capability() {
        let capability = LocalCapability::generate().expect("capability");
        let mut record = LocalHttpEndpointRecord::active(
            Ulid::new(),
            Some("127.0.0.1:43101".parse::<SocketAddr>().expect("address")),
            None,
            &capability,
        );

        record.revoke();

        assert_eq!(
            record
                .capability()
                .expect_err("reject revoked record")
                .code(),
            AtmErrorCode::LocalHttpCapabilityRevoked
        );
    }

    #[test]
    fn local_http_metadata_failures_have_specific_error_codes() {
        assert_eq!(
            LocalCapability::parse_base64url("not-base64url!")
                .expect_err("reject malformed capability")
                .code(),
            AtmErrorCode::LocalHttpCapabilityInvalid
        );

        let capability = LocalCapability::generate().expect("capability");
        let mut record = LocalHttpEndpointRecord::active(Ulid::new(), None, None, &capability);
        record.schema_version = 2;
        assert_eq!(
            record.capability().expect_err("reject schema").code(),
            AtmErrorCode::LocalHttpEndpointSchemaUnsupported
        );

        let record = LocalHttpEndpointRecord::active(Ulid::new(), None, None, &capability);
        assert_eq!(
            record.capability().expect_err("require endpoint").code(),
            AtmErrorCode::LocalHttpEndpointMissing
        );

        assert_eq!(
            super::owner_instance_id_for_local_http_record(std::path::Path::new(""))
                .expect_err("require record runtime directory")
                .code(),
            AtmErrorCode::LocalHttpRuntimeDirectoryMissing
        );
    }

    #[test]
    fn owner_record_read_preserves_io_cause() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let error =
            super::read_owner_record(tempdir.path()).expect_err("directory is not a lock file");
        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
        assert!(error.cause().is_some());
    }

    #[test]
    fn owner_instance_id_is_read_from_the_runtime_owner_record() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let instance_id = Ulid::new();
        std::fs::write(
            tempdir.path().join("owner.lock"),
            format!("123:owner-token:{instance_id}\n"),
        )
        .expect("write owner record");

        let found =
            super::owner_instance_id_for_local_http_record(&tempdir.path().join("local-http.json"))
                .expect("parse owner instance");

        assert_eq!(found, instance_id);
    }
}
