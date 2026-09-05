//! Graft receiver and peer-control-plane storage contracts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::SocketAddr;
use std::num::NonZeroU16;
use std::str::FromStr;

use crate::contract::{require_non_blank, sealed};
use crate::error::AtmError;
use crate::types::{AgentName, HostName, LocalCapability, OwnerGeneration, TeamName};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftReceiverRegistration {
    pub team: TeamName,
    pub agent: AgentName,
    pub endpoint: SocketAddr,
    pub capability: LocalCapability,
    pub owner_generation: OwnerGeneration,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraftEndpointStoreError {
    AlreadyActive,
    NotOwner,
    Absent,
    Storage {
        code: crate::error_codes::AtmErrorCode,
        message: String,
        cause: Option<String>,
    },
}
impl GraftEndpointStoreError {
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
                cause: Some(cause),
            } => write!(
                formatter,
                "graft receiver endpoint storage failed ({code}): {message}: {cause}"
            ),
            Self::Storage {
                code,
                message,
                cause: None,
            } => write!(
                formatter,
                "graft receiver endpoint storage failed ({code}): {message}"
            ),
        }
    }
}

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
    fn mark_unreachable(
        &self,
        team: &TeamName,
        agent: &AgentName,
        owner_generation: &OwnerGeneration,
        now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError>;
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpsInterface {
    pub bind_addr: SocketAddr,
    pub advertise_host: HostName,
    pub enabled: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalCertificate {
    pub fingerprint: CertificateFingerprint,
    pub private_key_ref: PrivateKeyRef,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedPeer {
    pub host: HostName,
    pub fingerprint: CertificateFingerprint,
    pub enabled: bool,
    pub https_port: NonZeroU16,
}

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
