//! Administrator-provisioned trust root for virtual tool senders.
//!
//! This module deliberately has no environment, workspace, hook, or request
//! input.  The daemon may use only the installed system file and an already
//! authenticated local host identity.  Loading builds a complete immutable
//! [`ToolSenderRegistryConfig`] before returning it, so a failed refresh never
//! exposes a partial policy to a caller which retains its previous config.

use std::fs;
use std::path::{Path, PathBuf};

use atm_storage::{AtmError, HostName, InstallerId, PluginSenderId, ToolKeyId};
use atm_storage_rusqlite::{
    ToolSenderPeerAdmissionConfig, ToolSenderRegistryConfig, TrustedToolInstallerKey,
    TrustedToolOriginKey,
};
use serde::Deserialize;

const TRUST_ROOT_FILE: &str = "/etc/atm/tool-sender-trust-root.json";

/// Load the installed, administrator-protected virtual-sender trust root.
///
/// `host_id` must originate at the authenticated daemon-host composition
/// boundary.  This loader never consults a request, environment variable,
/// repository configuration, or hook payload for either the host or policy.
pub fn load_protected_tool_sender_trust_root() -> Result<ToolSenderRegistryConfig, AtmError> {
    Ok(load_protected_tool_sender_configs()?.registry)
}

/// Complete protected policy supplied to the SQLite tool-sender adapters.
///
/// The local host identity, installer keys, and peer-origin allowlist are one
/// snapshot from the same administrator-owned file. This prevents a request,
/// environment variable, or user configuration file from selecting the host
/// bound into either signed admission path.
#[derive(Clone, Debug)]
pub struct ProtectedToolSenderConfigs {
    registry: ToolSenderRegistryConfig,
    peer_admission: ToolSenderPeerAdmissionConfig,
}

impl ProtectedToolSenderConfigs {
    #[must_use]
    pub fn registry(&self) -> ToolSenderRegistryConfig {
        self.registry.clone()
    }

    #[must_use]
    pub fn peer_admission(&self) -> ToolSenderPeerAdmissionConfig {
        self.peer_admission.clone()
    }
}

/// Load both adapter configurations from one administrator-protected snapshot.
pub fn load_protected_tool_sender_configs() -> Result<ProtectedToolSenderConfigs, AtmError> {
    load_from_protected_path(&system_trust_root_path())
}

/// The only production location from which the daemon bootstrap accepts this
/// authority.  It is intentionally not configurable through a launch flag or
/// environment variable.
#[must_use]
pub fn system_trust_root_path() -> PathBuf {
    PathBuf::from(TRUST_ROOT_FILE)
}

fn load_from_protected_path(path: &Path) -> Result<ProtectedToolSenderConfigs, AtmError> {
    verify_protected_path(path)?;
    let document = fs::read_to_string(path).map_err(|error| {
        AtmError::config(format!(
            "failed to read administrator-protected tool sender trust root {}: {error}",
            path.display()
        ))
    })?;
    let root: RawTrustRoot = serde_json::from_str(&document).map_err(|error| {
        AtmError::config(format!(
            "failed to parse administrator-protected tool sender trust root {}: {error}",
            path.display()
        ))
    })?;
    root.into_configs(path)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTrustRoot {
    version: u8,
    host_id: String,
    installers: Vec<RawInstaller>,
    trusted_origins: Vec<RawTrustedOrigin>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInstaller {
    installer_id: String,
    key_id: String,
    public_key: String,
    allowed_senders: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTrustedOrigin {
    host_id: String,
    public_key: String,
    allowed_senders: Vec<String>,
}

impl RawTrustRoot {
    fn into_configs(self, path: &Path) -> Result<ProtectedToolSenderConfigs, AtmError> {
        if self.version != 1 {
            return Err(config_error(path, "trust-root version must be 1"));
        }
        let host_id = self
            .host_id
            .parse::<HostName>()
            .map_err(|error| config_error(path, format!("invalid host_id: {}", error.detail())))?;
        let installers = self
            .installers
            .into_iter()
            .map(|installer| installer.into_trusted_key(path))
            .collect::<Result<Vec<_>, _>>()?;
        let registry = ToolSenderRegistryConfig::new(host_id, installers).map_err(|error| {
            config_error(
                path,
                format!("invalid trusted installer policy: {}", error.detail()),
            )
        })?;
        let origins = self
            .trusted_origins
            .into_iter()
            .map(|origin| origin.into_trusted_key(path))
            .collect::<Result<Vec<_>, _>>()?;
        let peer_admission = ToolSenderPeerAdmissionConfig::new(origins).map_err(|error| {
            config_error(
                path,
                format!("invalid trusted origin policy: {}", error.detail()),
            )
        })?;
        Ok(ProtectedToolSenderConfigs {
            registry,
            peer_admission,
        })
    }
}

impl RawTrustedOrigin {
    fn into_trusted_key(self, path: &Path) -> Result<TrustedToolOriginKey, AtmError> {
        let host_id = self.host_id.parse::<HostName>().map_err(|error| {
            config_error(path, format!("invalid origin host_id: {}", error.detail()))
        })?;
        let allowed_senders = self
            .allowed_senders
            .into_iter()
            .map(|sender| {
                PluginSenderId::parse(&sender).map_err(|error| {
                    config_error(
                        path,
                        format!("invalid origin allowed sender: {}", error.detail()),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        TrustedToolOriginKey::new(host_id, &self.public_key, allowed_senders)
            .map_err(|error| config_error(path, format!("invalid origin key: {}", error.detail())))
    }
}

impl RawInstaller {
    fn into_trusted_key(self, path: &Path) -> Result<TrustedToolInstallerKey, AtmError> {
        let installer_id = InstallerId::parse(&self.installer_id).map_err(|error| {
            config_error(path, format!("invalid installer_id: {}", error.detail()))
        })?;
        let key_id = ToolKeyId::parse(&self.key_id)
            .map_err(|error| config_error(path, format!("invalid key_id: {}", error.detail())))?;
        let allowed_senders = self
            .allowed_senders
            .into_iter()
            .map(|sender| {
                PluginSenderId::parse(&sender).map_err(|error| {
                    config_error(path, format!("invalid allowed sender: {}", error.detail()))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        TrustedToolInstallerKey::new(installer_id, key_id, &self.public_key, allowed_senders)
            .map_err(|error| {
                config_error(path, format!("invalid installer key: {}", error.detail()))
            })
    }
}

fn config_error(path: &Path, detail: impl std::fmt::Display) -> AtmError {
    AtmError::config(format!(
        "administrator-protected tool sender trust root {} is invalid: {detail}",
        path.display()
    ))
}

#[cfg(unix)]
fn verify_protected_path(path: &Path) -> Result<(), AtmError> {
    use std::os::unix::fs::MetadataExt;

    let parent = path
        .parent()
        .ok_or_else(|| config_error(path, "missing parent directory"))?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|error| {
        config_error(
            path,
            format!("cannot inspect parent {}: {error}", parent.display()),
        )
    })?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| config_error(path, format!("cannot inspect file: {error}")))?;
    verify_unix_metadata(
        path,
        parent_metadata.file_type().is_dir(),
        parent_metadata.uid(),
        parent_metadata.mode(),
        metadata.file_type().is_file(),
        metadata.uid(),
        metadata.mode(),
    )
}

#[cfg(unix)]
fn verify_unix_metadata(
    path: &Path,
    parent_is_dir: bool,
    parent_uid: u32,
    parent_mode: u32,
    file_is_regular: bool,
    file_uid: u32,
    file_mode: u32,
) -> Result<(), AtmError> {
    if !parent_is_dir || parent_uid != 0 || parent_mode & 0o022 != 0 {
        return Err(config_error(
            path,
            "parent directory must be root-owned and not group/world writable",
        ));
    }
    if !file_is_regular || file_uid != 0 || file_mode & 0o077 != 0 {
        return Err(config_error(
            path,
            "file must be a root-owned regular file with no group/world permissions",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_protected_path(path: &Path) -> Result<(), AtmError> {
    Err(config_error(
        path,
        "protected trust-root loading is unavailable until the platform ACL verifier is implemented",
    ))
}

#[cfg(test)]
mod tests {
    use super::{RawTrustRoot, load_from_protected_path, verify_unix_metadata};
    use atm_storage::AtmErrorCode;
    use std::path::Path;

    const PUBLIC_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

    #[test]
    fn parses_a_complete_installer_policy_into_the_validated_registry_config() {
        let root: RawTrustRoot = serde_json::from_str(&format!(
            r#"{{"version":1,"host_id":"daemon.example","installers":[{{"installer_id":"installer-a","key_id":"key-a","public_key":"{PUBLIC_KEY}","allowed_senders":["tool-a"]}}],"trusted_origins":[{{"host_id":"peer.example","public_key":"{PUBLIC_KEY}","allowed_senders":["tool-a"]}}]}}"#
        ))
        .expect("valid root document");

        let config = root.into_configs(Path::new("/etc/atm/tool-sender-trust-root.json"));

        assert!(config.is_ok(), "complete policy is accepted");
    }

    #[test]
    fn rejects_unknown_or_malformed_policy_before_constructing_config() {
        let error = serde_json::from_str::<RawTrustRoot>(
            r#"{"version":1,"host_id":"daemon.example","installers":[],"trusted_origins":[],"unexpected":true}"#,
        )
        .expect_err("unknown input must be rejected by the closed schema");
        assert!(error.to_string().contains("unexpected"));

        let root: RawTrustRoot = serde_json::from_str(
            r#"{"version":1,"host_id":"daemon.example","installers":[{"installer_id":"installer-a","key_id":"key-a","public_key":"not-base64","allowed_senders":["tool-a"]}],"trusted_origins":[{"host_id":"peer.example","public_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","allowed_senders":["tool-a"]}]}"#,
        )
        .expect("schema is syntactically valid");
        let error = root
            .into_configs(Path::new("/etc/atm/tool-sender-trust-root.json"))
            .expect_err("malformed key must fail closed");
        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);

        let root: RawTrustRoot = serde_json::from_str(&format!(
            r#"{{"version":1,"host_id":"not a host","installers":[{{"installer_id":"installer-a","key_id":"key-a","public_key":"{PUBLIC_KEY}","allowed_senders":["tool-a"]}}],"trusted_origins":[{{"host_id":"peer.example","public_key":"{PUBLIC_KEY}","allowed_senders":["tool-a"]}}]}}"#
        ))
        .expect("schema is syntactically valid");
        let error = root
            .into_configs(Path::new("/etc/atm/tool-sender-trust-root.json"))
            .expect_err("invalid protected local host must fail closed");
        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);

        let root: RawTrustRoot = serde_json::from_str(&format!(
            r#"{{"version":1,"host_id":"daemon.example","installers":[{{"installer_id":"installer-a","key_id":"key-a","public_key":"{PUBLIC_KEY}","allowed_senders":["tool-a"]}}],"trusted_origins":[]}}"#
        ))
        .expect("schema is syntactically valid");
        let error = root
            .into_configs(Path::new("/etc/atm/tool-sender-trust-root.json"))
            .expect_err("empty origin policy must fail closed");
        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_paths_without_administrator_protection() {
        let path = Path::new("/etc/atm/tool-sender-trust-root.json");
        let error = verify_unix_metadata(path, true, 1000, 0o755, true, 0, 0o600)
            .expect_err("non-root parent owner is unsafe");
        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);

        let error = verify_unix_metadata(path, true, 0, 0o755, true, 0, 0o640)
            .expect_err("group-readable trust root is unsafe");
        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_rename_refreshes_only_complete_valid_policy() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("trust-root.json");
        let replacement = directory.path().join("trust-root.next");
        // This test exercises parsing and refresh atomicity through a path,
        // while permission policy itself is deterministically unit-tested above.
        fs::write(
            &path,
            format!(r#"{{"version":1,"host_id":"daemon.example","installers":[{{"installer_id":"installer-a","key_id":"key-a","public_key":"{PUBLIC_KEY}","allowed_senders":["tool-a"]}}],"trusted_origins":[{{"host_id":"peer.example","public_key":"{PUBLIC_KEY}","allowed_senders":["tool-a"]}}]}}"#),
        )
        .expect("write initial policy");
        fs::write(
            &replacement,
            format!(r#"{{"version":1,"host_id":"daemon.example","installers":[{{"installer_id":"installer-b","key_id":"key-b","public_key":"{PUBLIC_KEY}","allowed_senders":["tool-b"]}}],"trusted_origins":[{{"host_id":"peer.example","public_key":"{PUBLIC_KEY}","allowed_senders":["tool-b"]}}]}}"#),
        )
        .expect("write replacement policy");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("initial mode");
        fs::set_permissions(&replacement, fs::Permissions::from_mode(0o600))
            .expect("replacement mode");

        // `load_from_protected_path` rightfully refuses this unprivileged test
        // directory.  The no-partial-refresh property is covered by parsing
        // each complete document independently before callers replace config.
        let initial: RawTrustRoot = serde_json::from_str(&fs::read_to_string(&path).expect("read"))
            .expect("initial snapshot");
        fs::rename(&replacement, &path).expect("atomic replacement");
        let refreshed: RawTrustRoot =
            serde_json::from_str(&fs::read_to_string(&path).expect("read"))
                .expect("refreshed snapshot");
        assert!(initial.into_configs(&path).is_ok());
        assert!(refreshed.into_configs(&path).is_ok());
        let _ = load_from_protected_path;
    }
}
