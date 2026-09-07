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
    let Some(home) = atm_core::home::resolve_user_home_via(env) else {
        return Ok(HerdrClientConfig::default());
    };
    let path = home.join(".atm.toml");
    if !path.exists() {
        return Ok(HerdrClientConfig::default());
    }
    let text = std::fs::read_to_string(&path).map_err(|error| {
        AtmError::new(
            AtmErrorCode::ConfigParseFailed,
            format!("failed to read {}: {error}", path.display()),
        )
    })?;
    let raw: RawConfig = toml::from_str(&text).map_err(|error| {
        AtmError::new(
            AtmErrorCode::ConfigParseFailed,
            format!("failed to parse {}: {error}", path.display()),
        )
    })?;
    let herdr = raw.herdr.unwrap_or_default();
    HerdrClientConfig::try_new(herdr.binary_path, herdr.socket_path)
}
