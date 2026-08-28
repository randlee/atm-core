use std::time::Duration;

use atm_core::error::AtmError;
use atm_http_runtime::PeerPoolConfig;

/// Resolves bounded outbound peer-pool settings before daemon composition.
/// Environment values provide deployment defaults; an explicit launch flag
/// overrides the matching environment value without changing peer identity or
/// wire-security policy.
pub fn parse_peer_pool_config(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<PeerPoolConfig, AtmError> {
    parse_peer_pool_config_with_environment(arguments, |name| std::env::var_os(name))
}

pub(crate) fn parse_peer_pool_config_with_environment(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
    mut environment: impl FnMut(&str) -> Option<std::ffi::OsString>,
) -> Result<PeerPoolConfig, AtmError> {
    let mut config = PeerPoolConfig::default();
    apply_peer_pool_environment(&mut config, &mut environment)?;
    apply_peer_pool_launch_overrides(&mut config, arguments)?;
    config.validate()?;
    Ok(config)
}

fn apply_peer_pool_environment(
    config: &mut PeerPoolConfig,
    environment: &mut impl FnMut(&str) -> Option<std::ffi::OsString>,
) -> Result<(), AtmError> {
    if let Some(value) = environment("ATM_PEER_POOL_MAX_PER_PEER") {
        config.max_per_peer = parse_pool_usize("ATM_PEER_POOL_MAX_PER_PEER", value)?;
    }
    if let Some(value) = environment("ATM_PEER_POOL_MAX_POOLED_TOTAL") {
        config.max_pooled_total = parse_pool_usize("ATM_PEER_POOL_MAX_POOLED_TOTAL", value)?;
    }
    if let Some(value) = environment("ATM_PEER_POOL_IDLE_TIMEOUT_MS") {
        config.idle_timeout =
            Duration::from_millis(parse_pool_u64("ATM_PEER_POOL_IDLE_TIMEOUT_MS", value)?);
    }
    Ok(())
}

fn apply_peer_pool_launch_overrides(
    config: &mut PeerPoolConfig,
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<(), AtmError> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let mut max_per_peer_seen = false;
    let mut max_total_seen = false;
    let mut idle_timeout_seen = false;
    while let Some(argument) = arguments.next() {
        let Some((name, value)) = peer_pool_launch_argument(&mut arguments, argument)? else {
            continue;
        };
        match name {
            "--peer-pool-max-per-peer" => {
                if max_per_peer_seen {
                    return Err(AtmError::config(
                        "--peer-pool-max-per-peer may be supplied only once",
                    ));
                }
                max_per_peer_seen = true;
                config.max_per_peer = parse_pool_usize(name, value)?;
            }
            "--peer-pool-max-pooled-total" => {
                if max_total_seen {
                    return Err(AtmError::config(
                        "--peer-pool-max-pooled-total may be supplied only once",
                    ));
                }
                max_total_seen = true;
                config.max_pooled_total = parse_pool_usize(name, value)?;
            }
            "--peer-pool-idle-timeout-ms" => {
                if idle_timeout_seen {
                    return Err(AtmError::config(
                        "--peer-pool-idle-timeout-ms may be supplied only once",
                    ));
                }
                idle_timeout_seen = true;
                config.idle_timeout = Duration::from_millis(parse_pool_u64(name, value)?);
            }
            _ => unreachable!("selected flag name is exhaustive"),
        }
    }
    Ok(())
}

fn peer_pool_launch_argument(
    arguments: &mut impl Iterator<Item = std::ffi::OsString>,
    argument: std::ffi::OsString,
) -> Result<Option<(&'static str, std::ffi::OsString)>, AtmError> {
    let argument = argument
        .into_string()
        .map_err(|_| AtmError::config("peer-pool launch arguments must be valid UTF-8"))?;
    let selected = match argument.as_str() {
        "--peer-pool-max-per-peer" => Some((
            "--peer-pool-max-per-peer",
            next_pool_argument(arguments, &argument)?,
        )),
        "--peer-pool-max-pooled-total" => Some((
            "--peer-pool-max-pooled-total",
            next_pool_argument(arguments, &argument)?,
        )),
        "--peer-pool-idle-timeout-ms" => Some((
            "--peer-pool-idle-timeout-ms",
            next_pool_argument(arguments, &argument)?,
        )),
        _ => argument
            .strip_prefix("--peer-pool-max-per-peer=")
            .map(|value| ("--peer-pool-max-per-peer", std::ffi::OsString::from(value)))
            .or_else(|| {
                argument
                    .strip_prefix("--peer-pool-max-pooled-total=")
                    .map(|value| {
                        (
                            "--peer-pool-max-pooled-total",
                            std::ffi::OsString::from(value),
                        )
                    })
            })
            .or_else(|| {
                argument
                    .strip_prefix("--peer-pool-idle-timeout-ms=")
                    .map(|value| {
                        (
                            "--peer-pool-idle-timeout-ms",
                            std::ffi::OsString::from(value),
                        )
                    })
            }),
    };
    Ok(selected)
}

fn next_pool_argument(
    arguments: &mut impl Iterator<Item = std::ffi::OsString>,
    flag: &str,
) -> Result<std::ffi::OsString, AtmError> {
    arguments
        .next()
        .ok_or_else(|| AtmError::config(format!("{flag} requires a positive integer")))
}

fn parse_pool_usize(name: &str, value: std::ffi::OsString) -> Result<usize, AtmError> {
    let value = value
        .into_string()
        .map_err(|_| AtmError::config(format!("{name} must be valid UTF-8")))?;
    value
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| AtmError::config(format!("{name} requires a positive integer")))
}

fn parse_pool_u64(name: &str, value: std::ffi::OsString) -> Result<u64, AtmError> {
    let value = value
        .into_string()
        .map_err(|_| AtmError::config(format!("{name} must be valid UTF-8")))?;
    value
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| AtmError::config(format!("{name} requires a positive integer")))
}
