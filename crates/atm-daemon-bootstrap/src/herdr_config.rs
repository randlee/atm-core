use std::path::PathBuf;

use atm_core::atm_temp::EnvSource;
use atm_core::doctor::HerdrTransportKind;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_herdr::HerdrClientConfig;
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawHerdrConfig {
    transport: Option<HerdrTransportKind>,
    binary_path: Option<PathBuf>,
    socket_path: Option<PathBuf>,
}
#[derive(Deserialize, Default)]
struct RawConfig {
    herdr: Option<RawHerdrConfig>,
}

/// Single-read bootstrap configuration for the validated Herdr client and
/// client configuration.
#[derive(Clone, Debug, Default)]
pub(crate) struct DaemonHerdrConfig {
    pub(crate) client: HerdrClientConfig,
}

pub(crate) fn daemon_herdr_config(env: &dyn EnvSource) -> Result<DaemonHerdrConfig, AtmError> {
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
            return Ok(DaemonHerdrConfig::default());
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
    let client = HerdrClientConfig::try_new(
        herdr.transport.unwrap_or(HerdrTransportKind::Socket),
        herdr.binary_path,
        herdr.socket_path,
    )
    .map_err(|error| {
        AtmError::new(
            AtmErrorCode::ConfigParseFailed,
            format!("failed to validate {} [herdr]: {error}", path.display()),
        )
        .with_cause(error)
    })?;
    Ok(DaemonHerdrConfig { client })
}

#[cfg(test)]
mod tests {
    use super::daemon_herdr_config;
    use atm_core::doctor::HerdrTransportKind;
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::test_support::FakeEnvSource;

    fn env_for(home: &tempfile::TempDir) -> FakeEnvSource {
        FakeEnvSource::new([("HOME", Some(home.path().to_str().expect("utf8 path")))])
    }

    #[test]
    fn missing_config_uses_the_valid_default() {
        let home = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            daemon_herdr_config(&env_for(&home))
                .expect("default config")
                .client,
            atm_herdr::HerdrClientConfig::default()
        );
        assert_eq!(
            daemon_herdr_config(&env_for(&home))
                .expect("default config")
                .client
                .transport(),
            &HerdrTransportKind::Socket
        );
    }

    #[test]
    fn reads_valid_absolute_paths_once_from_home_config() {
        let home = tempfile::tempdir().expect("tempdir");
        let binary_path = home.path().join("bin").join("herdr");
        let socket_path = home.path().join("run").join("herdr.sock");
        std::fs::write(
            home.path().join(".atm.toml"),
            format!(
                "[herdr]\nbinary_path = {}\nsocket_path = {}\n",
                toml::Value::String(binary_path.to_string_lossy().into_owned()),
                toml::Value::String(socket_path.to_string_lossy().into_owned()),
            ),
        )
        .expect("write config");

        let config = daemon_herdr_config(&env_for(&home)).expect("config parses");

        assert_eq!(config.client.binary_path(), Some(binary_path.as_path()));
        assert_eq!(config.client.socket_path(), Some(socket_path.as_path()));
        assert_eq!(config.client.transport(), &HerdrTransportKind::Socket);
    }

    #[test]
    fn accepts_only_socket_and_explicit_cli_transport_values() {
        for (value, expected) in [
            ("socket", HerdrTransportKind::Socket),
            ("cli", HerdrTransportKind::Cli),
        ] {
            let home = tempfile::tempdir().expect("tempdir");
            std::fs::write(
                home.path().join(".atm.toml"),
                format!("[herdr]\ntransport = \"{value}\"\n"),
            )
            .expect("write config");

            assert_eq!(
                daemon_herdr_config(&env_for(&home))
                    .expect("supported transport parses")
                    .client
                    .transport(),
                &expected
            );
        }
    }

    #[test]
    fn rejects_unknown_transport_with_file_and_value_context() {
        let home = tempfile::tempdir().expect("tempdir");
        let path = home.path().join(".atm.toml");
        std::fs::write(&path, "[herdr]\ntransport = \"other\"\n").expect("write config");

        let error = daemon_herdr_config(&env_for(&home)).expect_err("must reject transport");

        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
        assert!(error.detail().contains(path.to_str().expect("utf8 path")));
        assert!(error.detail().contains("transport"));
        assert!(error.detail().contains("other"));
        assert!(error.cause().is_some());
    }

    #[test]
    fn rejects_unresolvable_home_with_config_parse_failed() {
        let error = daemon_herdr_config(&FakeEnvSource::empty()).expect_err("must fail");

        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
        assert!(error.detail().contains(".atm.toml [herdr]"));
    }

    #[test]
    fn preserves_the_io_cause_when_the_config_file_is_unreadable() {
        let home = tempfile::tempdir().expect("tempdir");
        let path = home.path().join(".atm.toml");
        std::fs::create_dir(&path).expect("directory at config path");

        let error = daemon_herdr_config(&env_for(&home)).expect_err("directory is unreadable");

        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
        assert!(error.detail().contains(path.to_str().expect("utf8 path")));
        assert!(error.cause().is_some());
    }

    #[test]
    fn rejects_unknown_herdr_key_with_file_context() {
        let home = tempfile::tempdir().expect("tempdir");
        let path = home.path().join(".atm.toml");
        std::fs::write(&path, "[herdr]\nunknown = \"value\"\n").expect("write config");

        let error = daemon_herdr_config(&env_for(&home)).expect_err("must fail");

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

        let error = daemon_herdr_config(&env_for(&home)).expect_err("must fail");

        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
        assert!(error.detail().contains(path.to_str().expect("utf8 path")));
        assert!(error.cause().is_some());
    }

    #[test]
    fn rejects_relative_path_with_file_and_key_context() {
        let home = tempfile::tempdir().expect("tempdir");
        let path = home.path().join(".atm.toml");
        std::fs::write(&path, "[herdr]\nbinary_path = \"relative/herdr\"\n").expect("write config");

        let error = daemon_herdr_config(&env_for(&home)).expect_err("must fail");

        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
        assert!(error.detail().contains(path.to_str().expect("utf8 path")));
        assert!(error.detail().contains("binary_path"));
        assert!(error.cause().is_some());
    }
}
