use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::delivery_channel::HerdrSession;
use crate::error::AtmError;
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
#[serde(try_from = "String", into = "String")]
pub struct HerdrEndpointDisplay(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HerdrEndpointDisplayRoot {
    XdgConfigHome,
    Home,
    AppData,
    Configured,
}

impl HerdrEndpointDisplay {
    /// Converts a transport-owned endpoint into a symbolic display path.
    ///
    /// Only normal, relative components cross into the core DTO. This keeps
    /// the user's actual home, config, and socket roots out of doctor output.
    pub fn from_relative(
        root: HerdrEndpointDisplayRoot,
        relative: &Path,
    ) -> Result<Self, AtmError> {
        let components = relative
            .components()
            .map(|component| match component {
                Component::Normal(component) => {
                    component.to_str().map(str::to_owned).ok_or_else(|| {
                        AtmError::new(
                            AtmErrorCode::ConfigParseFailed,
                            "Herdr endpoint contains a non-Unicode component",
                        )
                    })
                }
                _ => Err(AtmError::new(
                    AtmErrorCode::ConfigParseFailed,
                    "Herdr endpoint must contain only relative normal components",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if components.is_empty() {
            return Err(AtmError::new(
                AtmErrorCode::ConfigParseFailed,
                "Herdr endpoint must not be empty",
            ));
        }

        let path = components.join("/");
        let value = match root {
            HerdrEndpointDisplayRoot::XdgConfigHome => format!("$XDG_CONFIG_HOME/{path}"),
            HerdrEndpointDisplayRoot::Home => format!("$HOME/{path}"),
            HerdrEndpointDisplayRoot::AppData => format!("%APPDATA%/{path}"),
            HerdrEndpointDisplayRoot::Configured => {
                format!(
                    "<configured>/{}",
                    components.last().expect("nonempty components")
                )
            }
        };
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for HerdrEndpointDisplay {
    type Error = AtmError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let (prefix, relative) = value
            .split_once('/')
            .ok_or_else(|| invalid_endpoint_display(&value))?;
        if !matches!(
            prefix,
            "$XDG_CONFIG_HOME" | "$HOME" | "%APPDATA%" | "<configured>"
        ) || relative.is_empty()
            || relative.split('/').any(|component| {
                component.is_empty()
                    || component == "."
                    || component == ".."
                    || component.contains('\\')
            })
        {
            return Err(invalid_endpoint_display(&value));
        }
        Ok(Self(value))
    }
}

impl From<HerdrEndpointDisplay> for String {
    fn from(value: HerdrEndpointDisplay) -> Self {
        value.0
    }
}

fn invalid_endpoint_display(value: &str) -> AtmError {
    AtmError::new(
        AtmErrorCode::ConfigParseFailed,
        format!("Herdr endpoint display is not a supported symbolic path: {value}"),
    )
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
#[serde(try_from = "String", into = "String")]
pub struct HerdrVersion(String);

impl HerdrVersion {
    pub fn parse(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        let version = semver::Version::parse(&value).map_err(|_| {
            AtmError::new(
                AtmErrorCode::HerdrUnavailable,
                "Herdr returned an invalid semantic version",
            )
        })?;
        Ok(Self(version.to_string()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for HerdrVersion {
    type Error = AtmError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<HerdrVersion> for String {
    fn from(value: HerdrVersion) -> Self {
        value.0
    }
}

impl fmt::Display for HerdrVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{HerdrEndpointDisplay, HerdrEndpointDisplayRoot, HerdrVersion};
    use crate::error_codes::AtmErrorCode;

    #[test]
    fn endpoint_display_redacts_roots_and_keeps_only_normal_components() {
        let home = HerdrEndpointDisplay::from_relative(
            HerdrEndpointDisplayRoot::Home,
            Path::new(".config/herdr/socket"),
        )
        .expect("relative path");
        let configured = HerdrEndpointDisplay::from_relative(
            HerdrEndpointDisplayRoot::Configured,
            Path::new("private/nested/socket"),
        )
        .expect("relative path");

        assert_eq!(home.as_str(), "$HOME/.config/herdr/socket");
        assert_eq!(configured.as_str(), "<configured>/socket");
        for invalid in [
            Path::new("/private/socket"),
            Path::new("../socket"),
            Path::new("."),
        ] {
            assert_eq!(
                HerdrEndpointDisplay::from_relative(HerdrEndpointDisplayRoot::Home, invalid)
                    .expect_err("non-normal path must fail")
                    .code(),
                AtmErrorCode::ConfigParseFailed
            );
        }
    }

    #[test]
    fn endpoint_display_deserialization_rejects_raw_and_traversing_paths() {
        let valid: HerdrEndpointDisplay =
            serde_json::from_str(r#""$XDG_CONFIG_HOME/herdr/socket""#).expect("symbolic path");
        assert_eq!(valid.as_str(), "$XDG_CONFIG_HOME/herdr/socket");
        for invalid in [
            r#""/Users/rand/.config/herdr/socket""#,
            r#""$HOME/../socket""#,
        ] {
            assert!(serde_json::from_str::<HerdrEndpointDisplay>(invalid).is_err());
        }
    }

    #[test]
    fn version_is_semver_validated_for_construction_and_deserialization() {
        let version = HerdrVersion::parse("0.8.2-alpha.1").expect("semantic version");
        assert_eq!(version.to_string(), "0.8.2-alpha.1");
        assert!(HerdrVersion::parse("version eight").is_err());
        assert!(serde_json::from_str::<HerdrVersion>(r#""not-a-version""#).is_err());
    }
}
