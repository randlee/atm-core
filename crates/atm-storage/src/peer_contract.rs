//! Graft receiver and peer-control-plane storage contracts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::SocketAddr;
use std::num::NonZeroU16;
use std::str::FromStr;
use std::time::Duration;

use crate::contract::{require_non_blank, sealed};
use crate::error::AtmError;
use crate::types::{AgentName, HostName, LocalCapability, OwnerGeneration, TeamName};

/// Registration payload for one loopback graft receiver lease.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftReceiverRegistration {
    pub team: TeamName,
    pub agent: AgentName,
    pub endpoint: SocketAddr,
    pub capability: LocalCapability,
    pub owner_generation: OwnerGeneration,
}

/// Durable graft receiver endpoint and its liveness observations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftReceiverLease {
    pub endpoint: SocketAddr,
    pub capability: LocalCapability,
    pub owner_generation: OwnerGeneration,
    pub registered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreachable_since: Option<DateTime<Utc>>,
}

/// Errors returned by the durable graft receiver endpoint store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraftEndpointStoreError {
    /// Reserved for a future caller that cannot prove same-host exclusivity
    /// (ADR-056): same-host callers instead prove exclusivity through the
    /// receiver's flock and are never rejected by this variant today.
    AlreadyActive,
    /// The supplied owner generation does not own the stored lease.
    NotOwner,
    /// No lease row exists for this receiver.
    Absent,
    /// The backend could not complete the requested operation.
    ///
    /// Preserves the originating [`AtmError`]'s code and cause chain (RBP-F001)
    /// instead of flattening it into an opaque string, so callers can
    /// distinguish e.g. a caller-input constraint violation from a true
    /// backend outage rather than collapsing every failure into one generic
    /// presentation.
    Storage {
        code: crate::error_codes::AtmErrorCode,
        message: String,
        cause: Option<String>,
    },
}

impl GraftEndpointStoreError {
    /// Wraps a structured backend [`AtmError`] as a [`Self::Storage`]
    /// variant, preserving its code and cause instead of flattening it into
    /// an opaque string.
    #[must_use]
    pub fn storage(error: &AtmError) -> Self {
        Self::Storage {
            code: error.code(),
            message: error.message().to_string(),
            cause: error.cause().map(ToOwned::to_owned),
        }
    }
}

impl fmt::Display for GraftEndpointStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyActive => formatter.write_str("graft receiver lease is already active"),
            Self::NotOwner => {
                formatter.write_str("graft receiver lease is owned by another generation")
            }
            Self::Absent => formatter.write_str("graft receiver lease is absent"),
            Self::Storage {
                code,
                message,
                cause,
            } => match cause {
                Some(cause) => write!(
                    formatter,
                    "graft receiver endpoint storage failed ({code}): {message}: {cause}"
                ),
                None => write!(
                    formatter,
                    "graft receiver endpoint storage failed ({code}): {message}"
                ),
            },
        }
    }
}

/// Durable registry for same-host graft receiver endpoints.
pub trait GraftReceiverEndpointStore: sealed::Sealed + Send + Sync {
    fn register(
        &self,
        registration: &GraftReceiverRegistration,
        now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError>;

    fn refresh(
        &self,
        team: &TeamName,
        agent: &AgentName,
        owner_generation: &OwnerGeneration,
        now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError>;

    fn unregister(
        &self,
        team: &TeamName,
        agent: &AgentName,
        owner_generation: &OwnerGeneration,
    ) -> Result<(), GraftEndpointStoreError>;

    fn lookup(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<GraftReceiverLease>, GraftEndpointStoreError>;

    /// Performs one bounded durable lease lookup. Implementations that own a
    /// reader pool must apply `deadline` to the submitted read.
    fn lookup_with_deadline(
        &self,
        team: &TeamName,
        agent: &AgentName,
        _deadline: Duration,
    ) -> Result<Option<GraftReceiverLease>, GraftEndpointStoreError> {
        self.lookup(team, agent)
    }

    fn mark_unreachable(
        &self,
        team: &TeamName,
        agent: &AgentName,
        owner_generation: &OwnerGeneration,
        now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError>;
}

/// Tokio-safe counterpart to [`GraftReceiverEndpointStore`] lease lookups.
///
/// The synchronous trait remains the compatibility surface for non-Tokio
/// callers. HTTP request paths use this companion so a SQLite backend can
/// await its bounded reader lane directly instead of consuming a Tokio
/// blocking worker merely to wait for that lane.
#[async_trait::async_trait]
pub trait AsyncGraftReceiverEndpointStore: GraftReceiverEndpointStore {
    /// Looks up one receiver lease through the backend-owned asynchronous
    /// reader lane.
    async fn lookup_with_deadline_async(
        &self,
        _team: &TeamName,
        _agent: &AgentName,
        _deadline: Duration,
    ) -> Result<Option<GraftReceiverLease>, GraftEndpointStoreError> {
        let error = AtmError::daemon_unavailable(
            "graft receiver endpoint store does not implement async lease lookup",
        );
        Err(GraftEndpointStoreError::storage(&error))
    }
}

/// A non-empty, opaque certificate fingerprint. It cannot be confused with a
/// private-key reference at storage and transport boundaries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct CertificateFingerprint(String);

impl CertificateFingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CertificateFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CertificateFingerprint {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        require_non_blank(value.to_owned(), "certificate fingerprint").map(Self)
    }
}

impl TryFrom<String> for CertificateFingerprint {
    type Error = AtmError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<CertificateFingerprint> for String {
    fn from(value: CertificateFingerprint) -> Self {
        value.0
    }
}

/// A non-empty opaque reference to locally held private-key material.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct PrivateKeyRef(String);

impl PrivateKeyRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PrivateKeyRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for PrivateKeyRef {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        require_non_blank(value.to_owned(), "certificate key reference").map(Self)
    }
}

impl TryFrom<String> for PrivateKeyRef {
    type Error = AtmError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<PrivateKeyRef> for String {
    fn from(value: PrivateKeyRef) -> Self {
        value.0
    }
}

/// One durable HTTPS listener configuration. This is control-plane state only;
/// it contains no delivery, retry, or mailbox data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpsInterface {
    pub bind_addr: SocketAddr,
    pub advertise_host: HostName,
    pub enabled: bool,
}

/// Public identity of the local TLS certificate. The private key is referenced
/// indirectly so doctor and callers cannot read secret material from storage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalCertificate {
    pub fingerprint: CertificateFingerprint,
    pub private_key_ref: PrivateKeyRef,
}

/// One exact, pinned peer allowed to use the cross-host HTTPS listener.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedPeer {
    pub host: HostName,
    pub fingerprint: CertificateFingerprint,
    pub enabled: bool,
    pub https_port: NonZeroU16,
}

/// Backend-neutral durable cross-host configuration.
///
/// This boundary deliberately excludes transport state, retries, receipts,
/// and mailbox state. HTTPS adapters consume this contract but never SQLite
/// implementation types.
pub trait PeerConfigStore: sealed::Sealed + Send + Sync {
    fn list_interfaces(&self) -> Result<Vec<HttpsInterface>, AtmError>;
    fn save_interface(&self, interface: &HttpsInterface) -> Result<(), AtmError>;
    fn remove_interface(&self, bind_addr: SocketAddr) -> Result<bool, AtmError>;
    fn local_certificate(&self) -> Result<Option<LocalCertificate>, AtmError>;
    fn save_local_certificate(&self, certificate: &LocalCertificate) -> Result<(), AtmError>;
    fn list_trusted_peers(&self) -> Result<Vec<TrustedPeer>, AtmError>;
    fn trusted_peer(&self, host: &HostName) -> Result<Option<TrustedPeer>, AtmError>;
    fn save_trusted_peer(&self, peer: &TrustedPeer) -> Result<(), AtmError>;
    fn remove_trusted_peer(&self, host: &HostName) -> Result<bool, AtmError>;
}
