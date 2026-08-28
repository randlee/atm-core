use std::num::NonZeroU16;
use std::time::Duration;

use atm_core::error::AtmError;
use atm_core::peer_wire::PeerWireMode;
use atm_http_runtime::PeerPoolConfig;

/// Extracts the value for a single-value CLI `flag` from one already-dequeued,
/// UTF-8-validated `argument`, honoring both `--flag value` (space-separated,
/// consuming the next item from `arguments`) and `--flag=value` syntaxes.
///
/// Returns `Ok(None)` when `argument` does not name `flag` at all. The
/// `missing_value` closure supplies the error used when `--flag` is the final
/// daemon-launch argument (space-separated form with no following value).
fn take_single_flag_value(
    arguments: &mut impl Iterator<Item = std::ffi::OsString>,
    argument: &str,
    flag: &str,
    missing_value: impl FnOnce() -> AtmError,
) -> Result<Option<std::ffi::OsString>, AtmError> {
    if argument == flag {
        return arguments.next().map(Some).ok_or_else(missing_value);
    }
    let prefixed = format!("{flag}=");
    Ok(argument
        .strip_prefix(prefixed.as_str())
        .map(std::ffi::OsString::from))
}

/// Parse the sole non-durable peer-wire launch policy before composition.
pub fn parse_peer_wire_mode(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<PeerWireMode, AtmError> {
    if std::env::var_os("ATM_PEER_WIRE_SECURITY").is_some() {
        return Err(AtmError::peer_wire_mode_source_forbidden(
            "ATM_PEER_WIRE_SECURITY is forbidden; use --peer-wire-security at daemon launch",
        ));
    }
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let mut mode = None;
    while let Some(argument) = arguments.next() {
        let argument = argument.into_string().map_err(|_| {
            AtmError::peer_wire_mode_invalid("daemon launch arguments must be valid UTF-8")
        })?;
        let Some(value) =
            take_single_flag_value(&mut arguments, &argument, "--peer-wire-security", || {
                AtmError::peer_wire_mode_invalid(
                    "--peer-wire-security requires `mutual-tls` or `plaintext-test`",
                )
            })?
        else {
            continue;
        };
        let value = value.into_string().map_err(|_| {
            AtmError::peer_wire_mode_invalid("peer-wire launch mode must be valid UTF-8")
        })?;
        let parsed = match value.as_str() {
            "mutual-tls" => PeerWireMode::mtls(),
            "plaintext-test" => PeerWireMode::plaintext_test(),
            _ => {
                return Err(AtmError::peer_wire_mode_invalid(
                    "--peer-wire-security accepts only `mutual-tls` or `plaintext-test`",
                ));
            }
        };
        if mode.replace(parsed).is_some() {
            return Err(AtmError::peer_wire_mode_invalid(
                "--peer-wire-security may be supplied only once",
            ));
        }
    }
    Ok(mode.unwrap_or_default())
}

/// Selects the direct-peer listener port from the immutable daemon launch.
pub fn parse_direct_peer_port(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<NonZeroU16, AtmError> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let mut port = None;
    while let Some(argument) = arguments.next() {
        let argument = argument
            .into_string()
            .map_err(|_| AtmError::config("direct-peer launch arguments must be valid UTF-8"))?;
        let Some(value) =
            take_single_flag_value(&mut arguments, &argument, "--direct-peer-port", || {
                AtmError::config("--direct-peer-port requires a non-zero TCP port")
            })?
        else {
            continue;
        };
        let value = value
            .into_string()
            .map_err(|_| AtmError::config("direct-peer launch port must be valid UTF-8"))?;
        let parsed = value
            .parse::<u16>()
            .ok()
            .and_then(NonZeroU16::new)
            .ok_or_else(|| AtmError::config("--direct-peer-port requires a non-zero TCP port"))?;
        if port.replace(parsed).is_some() {
            return Err(AtmError::config(
                "--direct-peer-port may be supplied only once",
            ));
        }
    }
    Ok(port.unwrap_or_else(|| {
        NonZeroU16::new(atm_http_runtime::DIRECT_PEER_TCP_PORT)
            .expect("the protocol direct-peer port is non-zero")
    }))
}

/// Resolves bounded outbound peer-pool settings before daemon composition.
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
    for flag in [
        "--peer-pool-max-per-peer",
        "--peer-pool-max-pooled-total",
        "--peer-pool-idle-timeout-ms",
    ] {
        if let Some(value) = take_single_flag_value(arguments, &argument, flag, || {
            AtmError::config(format!("{flag} requires a positive integer"))
        })? {
            return Ok(Some((flag, value)));
        }
    }
    Ok(None)
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

#[cfg(test)]
mod take_single_flag_value_tests {
    use std::ffi::OsString;

    use super::{AtmError, take_single_flag_value};

    fn unreachable_missing_value() -> AtmError {
        panic!("missing_value must not be invoked when a value is present")
    }

    #[test]
    fn space_syntax_consumes_the_next_argument() {
        let mut arguments = vec![OsString::from("value")].into_iter();
        let value = take_single_flag_value(
            &mut arguments,
            "--flag",
            "--flag",
            unreachable_missing_value,
        )
        .expect("space syntax resolves")
        .expect("flag matched");
        assert_eq!(value, OsString::from("value"));
        assert_eq!(
            arguments.next(),
            None,
            "the value argument must be consumed from the iterator"
        );
    }

    #[test]
    fn equals_syntax_does_not_consume_the_iterator() {
        let mut arguments = vec![OsString::from("--unrelated")].into_iter();
        let value = take_single_flag_value(
            &mut arguments,
            "--flag=value",
            "--flag",
            unreachable_missing_value,
        )
        .expect("equals syntax resolves")
        .expect("flag matched");
        assert_eq!(value, OsString::from("value"));
        assert_eq!(
            arguments.next(),
            Some(OsString::from("--unrelated")),
            "the equals syntax must not consume a following argument"
        );
    }

    #[test]
    fn non_matching_argument_returns_none() {
        let mut arguments = std::iter::empty();
        let selected = take_single_flag_value(
            &mut arguments,
            "--other-flag",
            "--flag",
            unreachable_missing_value,
        )
        .expect("non-matching argument is not an error");
        assert_eq!(selected, None);
    }

    #[test]
    fn prefix_that_is_not_the_flag_does_not_match() {
        // `--flag-extra=value` shares a prefix with `--flag` but is a
        // different flag entirely and must not be treated as `--flag=`.
        let mut arguments = std::iter::empty();
        let selected = take_single_flag_value(
            &mut arguments,
            "--flag-extra=value",
            "--flag",
            unreachable_missing_value,
        )
        .expect("non-matching argument is not an error");
        assert_eq!(selected, None);
    }

    #[test]
    fn missing_value_invokes_the_supplied_closure() {
        let mut arguments = std::iter::empty();
        let error = take_single_flag_value(&mut arguments, "--flag", "--flag", || {
            AtmError::config("--flag requires a value")
        })
        .expect_err("space syntax with nothing following is an error");
        assert!(error.message().contains("--flag requires a value"));
    }

    #[test]
    fn returned_value_is_not_utf8_validated_by_the_helper() {
        // The helper hands back the raw `OsString` unchanged; UTF-8
        // validation of the value remains each caller's responsibility so
        // every call site can keep its own existing error message.
        let mut arguments = vec![non_utf8_os_string()].into_iter();
        let value = take_single_flag_value(
            &mut arguments,
            "--flag",
            "--flag",
            unreachable_missing_value,
        )
        .expect("helper does not itself validate UTF-8")
        .expect("flag matched");
        assert_eq!(value, non_utf8_os_string());
        assert!(value.into_string().is_err(), "value remains non-UTF-8");
    }

    #[cfg(unix)]
    fn non_utf8_os_string() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![0xff, 0xfe])
    }

    #[cfg(not(unix))]
    fn non_utf8_os_string() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        // An unpaired surrogate is valid `OsString` on Windows but not valid
        // UTF-8.
        OsString::from_wide(&[0xd800])
    }

    #[test]
    fn duplicate_rejection_is_the_callers_responsibility() {
        // The helper only decides whether one argument names the flag; each
        // call site composes it with its own "supplied only once" tracking,
        // the same way `parse_peer_wire_mode` and `parse_direct_peer_port` do.
        fn parse_with_duplicate_guard(
            arguments: impl IntoIterator<Item = OsString>,
        ) -> Result<OsString, AtmError> {
            let mut arguments = arguments.into_iter();
            let mut seen = None;
            while let Some(argument) = arguments.next() {
                let argument = argument.into_string().expect("test arguments are UTF-8");
                let Some(value) = take_single_flag_value(
                    &mut arguments,
                    &argument,
                    "--flag",
                    unreachable_missing_value,
                )?
                else {
                    continue;
                };
                if seen.replace(value).is_some() {
                    return Err(AtmError::config("--flag may be supplied only once"));
                }
            }
            Ok(seen.expect("fixture always supplies --flag at least once"))
        }

        let error = parse_with_duplicate_guard([
            OsString::from("--flag"),
            OsString::from("first"),
            OsString::from("--flag=second"),
        ])
        .expect_err("second match must be rejected by the caller's duplicate guard");
        assert!(error.message().contains("only once"));

        let value = parse_with_duplicate_guard([OsString::from("--flag"), OsString::from("only")])
            .expect("a single match is accepted");
        assert_eq!(value, OsString::from("only"));
    }
}
