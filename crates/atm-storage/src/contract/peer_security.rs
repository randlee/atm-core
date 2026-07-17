use super::{AllowedHostName, IsoTimestamp, require_non_blank};
use crate::error::AtmError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PeerSecurityMode {
    SecureRequired,
    InsecureAllowed,
}

impl PeerSecurityMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SecureRequired => "secure-required",
            Self::InsecureAllowed => "insecure-allowed",
        }
    }
}

impl fmt::Display for PeerSecurityMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PeerSecurityMode {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "secure-required" => Ok(Self::SecureRequired),
            "insecure-allowed" => Ok(Self::InsecureAllowed),
            other => Err(AtmError::validation(format!(
                "unsupported peer security mode `{other}`"
            ))
            .with_recovery(
                "Use either secure-required or insecure-allowed before retrying the daemon security command.",
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerSecuritySettingsRow {
    pub mode: PeerSecurityMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<IsoTimestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SetPeerSecurityModeCommand {
    pub mode: PeerSecurityMode,
    pub updated_by: String,
}

impl SetPeerSecurityModeCommand {
    pub fn new(mode: PeerSecurityMode, updated_by: impl Into<String>) -> Result<Self, AtmError> {
        let updated_by = require_non_blank(
            updated_by.into(),
            "peer security updated_by",
            "Populate the caller identity before changing daemon peer security mode.",
        )?;
        Ok(Self { mode, updated_by })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalPeerIdentityRow {
    certificate_der: Vec<u8>,
    private_key_der: Vec<u8>,
    fingerprint_sha256: String,
    created_at: IsoTimestamp,
    updated_at: IsoTimestamp,
}

impl LocalPeerIdentityRow {
    pub fn new(
        certificate_der: Vec<u8>,
        private_key_der: Vec<u8>,
        fingerprint_sha256: impl Into<String>,
        created_at: IsoTimestamp,
        updated_at: IsoTimestamp,
    ) -> Result<Self, AtmError> {
        if certificate_der.is_empty() {
            return Err(AtmError::validation(
                "daemon local peer identity certificate_der must not be empty".to_string(),
            )
            .with_recovery(
                "Regenerate the local daemon peer identity before retrying the secure transport operation.",
            ));
        }
        if private_key_der.is_empty() {
            return Err(AtmError::validation(
                "daemon local peer identity private_key_der must not be empty".to_string(),
            )
            .with_recovery(
                "Regenerate the local daemon peer identity before retrying the secure transport operation.",
            ));
        }
        let fingerprint_sha256 = super::normalize_sha256_fingerprint(fingerprint_sha256.into())?;
        Ok(Self {
            certificate_der,
            private_key_der,
            fingerprint_sha256,
            created_at,
            updated_at,
        })
    }

    pub fn certificate_der(&self) -> &[u8] {
        &self.certificate_der
    }

    pub fn private_key_der(&self) -> &[u8] {
        &self.private_key_der
    }

    pub fn fingerprint_sha256(&self) -> &str {
        &self.fingerprint_sha256
    }

    pub fn created_at(&self) -> &IsoTimestamp {
        &self.created_at
    }

    pub fn updated_at(&self) -> &IsoTimestamp {
        &self.updated_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedPeerRow {
    host_name: AllowedHostName,
    fingerprint_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
    approved_by: String,
    approved_at: IsoTimestamp,
    updated_at: IsoTimestamp,
}

impl TrustedPeerRow {
    pub fn new(
        host_name: AllowedHostName,
        fingerprint_sha256: impl Into<String>,
        display_name: Option<String>,
        approved_by: impl Into<String>,
        approved_at: IsoTimestamp,
        updated_at: IsoTimestamp,
    ) -> Result<Self, AtmError> {
        let fingerprint_sha256 = super::normalize_sha256_fingerprint(fingerprint_sha256.into())?;
        let approved_by = require_non_blank(
            approved_by.into(),
            "trusted peer approved_by",
            "Populate the caller identity before approving daemon peer trust.",
        )?;
        Ok(Self {
            host_name,
            fingerprint_sha256,
            display_name: display_name.and_then(|value| {
                let trimmed = value.trim().to_string();
                (!trimmed.is_empty()).then_some(trimmed)
            }),
            approved_by,
            approved_at,
            updated_at,
        })
    }

    pub fn host_name(&self) -> &AllowedHostName {
        &self.host_name
    }

    pub fn fingerprint_sha256(&self) -> &str {
        &self.fingerprint_sha256
    }

    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    pub fn approved_by(&self) -> &str {
        &self.approved_by
    }

    pub fn approved_at(&self) -> &IsoTimestamp {
        &self.approved_at
    }

    pub fn updated_at(&self) -> &IsoTimestamp {
        &self.updated_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpsertTrustedPeerCommand {
    host_name: AllowedHostName,
    fingerprint_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
    approved_by: String,
}

impl UpsertTrustedPeerCommand {
    pub fn new(
        host_name: impl Into<String>,
        fingerprint_sha256: impl Into<String>,
        display_name: Option<String>,
        approved_by: impl Into<String>,
    ) -> Result<Self, AtmError> {
        let host_name = AllowedHostName::new(host_name.into())?;
        let fingerprint_sha256 = super::normalize_sha256_fingerprint(fingerprint_sha256.into())?;
        let approved_by = require_non_blank(
            approved_by.into(),
            "trusted peer approved_by",
            "Populate the caller identity before approving daemon peer trust.",
        )?;
        Ok(Self {
            host_name,
            fingerprint_sha256,
            display_name: display_name.and_then(|value| {
                let trimmed = value.trim().to_string();
                (!trimmed.is_empty()).then_some(trimmed)
            }),
            approved_by,
        })
    }

    pub fn host_name(&self) -> &AllowedHostName {
        &self.host_name
    }

    pub fn fingerprint_sha256(&self) -> &str {
        &self.fingerprint_sha256
    }

    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    pub fn approved_by(&self) -> &str {
        &self.approved_by
    }
}
