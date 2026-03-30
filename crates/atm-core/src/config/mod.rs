pub mod aliases;
pub mod bridge;
pub mod discovery;
pub mod types;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub use types::AtmConfig;

use crate::error::{AtmError, AtmErrorKind};

pub fn load_config(start_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
    let Some(path) = find_config_path(start_dir) else {
        return Ok(None);
    };

    let contents = fs::read_to_string(&path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!("failed to read config at {}: {error}", path.display()),
        )
        .with_source(error)
    })?;
    let parsed = toml::from_str(&contents).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!("failed to parse config at {}: {error}", path.display()),
        )
        .with_source(error)
    })?;
    Ok(Some(parsed))
}

pub fn resolve_identity(config: Option<&AtmConfig>) -> Option<String> {
    env::var("ATM_IDENTITY")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| config.and_then(|cfg| cfg.identity.clone()))
}

pub fn resolve_team(team_override: Option<&str>, config: Option<&AtmConfig>) -> Option<String> {
    team_override
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| env::var("ATM_TEAM").ok().filter(|value| !value.is_empty()))
        .or_else(|| config.and_then(|cfg| cfg.default_team.clone()))
}

fn find_config_path(start_dir: &Path) -> Option<PathBuf> {
    let mut current = Some(start_dir);

    while let Some(dir) = current {
        let candidate = dir.join(".atm.toml");
        if candidate.is_file() {
            return Some(candidate);
        }

        current = dir.parent();
    }

    None
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    use super::{load_config, resolve_identity, resolve_team, AtmConfig};

    #[test]
    fn load_config_walks_upward_for_dot_atm_toml() {
        let root = unique_temp_dir("config-discovery");
        let nested = root.join("workspace").join("nested");
        fs::create_dir_all(&nested).expect("nested dir");
        fs::write(
            root.join(".atm.toml"),
            "identity = \"arch-ctm\"\ndefault_team = \"atm-dev\"\n",
        )
        .expect("config");

        let config = load_config(&nested).expect("config").expect("present");
        assert_eq!(config.identity.as_deref(), Some("arch-ctm"));
        assert_eq!(config.default_team.as_deref(), Some("atm-dev"));
    }

    #[test]
    #[serial_test::serial]
    fn identity_prefers_environment_over_config() {
        let original_identity = env::var_os("ATM_IDENTITY");
        env::set_var("ATM_IDENTITY", "env-identity");

        let config = AtmConfig {
            identity: Some("config-identity".into()),
            default_team: None,
        };

        assert_eq!(
            resolve_identity(Some(&config)).as_deref(),
            Some("env-identity")
        );
        restore("ATM_IDENTITY", original_identity);
    }

    #[test]
    #[serial_test::serial]
    fn team_resolution_prefers_flag_then_env_then_config() {
        let original_team = env::var_os("ATM_TEAM");
        env::set_var("ATM_TEAM", "env-team");

        let config = AtmConfig {
            identity: None,
            default_team: Some("config-team".into()),
        };

        assert_eq!(
            resolve_team(Some("flag-team"), Some(&config)).as_deref(),
            Some("flag-team")
        );
        assert_eq!(
            resolve_team(None, Some(&config)).as_deref(),
            Some("env-team")
        );

        env::remove_var("ATM_TEAM");
        assert_eq!(
            resolve_team(None, Some(&config)).as_deref(),
            Some("config-team")
        );

        restore("ATM_TEAM", original_team);
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let path = env::temp_dir().join(format!("{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn restore(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => env::set_var(key, value),
            None => env::remove_var(key),
        }
    }
}
