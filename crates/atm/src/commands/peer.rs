use anyhow::{Context, Result, bail};
use atm_daemon_bootstrap::with_default_peer_config_store;
use atm_storage::{HostName, HttpsInterface, LocalCertificate, TrustedPeer};
use clap::{Args, Subcommand};
use serde::Serialize;

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
        #[arg(long)]
        yes: bool,
    },
    Replace {
        #[arg(long)]
        host: String,
        #[arg(long)]
        fingerprint: String,
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
    pub fn run(self, _observability: &CliObservability) -> Result<()> {
        match self.command {
            PeerSubcommand::Interface(command) => command.run(),
            PeerSubcommand::Certificate(command) => command.run(),
            PeerSubcommand::Trust(command) => command.run(),
        }
    }
}

impl InterfaceCommand {
    fn run(self) -> Result<()> {
        match self.command {
            InterfaceSubcommand::List { json } => {
                let interfaces = with_default_peer_config_store(|store| store.list_interfaces())?;
                print_output(&interfaces, json)
            }
            InterfaceSubcommand::Set {
                bind,
                advertise_host,
                enabled,
            } => {
                let interface = HttpsInterface {
                    bind_addr: bind.parse().context("invalid --bind socket address")?,
                    advertise_host: advertise_host.parse().context("invalid --advertise-host")?,
                    enabled,
                };
                with_default_peer_config_store(|store| store.save_interface(&interface))?;
                println!("saved HTTPS interface {}", interface.bind_addr);
                Ok(())
            }
            InterfaceSubcommand::Remove { bind } => {
                let bind = bind.parse().context("invalid --bind socket address")?;
                let removed = with_default_peer_config_store(|store| store.remove_interface(bind))?;
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
    fn run(self) -> Result<()> {
        match self.command {
            CertificateSubcommand::Show { json } => {
                // The record has only a key reference, never key material.
                let certificate =
                    with_default_peer_config_store(|store| store.local_certificate())?;
                print_output(&certificate, json)
            }
            CertificateSubcommand::Init {
                fingerprint,
                private_key_ref,
                yes,
            } => {
                require_confirmation(yes, "initializing the local certificate")?;
                let certificate = LocalCertificate {
                    fingerprint,
                    private_key_ref,
                };
                with_default_peer_config_store(|store| store.save_local_certificate(&certificate))?;
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
    fn run(self) -> Result<()> {
        match self.command {
            TrustSubcommand::List { json } => {
                let peers = with_default_peer_config_store(|store| store.list_trusted_peers())?;
                print_output(&peers, json)
            }
            TrustSubcommand::Add {
                host,
                fingerprint,
                yes,
            } => {
                require_confirmation(yes, "adding a trusted peer")?;
                let peer = peer(host, fingerprint)?;
                with_default_peer_config_store(|store| {
                    if store.trusted_peer(&peer.host)?.is_some() {
                        return Err(atm_storage::AtmError::validation(
                            "trusted peer already exists; use `atm peer trust replace --yes`",
                        ));
                    }
                    store.save_trusted_peer(&peer)
                })?;
                println!("added trusted peer {}", peer.host);
                Ok(())
            }
            TrustSubcommand::Replace {
                host,
                fingerprint,
                yes,
            } => {
                require_confirmation(yes, "replacing a trusted-peer fingerprint")?;
                let peer = peer(host, fingerprint)?;
                with_default_peer_config_store(|store| store.save_trusted_peer(&peer))?;
                println!("replaced trusted peer {}", peer.host);
                Ok(())
            }
            TrustSubcommand::Revoke { host, yes } => {
                require_confirmation(yes, "revoking a trusted peer")?;
                let host: HostName = host.parse().context("invalid --host")?;
                let removed =
                    with_default_peer_config_store(|store| store.remove_trusted_peer(&host))?;
                println!(
                    "{} trusted peer {}",
                    if removed { "revoked" } else { "no" },
                    host
                );
                Ok(())
            }
        }
    }
}

fn peer(host: String, fingerprint: String) -> Result<TrustedPeer> {
    if fingerprint.trim().is_empty() {
        bail!("--fingerprint must not be blank");
    }
    Ok(TrustedPeer {
        host: host.parse().context("invalid --host")?,
        fingerprint,
        enabled: true,
    })
}

fn require_confirmation(confirmed: bool, operation: &str) -> Result<()> {
    if confirmed {
        Ok(())
    } else {
        bail!("{operation} requires explicit --yes confirmation")
    }
}

fn print_output<T: Serialize>(value: &T, _json: bool) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
