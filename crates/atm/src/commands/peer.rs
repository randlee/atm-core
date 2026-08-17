use anyhow::Result;
use atm_daemon_bootstrap::with_default_peer_config_store;
use atm_storage::{
    AtmError, CertificateFingerprint, HostName, HttpsInterface, LocalCertificate, PeerConfigStore,
    TrustedPeer,
};
use clap::{Args, Subcommand};
use serde::Serialize;
use std::num::NonZeroU16;

use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;

/// Manage durable cross-host HTTPS control-plane configuration.
#[derive(Debug, Args)]
pub struct PeerCommand {
    #[command(subcommand)]
    command: PeerSubcommand,
}

#[derive(Debug, Subcommand)]
enum PeerSubcommand {
    Interface(InterfaceCommand),
    Certificate(CertificateCommand),
    Trust(TrustCommand),
}

#[derive(Debug, Args)]
struct InterfaceCommand {
    #[command(subcommand)]
    command: InterfaceSubcommand,
}

#[derive(Debug, Subcommand)]
enum InterfaceSubcommand {
    List {
        #[arg(long)]
        json: bool,
    },
    Set {
        #[arg(long)]
        bind: String,
        #[arg(long)]
        advertise_host: String,
        #[arg(long, default_value_t = true)]
        enabled: bool,
    },
    Remove {
        #[arg(long)]
        bind: String,
    },
}

#[derive(Debug, Args)]
struct CertificateCommand {
    #[command(subcommand)]
    command: CertificateSubcommand,
}

#[derive(Debug, Subcommand)]
enum CertificateSubcommand {
    Show {
        #[arg(long)]
        json: bool,
    },
    Init {
        #[arg(long)]
        fingerprint: String,
        #[arg(long)]
        private_key_ref: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Args)]
struct TrustCommand {
    #[command(subcommand)]
    command: TrustSubcommand,
}

#[derive(Debug, Subcommand)]
enum TrustSubcommand {
    List {
        #[arg(long)]
        json: bool,
    },
    Add {
        #[arg(long)]
        host: String,
        #[arg(long)]
        fingerprint: String,
        #[arg(long, default_value_t = 43101)]
        https_port: u16,
        #[arg(long)]
        yes: bool,
    },
    Replace {
        #[arg(long)]
        host: String,
        #[arg(long)]
        fingerprint: String,
        #[arg(long, default_value_t = 43101)]
        https_port: u16,
        #[arg(long)]
        yes: bool,
    },
    Revoke {
        #[arg(long)]
        host: String,
        #[arg(long)]
        yes: bool,
    },
}

impl PeerCommand {
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        match self.command {
            PeerSubcommand::Trust(command) => {
                let changed =
                    with_default_peer_config_store(|store| command.run_with_store(store))?;
                if changed {
                    Self::reload_runtime_view(observability).await?;
                }
                Ok(())
            }
            command => {
                with_default_peer_config_store(|store| Self { command }.run_with_store(store))?;
                Ok(())
            }
        }
    }

    fn run_with_store(self, store: &(dyn PeerConfigStore + Send + Sync)) -> Result<(), AtmError> {
        match self.command {
            PeerSubcommand::Interface(command) => command.run_with_store(store),
            PeerSubcommand::Certificate(command) => command.run_with_store(store),
            PeerSubcommand::Trust(command) => command.run_with_store(store).map(|_| ()),
        }
    }

    async fn reload_runtime_view(observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("peer trust reload")?;
        let composition = CliComposition::bootstrap(
            "peer trust reload",
            observability,
            InvocationDir::new(&current_dir),
            AtmHomePath::new(&home_dir),
        )?;
        Ok(composition.reload_runtime_view().await?)
    }
}

impl InterfaceCommand {
    fn run_with_store(self, store: &(dyn PeerConfigStore + Send + Sync)) -> Result<(), AtmError> {
        match self.command {
            InterfaceSubcommand::List { json } => {
                let interfaces = store.list_interfaces()?;
                print_output(&interfaces, json)
            }
            InterfaceSubcommand::Set {
                bind,
                advertise_host,
                enabled,
            } => {
                let interface = HttpsInterface {
                    bind_addr: bind.parse().map_err(|_source| {
                        AtmError::peer_config_validation("invalid --bind socket address")
                    })?,
                    advertise_host: advertise_host.parse().map_err(|_source| {
                        AtmError::peer_config_validation("invalid --advertise-host")
                    })?,
                    enabled,
                };
                store.save_interface(&interface)?;
                println!("saved HTTPS interface {}", interface.bind_addr);
                Ok(())
            }
            InterfaceSubcommand::Remove { bind } => {
                let bind = bind.parse().map_err(|_source| {
                    AtmError::peer_config_validation("invalid --bind socket address")
                })?;
                let removed = store.remove_interface(bind)?;
                println!(
                    "{} HTTPS interface {}",
                    if removed { "removed" } else { "no" },
                    bind
                );
                Ok(())
            }
        }
    }
}

impl CertificateCommand {
    fn run_with_store(self, store: &(dyn PeerConfigStore + Send + Sync)) -> Result<(), AtmError> {
        match self.command {
            CertificateSubcommand::Show { json } => {
                // The record has only a key reference, never key material.
                let certificate = store.local_certificate()?;
                print_output(&certificate, json)
            }
            CertificateSubcommand::Init {
                fingerprint,
                private_key_ref,
                yes,
            } => {
                require_confirmation(yes, "initializing the local certificate")?;
                let certificate = certificate(fingerprint, private_key_ref)?;
                store.save_local_certificate(&certificate)?;
                println!(
                    "saved local certificate fingerprint {}",
                    certificate.fingerprint
                );
                Ok(())
            }
        }
    }
}

impl TrustCommand {
    fn run_with_store(self, store: &(dyn PeerConfigStore + Send + Sync)) -> Result<bool, AtmError> {
        match self.command {
            TrustSubcommand::List { json } => {
                let peers = store.list_trusted_peers()?;
                print_output(&peers, json)?;
                Ok(false)
            }
            TrustSubcommand::Add {
                host,
                fingerprint,
                https_port,
                yes,
            } => {
                require_confirmation(yes, "adding a trusted peer")?;
                let peer = peer(host, fingerprint, https_port)?;
                if store.trusted_peer(&peer.host)?.is_some() {
                    return Err(atm_storage::AtmError::validation(
                        "trusted peer already exists; use `atm peer trust replace --yes`",
                    ));
                }
                store.save_trusted_peer(&peer)?;
                println!("added trusted peer {}", peer.host);
                Ok(true)
            }
            TrustSubcommand::Replace {
                host,
                fingerprint,
                https_port,
                yes,
            } => {
                require_confirmation(yes, "replacing a trusted-peer fingerprint")?;
                let peer = peer(host, fingerprint, https_port)?;
                if store.trusted_peer(&peer.host)?.is_none() {
                    return Err(atm_storage::AtmError::validation(
                        "trusted peer does not exist; use `atm peer trust add --yes`",
                    ));
                }
                store.save_trusted_peer(&peer)?;
                println!("replaced trusted peer {}", peer.host);
                Ok(true)
            }
            TrustSubcommand::Revoke { host, yes } => {
                require_confirmation(yes, "revoking a trusted peer")?;
                let host: HostName = host
                    .parse()
                    .map_err(|_source| AtmError::peer_config_validation("invalid --host"))?;
                let removed = store.remove_trusted_peer(&host)?;
                println!(
                    "{} trusted peer {}",
                    if removed { "revoked" } else { "no" },
                    host
                );
                Ok(removed)
            }
        }
    }
}

fn peer(
    host: String,
    fingerprint: String,
    https_port: u16,
) -> std::result::Result<TrustedPeer, AtmError> {
    Ok(TrustedPeer {
        host: trusted_peer_host(&host)?,
        fingerprint: fingerprint
            .parse::<CertificateFingerprint>()
            .map_err(|_source| AtmError::peer_config_validation("invalid --fingerprint"))?,
        enabled: true,
        https_port: NonZeroU16::new(https_port)
            .ok_or_else(|| AtmError::peer_config_validation("--https-port must be non-zero"))?,
    })
}

fn trusted_peer_host(value: &str) -> std::result::Result<HostName, AtmError> {
    let host: HostName = value
        .parse()
        .map_err(|_source| AtmError::peer_config_validation("invalid --host"))?;
    if !host.is_durable_hostname() {
        return Err(AtmError::peer_config_validation(
            "--host must be a durable DNS or mDNS hostname (IP addresses are not stable peer identities)",
        ));
    }
    Ok(host)
}

fn certificate(
    fingerprint: String,
    private_key_ref: String,
) -> std::result::Result<LocalCertificate, AtmError> {
    let fingerprint = fingerprint.parse().map_err(|error| {
        AtmError::certificate_operation(format!("invalid --fingerprint: {error}"))
    })?;
    let private_key_ref = private_key_ref.parse().map_err(|error| {
        AtmError::certificate_operation(format!("invalid --private-key-ref: {error}"))
    })?;
    Ok(LocalCertificate {
        fingerprint,
        private_key_ref,
    })
}

fn require_confirmation(confirmed: bool, operation: &str) -> Result<(), AtmError> {
    if confirmed {
        Ok(())
    } else {
        Err(AtmError::validation(format!(
            "{operation} requires explicit --yes confirmation"
        )))
    }
}

fn print_output<T: Serialize>(value: &T, json: bool) -> Result<(), AtmError> {
    println!("{}", render_output(value, json)?);
    Ok(())
}

fn render_output<T: Serialize>(value: &T, json: bool) -> Result<String, AtmError> {
    if json {
        serde_json::to_string(value)
    } else {
        serde_json::to_string_pretty(value)
    }
    .map_err(|_source| {
        AtmError::new(
            atm_storage::AtmErrorCode::SerializationFailed,
            "failed to render peer configuration output",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{certificate, render_output, require_confirmation};
    use std::sync::Mutex;

    use atm_storage::{
        AtmErrorCode, HostName, HttpsInterface, LocalCertificate, PeerConfigStore, TrustedPeer,
    };
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct PeerCli {
        #[command(flatten)]
        peer: super::PeerCommand,
    }

    #[derive(Default)]
    struct PeerConfigState {
        interfaces: Vec<HttpsInterface>,
        certificate: Option<LocalCertificate>,
        peers: Vec<TrustedPeer>,
    }

    #[derive(Default)]
    struct InMemoryPeerConfigStore(Mutex<PeerConfigState>);

    impl atm_storage::contract::sealed::Sealed for InMemoryPeerConfigStore {}

    impl PeerConfigStore for InMemoryPeerConfigStore {
        fn list_interfaces(&self) -> Result<Vec<HttpsInterface>, atm_storage::AtmError> {
            Ok(self.0.lock().expect("peer state lock").interfaces.clone())
        }

        fn save_interface(&self, interface: &HttpsInterface) -> Result<(), atm_storage::AtmError> {
            let mut state = self.0.lock().expect("peer state lock");
            state
                .interfaces
                .retain(|existing| existing.bind_addr != interface.bind_addr);
            state.interfaces.push(interface.clone());
            Ok(())
        }

        fn remove_interface(
            &self,
            bind_addr: std::net::SocketAddr,
        ) -> Result<bool, atm_storage::AtmError> {
            let mut state = self.0.lock().expect("peer state lock");
            let initial_len = state.interfaces.len();
            state
                .interfaces
                .retain(|interface| interface.bind_addr != bind_addr);
            Ok(state.interfaces.len() != initial_len)
        }

        fn local_certificate(&self) -> Result<Option<LocalCertificate>, atm_storage::AtmError> {
            Ok(self.0.lock().expect("peer state lock").certificate.clone())
        }

        fn save_local_certificate(
            &self,
            certificate: &LocalCertificate,
        ) -> Result<(), atm_storage::AtmError> {
            self.0.lock().expect("peer state lock").certificate = Some(certificate.clone());
            Ok(())
        }

        fn list_trusted_peers(&self) -> Result<Vec<TrustedPeer>, atm_storage::AtmError> {
            Ok(self.0.lock().expect("peer state lock").peers.clone())
        }

        fn trusted_peer(
            &self,
            host: &HostName,
        ) -> Result<Option<TrustedPeer>, atm_storage::AtmError> {
            Ok(self
                .0
                .lock()
                .expect("peer state lock")
                .peers
                .iter()
                .find(|peer| &peer.host == host)
                .cloned())
        }

        fn save_trusted_peer(&self, peer: &TrustedPeer) -> Result<(), atm_storage::AtmError> {
            let mut state = self.0.lock().expect("peer state lock");
            state.peers.retain(|existing| existing.host != peer.host);
            state.peers.push(peer.clone());
            Ok(())
        }

        fn remove_trusted_peer(&self, host: &HostName) -> Result<bool, atm_storage::AtmError> {
            let mut state = self.0.lock().expect("peer state lock");
            let initial_len = state.peers.len();
            state.peers.retain(|peer| &peer.host != host);
            Ok(state.peers.len() != initial_len)
        }
    }

    fn run_peer(
        store: &(dyn PeerConfigStore + Send + Sync),
        arguments: &[&str],
    ) -> Result<(), atm_storage::AtmError> {
        PeerCli::try_parse_from(arguments)
            .expect("peer command parses")
            .peer
            .run_with_store(store)
    }

    #[test]
    fn json_flag_selects_compact_machine_output() {
        let compact = render_output(&vec!["peer.example"], true).expect("compact JSON");
        let pretty = render_output(&vec!["peer.example"], false).expect("pretty JSON");
        assert_eq!(compact, "[\"peer.example\"]");
        assert!(pretty.contains('\n'));
    }

    #[test]
    fn mutations_require_explicit_confirmation() {
        assert!(require_confirmation(false, "test mutation").is_err());
        require_confirmation(true, "test mutation").expect("explicit confirmation");
    }

    #[test]
    fn parses_complete_peer_control_plane_lifecycle() {
        let commands = [
            vec!["atm", "interface", "list", "--json"],
            vec![
                "atm",
                "interface",
                "set",
                "--bind",
                "127.0.0.1:43101",
                "--advertise-host",
                "localhost",
            ],
            vec!["atm", "interface", "remove", "--bind", "127.0.0.1:43101"],
            vec!["atm", "certificate", "show", "--json"],
            vec![
                "atm",
                "certificate",
                "init",
                "--fingerprint",
                "sha256:local",
                "--private-key-ref",
                "keychain:atm",
                "--yes",
            ],
            vec!["atm", "trust", "list", "--json"],
            vec![
                "atm",
                "trust",
                "add",
                "--host",
                "peer.example",
                "--fingerprint",
                "sha256:peer",
                "--yes",
            ],
            vec![
                "atm",
                "trust",
                "replace",
                "--host",
                "peer.example",
                "--fingerprint",
                "sha256:replacement",
                "--yes",
            ],
            vec!["atm", "trust", "revoke", "--host", "peer.example", "--yes"],
        ];

        for command in commands {
            PeerCli::try_parse_from(command).expect("documented peer command must parse");
        }
    }

    #[test]
    fn certificate_input_uses_the_semantic_certificate_error() {
        let error = certificate("   ".to_string(), "keychain:atm".to_string())
            .expect_err("blank certificate fingerprint must fail");
        assert_eq!(error.code(), AtmErrorCode::CertificateOperationFailed);
    }

    #[test]
    fn peer_commands_mutate_and_read_an_isolated_durable_store() {
        let store = InMemoryPeerConfigStore::default();

        run_peer(
            &store,
            &[
                "atm",
                "interface",
                "set",
                "--bind",
                "127.0.0.1:43101",
                "--advertise-host",
                "localhost",
            ],
        )
        .expect("save interface");
        assert_eq!(store.list_interfaces().expect("list interfaces").len(), 1);
        run_peer(
            &store,
            &["atm", "interface", "remove", "--bind", "127.0.0.1:43101"],
        )
        .expect("remove interface");
        assert!(
            store
                .list_interfaces()
                .expect("list interfaces after remove")
                .is_empty()
        );

        run_peer(
            &store,
            &[
                "atm",
                "certificate",
                "init",
                "--fingerprint",
                "sha256:local",
                "--private-key-ref",
                "keychain:atm",
                "--yes",
            ],
        )
        .expect("save certificate");
        assert_eq!(
            store
                .local_certificate()
                .expect("load certificate")
                .as_ref()
                .map(|certificate| certificate.fingerprint.to_string()),
            Some("sha256:local".to_string())
        );

        run_peer(
            &store,
            &[
                "atm",
                "trust",
                "add",
                "--host",
                "peer.example",
                "--fingerprint",
                "sha256:peer-one",
                "--yes",
            ],
        )
        .expect("add peer");
        assert_eq!(store.list_trusted_peers().expect("list peers").len(), 1);
        run_peer(
            &store,
            &[
                "atm",
                "trust",
                "replace",
                "--host",
                "peer.example",
                "--fingerprint",
                "sha256:peer-two",
                "--yes",
            ],
        )
        .expect("replace peer fingerprint");
        assert_eq!(
            store
                .trusted_peer(&"peer.example".parse().expect("host"))
                .expect("read replaced peer")
                .map(|peer| peer.fingerprint.to_string()),
            Some("sha256:peer-two".to_string())
        );
        run_peer(
            &store,
            &["atm", "trust", "revoke", "--host", "peer.example", "--yes"],
        )
        .expect("revoke peer");
        assert!(
            store
                .list_trusted_peers()
                .expect("list peers after revoke")
                .is_empty()
        );
    }

    #[test]
    fn trust_add_and_replace_require_durable_peer_hostnames() {
        let store = InMemoryPeerConfigStore::default();
        for (command, host) in [("add", "192.168.128.29"), ("replace", "peer.local")] {
            let error = run_peer(
                &store,
                &[
                    "atm",
                    "trust",
                    command,
                    "--host",
                    host,
                    "--fingerprint",
                    "sha256:peer",
                    "--yes",
                ],
            )
            .expect_err("attachment-specific host must be rejected");
            assert!(error.message().contains("durable DNS hostname"));
        }
        assert!(store.list_trusted_peers().expect("list peers").is_empty());
    }

    #[test]
    fn trust_revoke_keeps_legacy_host_lookup_available() {
        let store = InMemoryPeerConfigStore::default();
        let legacy_host: HostName = "192.168.128.29".parse().expect("legacy host syntax");
        store
            .save_trusted_peer(&TrustedPeer {
                host: legacy_host,
                fingerprint: "sha256:legacy".parse().expect("fingerprint"),
                enabled: true,
                https_port: std::num::NonZeroU16::new(443).expect("non-zero port"),
            })
            .expect("seed legacy peer");

        run_peer(
            &store,
            &[
                "atm",
                "trust",
                "revoke",
                "--host",
                "192.168.128.29",
                "--yes",
            ],
        )
        .expect("legacy peer should remain revocable");
        assert!(store.list_trusted_peers().expect("list peers").is_empty());
    }

    #[test]
    fn all_confirmed_peer_mutations_fail_closed_without_yes() {
        let store = InMemoryPeerConfigStore::default();
        let commands = [
            vec![
                "atm",
                "certificate",
                "init",
                "--fingerprint",
                "sha256:local",
                "--private-key-ref",
                "keychain:atm",
            ],
            vec![
                "atm",
                "trust",
                "add",
                "--host",
                "peer.example",
                "--fingerprint",
                "sha256:peer",
            ],
            vec![
                "atm",
                "trust",
                "replace",
                "--host",
                "peer.example",
                "--fingerprint",
                "sha256:peer",
            ],
            vec!["atm", "trust", "revoke", "--host", "peer.example"],
        ];

        for command in commands {
            let error = run_peer(&store, &command).expect_err("--yes is required");
            assert!(
                error
                    .message()
                    .contains("requires explicit --yes confirmation")
            );
        }
        assert!(
            store
                .local_certificate()
                .expect("load certificate")
                .is_none()
        );
        assert!(store.list_trusted_peers().expect("list peers").is_empty());
    }
}
