use std::path::{Path, PathBuf};
use std::time::Duration;

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
    escalation_min_interval_secs: Option<i64>,
}
#[derive(Deserialize, Default)]
struct RawConfig {
    herdr: Option<RawHerdrConfig>,
}

pub(crate) const DEFAULT_HERDR_ESCALATION_MIN_INTERVAL: Duration = Duration::from_secs(1_800);
const MIN_HERDR_ESCALATION_MIN_INTERVAL_SECS: i64 = 1;
const MAX_HERDR_ESCALATION_MIN_INTERVAL_SECS: i64 = 86_400;

/// Single-read bootstrap configuration for the validated Herdr client and
/// Tokio runtime's breaker-escalation cadence.
#[derive(Clone, Debug)]
pub(crate) struct DaemonHerdrConfig {
    pub(crate) client: HerdrClientConfig,
    pub(crate) escalation_min_interval: Duration,
}

impl Default for DaemonHerdrConfig {
    fn default() -> Self {
        Self {
            client: HerdrClientConfig::default(),
            escalation_min_interval: DEFAULT_HERDR_ESCALATION_MIN_INTERVAL,
        }
    }
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
    let escalation_min_interval =
        validate_escalation_min_interval(herdr.escalation_min_interval_secs, &path)?;
    Ok(DaemonHerdrConfig {
        client,
        escalation_min_interval,
    })
}

fn validate_escalation_min_interval(
    seconds: Option<i64>,
    path: &Path,
) -> Result<Duration, AtmError> {
    let seconds = seconds.unwrap_or(DEFAULT_HERDR_ESCALATION_MIN_INTERVAL.as_secs() as i64);
    if !(MIN_HERDR_ESCALATION_MIN_INTERVAL_SECS..=MAX_HERDR_ESCALATION_MIN_INTERVAL_SECS)
        .contains(&seconds)
    {
        return Err(AtmError::new(
            AtmErrorCode::ConfigParseFailed,
            format!(
                "failed to validate {} [herdr].escalation_min_interval_secs={seconds}; accepted range is {MIN_HERDR_ESCALATION_MIN_INTERVAL_SECS}..={MAX_HERDR_ESCALATION_MIN_INTERVAL_SECS}",
                path.display()
            ),
        ));
    }
    Ok(Duration::from_secs(seconds as u64))
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_HERDR_ESCALATION_MIN_INTERVAL, daemon_herdr_config};
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
                .escalation_min_interval,
            DEFAULT_HERDR_ESCALATION_MIN_INTERVAL
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

    #[test]
    fn reads_escalation_interval_bounds_and_rejects_invalid_values() {
        for (value, expected) in [("1", 1), ("86400", 86_400)] {
            let home = tempfile::tempdir().expect("tempdir");
            std::fs::write(
                home.path().join(".atm.toml"),
                format!("[herdr]\nescalation_min_interval_secs = {value}\n"),
            )
            .expect("write config");
            assert_eq!(
                daemon_herdr_config(&env_for(&home))
                    .expect("valid interval")
                    .escalation_min_interval,
                std::time::Duration::from_secs(expected)
            );
        }

        for value in ["0", "-1", "86401", "9223372036854775808", "\"wrong\""] {
            let home = tempfile::tempdir().expect("tempdir");
            let path = home.path().join(".atm.toml");
            std::fs::write(
                &path,
                format!("[herdr]\nescalation_min_interval_secs = {value}\n"),
            )
            .expect("write config");
            let error = daemon_herdr_config(&env_for(&home)).expect_err("must reject interval");
            assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
            assert!(error.detail().contains(path.to_str().expect("utf8 path")));
            assert!(
                error.detail().contains("escalation_min_interval_secs") || error.cause().is_some()
            );
        }
    }
}
