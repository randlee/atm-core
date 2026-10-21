use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::time::Duration;

use atm_core::error::AtmError;
use atm_core::peer_wire::PeerWireMode;
use atm_http_runtime::PeerPoolConfig;
use atm_storage::{HttpsInterface, PeerConfigStore};

const FIRST_PEER_PORT: u16 = 43101;
const LAST_PEER_PORT: u16 = 43199;

fn system_claims_dir() -> Result<PathBuf, AtmError> {
    #[cfg(target_os = "macos")]
    {
        Ok(PathBuf::from("/Users/Shared/atm/ports"))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(PathBuf::from("/var/lib/atm/ports"))
    }
    #[cfg(windows)]
    {
        Ok(
            PathBuf::from(std::env::var_os("ProgramData").ok_or_else(|| {
                AtmError::peer_config_validation(
                    "ProgramData is unset; set it to the Windows shared application-data directory",
                )
            })?)
            .join("ATM")
            .join("ports"),
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        Err(AtmError::peer_config_validation(
            "peer port claims are unsupported on this platform",
        ))
    }
}

fn claim_error(dir: &Path, error: impl std::fmt::Display) -> AtmError {
    AtmError::peer_config_validation(format!(
        "peer port claims directory {} is unavailable: {error}; create it with mode 1777 and grant this account write access",
        dir.display()
    ))
}

fn prepare_claims_dir(dir: &Path) -> Result<(), AtmError> {
    fs::create_dir_all(dir).map_err(|error| claim_error(dir, error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::metadata(dir)
            .map_err(|error| claim_error(dir, error))?
            .permissions();
        if permissions.mode() & 0o7777 != 0o1777 {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o1777))
                .map_err(|error| claim_error(dir, error))?;
        }
    }
    Ok(())
}

fn write_claim(file: &mut fs::File) -> Result<(), AtmError> {
    #[cfg(unix)]
    let uid = {
        use std::os::unix::fs::MetadataExt;
        Some(
            file.metadata()
                .map_err(|error| {
                    AtmError::peer_config_validation(format!(
                        "could not read peer port claim owner: {error}"
                    ))
                })?
                .uid(),
        )
    };
    #[cfg(not(unix))]
    let uid: Option<u32> = None;
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| {
            AtmError::peer_config_validation(format!(
                "clock error while claiming peer port: {error}"
            ))
        })?
        .as_secs();
    serde_json::to_writer(
        &mut *file,
        &serde_json::json!({
            "uid": uid, "user": user, "timestamp_unix_seconds": timestamp
        }),
    )
    .map_err(|error| {
        AtmError::peer_config_validation(format!("could not write peer port claim: {error}"))
    })?;
    file.write_all(b"\n").map_err(|error| {
        AtmError::peer_config_validation(format!("could not finish peer port claim: {error}"))
    })?;
    file.sync_all().map_err(|error| {
        AtmError::peer_config_validation(format!("could not sync peer port claim: {error}"))
    })
}

fn claim_file(dir: &Path, port: NonZeroU16) -> Result<bool, AtmError> {
    let path = dir.join(port.get().to_string());
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            if let Err(error) = write_claim(&mut file) {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(claim_error(dir, error)),
    }
}

fn port_is_free(port: NonZeroU16) -> bool {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port.get())).is_ok()
}

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

/// Resolve the account's durable peer-listener port. Every enabled interface
/// must advertise the same port because the direct listener binds all IPv4
/// addresses on this account's one selected port.
pub fn configured_direct_peer_port(
    store: &(dyn PeerConfigStore + Send + Sync),
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<NonZeroU16, AtmError> {
    configured_direct_peer_port_with(store, arguments, &system_claims_dir()?, port_is_free)
}

fn configured_direct_peer_port_with(
    store: &(dyn PeerConfigStore + Send + Sync),
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
    claims_dir: &Path,
    is_free: impl Fn(NonZeroU16) -> bool,
) -> Result<NonZeroU16, AtmError> {
    for argument in arguments.into_iter().skip(1) {
        if argument.to_string_lossy().starts_with("--direct-peer-port") {
            return Err(AtmError::config(
                "--direct-peer-port is unsupported; configure `atm peer interface set --bind`",
            ));
        }
    }
    let mut selected = None;
    for interface in store
        .list_interfaces()?
        .into_iter()
        .filter(|item| item.enabled)
    {
        let port = NonZeroU16::new(interface.bind_addr.port()).ok_or_else(|| {
            AtmError::peer_config_validation("enabled peer interface requires a non-zero bind port")
        })?;
        if selected.is_some_and(|previous| previous != port) {
            return Err(AtmError::peer_config_validation(
                "enabled peer interfaces must use the same bind port",
            ));
        }
        selected = Some(port);
    }
    prepare_claims_dir(claims_dir)?;
    if let Some(port) = selected {
        let _ = claim_file(claims_dir, port)?;
        return Ok(port);
    }
    for raw_port in FIRST_PEER_PORT..=LAST_PEER_PORT {
        let port = NonZeroU16::new(raw_port).expect("claim range excludes zero");
        if !claim_file(claims_dir, port)? {
            continue;
        }
        let claim_path = claims_dir.join(raw_port.to_string());
        if !is_free(port) {
            fs::remove_file(&claim_path).map_err(|error| claim_error(claims_dir, error))?;
            continue;
        }
        let interface = HttpsInterface {
            bind_addr: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, raw_port).into(),
            advertise_host: "localhost".parse().expect("valid default hostname"),
            enabled: true,
        };
        if let Err(error) = store.save_interface(&interface) {
            let _ = fs::remove_file(&claim_path);
            return Err(error);
        }
        return Ok(port);
    }
    Err(AtmError::peer_config_validation(format!(
        "peer port range {FIRST_PEER_PORT}-{LAST_PEER_PORT} is exhausted in {}; release an unused claim or configure a peer interface with a free port",
        claims_dir.display()
    )))
}

#[cfg(test)]
mod direct_peer_port_tests {
    use std::ffi::OsString;
    use std::num::NonZeroU16;
    use std::path::Path;

    use atm_runtime_test_support::open_sqlite_boundary;
    use atm_storage::HttpsInterface;

    use super::{FIRST_PEER_PORT, LAST_PEER_PORT, configured_direct_peer_port_with};

    fn args() -> [OsString; 1] {
        [OsString::from("atm-daemon")]
    }

    fn resolve(store: &(dyn atm_storage::PeerConfigStore + Send + Sync), claims: &Path) -> u16 {
        configured_direct_peer_port_with(store, args(), claims, |_| true)
            .expect("claimed port")
            .get()
    }

    #[test]
    fn listener_port_comes_from_persisted_account_interface() {
        let root = tempfile::tempdir().expect("test root");
        let assembly =
            open_sqlite_boundary(root.path().join("peer.sqlite")).expect("SQLite peer boundary");
        let store = assembly.peer_config_store();
        let claims = root.path().join("claims");

        store
            .save_interface(&HttpsInterface {
                bind_addr: "127.0.0.1:43201".parse().expect("bind address"),
                advertise_host: "peer.example.test".parse().expect("host"),
                enabled: true,
            })
            .expect("save account interface");
        assert_eq!(
            configured_direct_peer_port_with(store.as_ref(), args(), &claims, |_| false)
                .expect("configured port"),
            NonZeroU16::new(43201).expect("non-zero port"),
        );
        assert!(claims.join("43201").exists());
        let reopened =
            open_sqlite_boundary(root.path().join("peer.sqlite")).expect("reopen account store");
        assert_eq!(
            configured_direct_peer_port_with(
                reopened.peer_config_store().as_ref(),
                args(),
                &claims,
                |_| false
            )
            .expect("port survives a new runtime assembly"),
            NonZeroU16::new(43201).expect("non-zero port"),
        );
        assert!(
            configured_direct_peer_port_with(
                store.as_ref(),
                [
                    OsString::from("atm-daemon"),
                    OsString::from("--direct-peer-port=43101")
                ],
                &claims,
                |_| true,
            )
            .is_err()
        );
    }

    #[test]
    fn second_account_claims_next_port_and_existing_account_reuses_its_port() {
        let root = tempfile::tempdir().expect("test root");
        let claims = root.path().join("claims");
        let first = open_sqlite_boundary(root.path().join("first.sqlite")).expect("first account");
        let second =
            open_sqlite_boundary(root.path().join("second.sqlite")).expect("second account");
        assert_eq!(
            resolve(first.peer_config_store().as_ref(), &claims),
            FIRST_PEER_PORT
        );
        assert_eq!(
            first
                .peer_config_store()
                .list_interfaces()
                .expect("interfaces")[0]
                .bind_addr
                .port(),
            FIRST_PEER_PORT
        );
        let claim: serde_json::Value = serde_json::from_slice(
            &std::fs::read(claims.join(FIRST_PEER_PORT.to_string())).expect("claim file"),
        )
        .expect("claim JSON");
        assert!(claim.get("uid").is_some());
        assert!(claim.get("user").is_some());
        assert!(claim.get("timestamp_unix_seconds").is_some());
        assert_eq!(
            resolve(second.peer_config_store().as_ref(), &claims),
            FIRST_PEER_PORT + 1
        );
        let restarted = open_sqlite_boundary(root.path().join("first.sqlite")).expect("restart");
        assert_eq!(
            resolve(restarted.peer_config_store().as_ref(), &claims),
            FIRST_PEER_PORT
        );
    }

    #[test]
    fn exclusive_claim_file_race_and_bound_port_skip_are_deterministic() {
        let root = tempfile::tempdir().expect("test root");
        let claims = root.path().join("claims");
        std::fs::create_dir_all(&claims).expect("claims directory");
        std::fs::write(claims.join(FIRST_PEER_PORT.to_string()), "other account")
            .expect("racing account won exclusive create");
        let account = open_sqlite_boundary(root.path().join("account.sqlite")).expect("account");
        let claimed = configured_direct_peer_port_with(
            account.peer_config_store().as_ref(),
            args(),
            &claims,
            |port| port.get() != FIRST_PEER_PORT + 1,
        )
        .expect("claim after occupied and bound ports");
        assert_eq!(claimed.get(), FIRST_PEER_PORT + 2);
        assert!(!claims.join((FIRST_PEER_PORT + 1).to_string()).exists());
        assert_eq!(
            std::fs::read_to_string(claims.join(FIRST_PEER_PORT.to_string())).expect("other claim"),
            "other account"
        );
    }

    #[test]
    fn exhausted_claim_range_is_a_typed_error() {
        let root = tempfile::tempdir().expect("test root");
        let claims = root.path().join("claims");
        std::fs::create_dir_all(&claims).expect("claims directory");
        for port in FIRST_PEER_PORT..=LAST_PEER_PORT {
            std::fs::write(claims.join(port.to_string()), "occupied").expect("occupy claim");
        }
        let account = open_sqlite_boundary(root.path().join("account.sqlite")).expect("account");
        let error = configured_direct_peer_port_with(
            account.peer_config_store().as_ref(),
            args(),
            &claims,
            |_| true,
        )
        .expect_err("range is exhausted");
        assert_eq!(
            error.code(),
            atm_core::error::AtmErrorCode::PeerConfigValidationFailed
        );
        assert!(error.message().contains("43101-43199"));
    }

    #[test]
    fn unavailable_claims_directory_names_path_and_repair() {
        let root = tempfile::tempdir().expect("test root");
        let claims = root.path().join("claims");
        std::fs::write(&claims, "a file blocks directory creation").expect("blocking file");
        let account = open_sqlite_boundary(root.path().join("account.sqlite")).expect("account");
        let error = configured_direct_peer_port_with(
            account.peer_config_store().as_ref(),
            args(),
            &claims,
            |_| true,
        )
        .expect_err("claim directory unavailable");
        assert_eq!(
            error.code(),
            atm_core::error::AtmErrorCode::PeerConfigValidationFailed
        );
        assert!(error.message().contains(&claims.display().to_string()));
        assert!(error.message().contains("mode 1777"));
    }
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
        // the same way `parse_peer_wire_mode` does.
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
