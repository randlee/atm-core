use std::path::PathBuf;

use atm_core::atm_temp::EnvSource;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_herdr::HerdrClientConfig;
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawHerdrConfig {
    binary_path: Option<PathBuf>,
    socket_path: Option<PathBuf>,
}
#[derive(Deserialize, Default)]
struct RawConfig {
    herdr: Option<RawHerdrConfig>,
}

pub(crate) fn daemon_herdr_client_config(
    env: &dyn EnvSource,
) -> Result<HerdrClientConfig, AtmError> {
    let home = atm_core::home::resolve_user_home_via(env).ok_or_else(|| {
        AtmError::new(
            AtmErrorCode::ConfigParseFailed,
            "cannot resolve home directory for .atm.toml [herdr] configuration",
        )
    })?;
    let path = home.join(".atm.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HerdrClientConfig::default());
        }
        Err(source) => {
            return Err(AtmError::new(
                AtmErrorCode::ConfigParseFailed,
                format!("failed to read {}", path.display()),
            )
            .with_cause(source));
        }
    };
    let raw: RawConfig = toml::from_str(&text).map_err(|error| {
        AtmError::new(
            AtmErrorCode::ConfigParseFailed,
            format!("failed to parse {}: {error}", path.display()),
        )
        .with_cause(error)
    })?;
    let herdr = raw.herdr.unwrap_or_default();
    HerdrClientConfig::try_new(herdr.binary_path, herdr.socket_path).map_err(|error| {
        AtmError::new(
            AtmErrorCode::ConfigParseFailed,
            format!("failed to validate {} [herdr]: {error}", path.display()),
        )
        .with_cause(error)
    })
}

#[cfg(test)]
mod tests {
    use super::daemon_herdr_client_config;
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::test_support::FakeEnvSource;

    fn env_for(home: &tempfile::TempDir) -> FakeEnvSource {
        FakeEnvSource::new([("HOME", Some(home.path().to_str().expect("utf8 path")))])
    }

    #[test]
    fn missing_config_uses_the_valid_default() {
        let home = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            daemon_herdr_client_config(&env_for(&home)).expect("default config"),
            atm_herdr::HerdrClientConfig::default()
        );
    }

    #[test]
    fn reads_valid_absolute_paths_once_from_home_config() {
        let home = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            home.path().join(".atm.toml"),
            "[herdr]\nbinary_path = \"/opt/homebrew/bin/herdr\"\nsocket_path = \"/tmp/herdr.sock\"\n",
        )
        .expect("write config");

        let config = daemon_herdr_client_config(&env_for(&home)).expect("config parses");

        assert_eq!(
            config.binary_path().unwrap().to_str(),
            Some("/opt/homebrew/bin/herdr")
        );
        assert_eq!(
            config.socket_path().unwrap().to_str(),
            Some("/tmp/herdr.sock")
        );
    }

    #[test]
    fn rejects_unresolvable_home_with_config_parse_failed() {
        let error = daemon_herdr_client_config(&FakeEnvSource::empty()).expect_err("must fail");

        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
        assert!(error.detail().contains(".atm.toml [herdr]"));
    }

    #[test]
    fn rejects_unknown_herdr_key_with_file_context() {
        let home = tempfile::tempdir().expect("tempdir");
        let path = home.path().join(".atm.toml");
        std::fs::write(&path, "[herdr]\nunknown = \"value\"\n").expect("write config");

        let error = daemon_herdr_client_config(&env_for(&home)).expect_err("must fail");

        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
        assert!(error.detail().contains(path.to_str().expect("utf8 path")));
        assert!(error.detail().contains("unknown"));
        assert!(error.cause().is_some());
    }

    #[test]
    fn rejects_malformed_toml_with_file_context_and_parser_cause() {
        let home = tempfile::tempdir().expect("tempdir");
        let path = home.path().join(".atm.toml");
        std::fs::write(&path, "[herdr\nbinary_path = \"/opt/herdr\"\n")
            .expect("write malformed config");

        let error = daemon_herdr_client_config(&env_for(&home)).expect_err("must fail");

        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
        assert!(error.detail().contains(path.to_str().expect("utf8 path")));
        assert!(error.cause().is_some());
    }

    #[test]
    fn rejects_relative_path_with_file_and_key_context() {
        let home = tempfile::tempdir().expect("tempdir");
        let path = home.path().join(".atm.toml");
        std::fs::write(&path, "[herdr]\nbinary_path = \"relative/herdr\"\n").expect("write config");

        let error = daemon_herdr_client_config(&env_for(&home)).expect_err("must fail");

        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
        assert!(error.detail().contains(path.to_str().expect("utf8 path")));
        assert!(error.detail().contains("binary_path"));
        assert!(error.cause().is_some());
    }
}
