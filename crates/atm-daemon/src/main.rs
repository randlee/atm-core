use std::sync::Arc;

use atm_core::error::AtmError;
use sc_observability as _;

mod daemon_observability;

use daemon_observability::DaemonObservability;

const _: Option<fn(sc_observability::Logger)> = None;

fn main() {
    let exit_code = match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            atm_daemon::daemon_exit_code_for_error(&error).as_i32()
        }
    };
    std::process::exit(exit_code);
}

fn run() -> Result<(), AtmError> {
    atm_daemon_bootstrap::install_sqlite_retained_runtime_factory();
    let peer_wire_security = parse_peer_wire_security(std::env::args_os().skip(1))?;
    let observability: Arc<dyn atm_daemon::DaemonRuntimeObservability> =
        Arc::new(DaemonObservability::bootstrap()?);
    atm_daemon::run_daemon_with_observability_and_peer_wire_security(
        observability,
        peer_wire_security,
    )
}

fn parse_peer_wire_security(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<atm_daemon::PeerWireSecurity, AtmError> {
    let mut args = args.into_iter();
    let mut security = atm_daemon::PeerWireSecurity::MutualTls;
    while let Some(argument) = args.next() {
        if argument == "--peer-wire-security" {
            let value = args.next().ok_or_else(|| {
                AtmError::validation(
                    "--peer-wire-security requires `mutual-tls` or `plaintext-test`",
                )
            })?;
            security = value.to_string_lossy().parse()?;
        } else {
            return Err(AtmError::validation(format!(
                "unknown atm-daemon argument: {}",
                argument.to_string_lossy()
            )));
        }
    }
    Ok(security)
}

#[cfg(test)]
mod tests {
    use super::parse_peer_wire_security;
    use atm_daemon::PeerWireSecurity;

    #[test]
    fn peer_wire_security_defaults_to_mutual_tls() {
        assert_eq!(
            parse_peer_wire_security([]).expect("default security"),
            PeerWireSecurity::MutualTls
        );
    }

    #[test]
    fn peer_wire_security_accepts_only_the_explicit_smoke_flag() {
        assert_eq!(
            parse_peer_wire_security(["--peer-wire-security".into(), "plaintext-test".into(),])
                .expect("plaintext mode"),
            PeerWireSecurity::PlaintextTest
        );
        assert!(parse_peer_wire_security(["--peer-wire-security".into()]).is_err());
        assert!(parse_peer_wire_security(["--unknown".into()]).is_err());
    }
}
