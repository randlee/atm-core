use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::delivery_channel::HerdrSession;
use crate::error_codes::AtmErrorCode;
use crate::types::AgentName;

use super::DoctorFinding;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrTransportKind {
    Cli,
    Socket,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrEndpointProvenance {
    Session,
    SocketPath,
    HerdrDefault,
}

/// A sanitized endpoint suitable for doctor output. Raw filesystem paths do
/// not cross the Herdr boundary into core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HerdrEndpointDisplay(String);

impl HerdrEndpointDisplay {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Constructed only by the atm-herdr endpoint sanitizer.
    #[doc(hidden)]
    pub fn sanitized(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrBinaryProvenance {
    Configured,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrBinaryResolution {
    pub path: PathBuf,
    pub provenance: HerdrBinaryProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HerdrPresenceOutcome {
    Visible,
    Finding { finding: DoctorFinding },
    Infrastructure { code: AtmErrorCode, detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HerdrVersion(String);

impl HerdrVersion {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The Herdr transport owns semantic-version validation.
    #[doc(hidden)]
    pub fn parsed(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrRosterMember {
    pub ordinal: usize,
    pub name: AgentName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrMemberPresence {
    #[serde(skip)]
    pub ordinal: usize,
    pub name: AgentName,
    pub outcome: HerdrPresenceOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HerdrDoctorState {
    Ok {
        version: HerdrVersion,
        protocol: u32,
    },
    NotConfigured,
    BinaryNotFound {
        searched: Vec<PathBuf>,
    },
    BinaryNotExecutable {
        path: PathBuf,
        cause: String,
    },
    BelowMinimum {
        version: HerdrVersion,
        minimum: HerdrVersion,
    },
    ServerNotRunning {
        endpoint_named_by_herdr: Option<HerdrEndpointDisplay>,
    },
    ClientServerMismatch {
        client: Option<HerdrVersion>,
        server: Option<HerdrVersion>,
    },
    EndpointUnreachable {
        endpoint: HerdrEndpointDisplay,
    },
    PermissionDenied {
        endpoint: HerdrEndpointDisplay,
    },
    ProbeTimedOut {
        #[serde(with = "duration_millis")]
        after: Duration,
    },
    UnexpectedResponse {
        code: Option<String>,
        detail: String,
    },
    Other {
        code: AtmErrorCode,
        detail: String,
    },
}

impl HerdrDoctorState {
    #[must_use]
    pub const fn remedy(&self) -> &'static str {
        match self {
            Self::Ok { .. } => "none",
            Self::NotConfigured => "Configure Herdr only if desired",
            Self::BinaryNotFound { .. } => "Install Herdr or correct binary_path",
            Self::BinaryNotExecutable { .. } => "Correct permissions or path",
            Self::BelowMinimum { .. } => "Upgrade Herdr",
            Self::ServerNotRunning { .. } => "Start the endpoint as the same user",
            Self::ClientServerMismatch { .. } => "Use Herdr handoff coordination",
            Self::EndpointUnreachable { .. } => "Start Herdr or correct endpoint config",
            Self::PermissionDenied { .. } => "Align per-user ownership or permissions",
            Self::ProbeTimedOut { .. } => "Inspect Herdr health and retry",
            Self::UnexpectedResponse { .. } => "Verify supported Herdr and capture response",
            Self::Other { .. } => "Follow detail and file a compatibility finding",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrEndpointObservation {
    pub session: Option<HerdrSession>,
    pub provenance: HerdrEndpointProvenance,
    pub transport: HerdrTransportKind,
    pub endpoint: Option<HerdrEndpointDisplay>,
    pub binary: Option<HerdrBinaryResolution>,
    pub state: HerdrDoctorState,
    pub live_handoff: Option<bool>,
    pub members: Vec<HerdrMemberPresence>,
}

mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;
    pub fn serialize<S: Serializer>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u128(value.as_millis())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_millis(u64::deserialize(deserializer)?))
    }
}
