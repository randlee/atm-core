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
    let wire_security = parse_peer_wire_security(std::env::args().skip(1))?;
    let observability: Arc<dyn atm_daemon::DaemonRuntimeObservability> =
        Arc::new(DaemonObservability::bootstrap()?);
    atm_daemon::run_daemon_with_observability_and_wire_security(observability, wire_security)
}

fn parse_peer_wire_security(
    args: impl IntoIterator<Item = String>,
) -> Result<atm_daemon::PeerWireSecurity, AtmError> {
    let mut args = args.into_iter();
    let mut wire_security = atm_daemon::PeerWireSecurity::MutualTls;
    while let Some(argument) = args.next() {
        if argument != "--peer-wire-security" {
            return Err(AtmError::validation(format!(
                "unknown atm-daemon argument `{argument}`; expected --peer-wire-security"
            )));
        }
        wire_security = args
            .next()
            .ok_or_else(|| {
                AtmError::validation(
                    "--peer-wire-security requires `mutual-tls` or `plaintext-test`",
                )
            })?
            .parse()?;
    }
    Ok(wire_security)
}

#[cfg(test)]
mod tests {
    use super::parse_peer_wire_security;
    use atm_daemon::PeerWireSecurity;

    #[test]
    fn default_peer_wire_security_is_mutual_tls() {
        assert_eq!(
            parse_peer_wire_security(Vec::<String>::new()).expect("default"),
            PeerWireSecurity::MutualTls
        );
    }

    #[test]
    fn plaintext_test_requires_explicit_cli_flag() {
        assert_eq!(
            parse_peer_wire_security(["--peer-wire-security".into(), "plaintext-test".into()])
                .expect("plaintext"),
            PeerWireSecurity::PlaintextTest
        );
    }

    #[test]
    fn environment_cannot_select_plaintext_test() {
        assert_eq!(
            parse_peer_wire_security(Vec::<String>::new()).expect("default"),
            PeerWireSecurity::MutualTls,
            "only the explicit daemon argument may select plaintext-test"
        );
    }
}
