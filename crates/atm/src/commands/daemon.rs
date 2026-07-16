use std::net::IpAddr;

use anyhow::Result;
use atm_core::error::AtmError;
use atm_daemon_bootstrap::{
    with_default_allowed_host_store, with_default_peer_interface_config_store,
    with_default_peer_security_store,
};
use atm_storage::contract::{
    AddPeerInterfaceCommand, AllowHostCommand, AllowedHostName, AllowedHostRow, PeerInterfaceKey,
    PeerInterfaceKind, PeerInterfaceRow, PeerSecurityMode, PeerSecuritySettingsRow,
    SetPeerSecurityModeCommand, TrustedPeerRow, UpdatePeerInterfaceCommand,
    UpsertTrustedPeerCommand,
};
use clap::{Args, Subcommand, ValueEnum};

use crate::commands::caller_context::{CallerContextOverrides, resolve_cli_caller_context};
use crate::observability::CliObservability;

#[derive(Debug, Args)]
pub struct DaemonCommand {
    #[command(subcommand)]
    command: DaemonSubcommand,
}

#[derive(Debug, Subcommand)]
enum DaemonSubcommand {
    Interfaces(DaemonInterfacesCommand),
    Hosts(DaemonHostsCommand),
    Security(DaemonSecurityCommand),
}

#[derive(Debug, Args)]
struct DaemonInterfacesCommand {
    #[command(subcommand)]
    command: DaemonInterfacesSubcommand,
}

#[derive(Debug, Args)]
struct DaemonHostsCommand {
    #[command(subcommand)]
    command: DaemonHostsSubcommand,
}

#[derive(Debug, Args)]
struct DaemonSecurityCommand {
    #[command(subcommand)]
    command: DaemonSecuritySubcommand,
}

#[derive(Debug, Subcommand)]
enum DaemonInterfacesSubcommand {
    Add(AddInterfaceCommand),
    Update(UpdateInterfaceCommand),
    Enable(ToggleInterfaceCommand),
    Disable(ToggleInterfaceCommand),
    Remove(RemoveInterfaceCommand),
    List(ListInterfacesCommand),
}

#[derive(Debug, Subcommand)]
enum DaemonHostsSubcommand {
    Allow(AllowHostCliCommand),
    Deny(ToggleHostCliCommand),
    Remove(RemoveHostCliCommand),
    List(ListHostsCliCommand),
}

#[derive(Debug, Subcommand)]
enum DaemonSecuritySubcommand {
    Show(ShowSecurityCommand),
    Mode(SetSecurityModeCliCommand),
    Identity(ShowSecurityIdentityCommand),
    Trust(DaemonSecurityTrustCommand),
}

#[derive(Debug, Args)]
struct DaemonSecurityTrustCommand {
    #[command(subcommand)]
    command: DaemonSecurityTrustSubcommand,
}

#[derive(Debug, Subcommand)]
enum DaemonSecurityTrustSubcommand {
    Approve(ApproveTrustedPeerCliCommand),
    Remove(RemoveTrustedPeerCliCommand),
    List(ListTrustedPeersCliCommand),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PeerInterfaceKindArg {
    Lan,
    Vpn,
    Loopback,
    Other,
}

impl From<PeerInterfaceKindArg> for PeerInterfaceKind {
    fn from(value: PeerInterfaceKindArg) -> Self {
        match value {
            PeerInterfaceKindArg::Lan => Self::Lan,
            PeerInterfaceKindArg::Vpn => Self::Vpn,
            PeerInterfaceKindArg::Loopback => Self::Loopback,
            PeerInterfaceKindArg::Other => Self::Other,
        }
    }
}

#[derive(Debug, Args)]
struct AddInterfaceCommand {
    interface_name: String,

    #[arg(long)]
    bind_addr: IpAddr,

    #[arg(long)]
    advertise_addr: IpAddr,

    #[arg(long)]
    port: u16,

    #[arg(long = "kind", value_enum)]
    interface_kind: PeerInterfaceKindArg,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct UpdateInterfaceCommand {
    interface_name: String,

    #[arg(long)]
    bind_addr: IpAddr,

    #[arg(long)]
    advertise_addr: IpAddr,

    #[arg(long)]
    port: u16,

    #[arg(long = "kind", value_enum)]
    interface_kind: Option<PeerInterfaceKindArg>,

    #[arg(long = "new-bind-addr")]
    new_bind_addr: Option<IpAddr>,

    #[arg(long = "enabled")]
    enabled: Option<bool>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ToggleInterfaceCommand {
    interface_name: String,

    #[arg(long)]
    bind_addr: IpAddr,

    #[arg(long)]
    port: u16,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RemoveInterfaceCommand {
    interface_name: String,

    #[arg(long)]
    bind_addr: IpAddr,

    #[arg(long)]
    port: u16,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ListInterfacesCommand {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AllowHostCliCommand {
    host_name: String,

    #[arg(long)]
    note: Option<String>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ToggleHostCliCommand {
    host_name: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RemoveHostCliCommand {
    host_name: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ListHostsCliCommand {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ShowSecurityCommand {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct SetSecurityModeCliCommand {
    mode: PeerSecurityModeArg,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ShowSecurityIdentityCommand {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ApproveTrustedPeerCliCommand {
    host_name: String,

    #[arg(long)]
    fingerprint: String,

    #[arg(long = "display-name")]
    display_name: Option<String>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RemoveTrustedPeerCliCommand {
    host_name: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ListTrustedPeersCliCommand {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PeerSecurityModeArg {
    SecureRequired,
    InsecureAllowed,
}

impl From<PeerSecurityModeArg> for PeerSecurityMode {
    fn from(value: PeerSecurityModeArg) -> Self {
        match value {
            PeerSecurityModeArg::SecureRequired => Self::SecureRequired,
            PeerSecurityModeArg::InsecureAllowed => Self::InsecureAllowed,
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct PeerInterfaceMutationOutcome {
    row: PeerInterfaceRow,
}

#[derive(Debug, serde::Serialize)]
struct PeerInterfaceRemoveOutcome {
    removed: bool,
    key: PeerInterfaceKey,
}

#[derive(Debug, serde::Serialize)]
struct PeerInterfaceListOutcome {
    interfaces: Vec<PeerInterfaceRow>,
}

#[derive(Debug, serde::Serialize)]
struct AllowedHostMutationOutcome {
    row: AllowedHostRow,
}

#[derive(Debug, serde::Serialize)]
struct AllowedHostRemoveOutcome {
    removed: bool,
    host_name: AllowedHostName,
}

#[derive(Debug, serde::Serialize)]
struct AllowedHostListOutcome {
    hosts: Vec<AllowedHostRow>,
}

#[derive(Debug, serde::Serialize)]
struct PeerSecurityShowOutcome {
    settings: PeerSecuritySettingsRow,
    local_identity_fingerprint_sha256: Option<String>,
    trusted_peers: Vec<TrustedPeerRow>,
}

#[derive(Debug, serde::Serialize)]
struct PeerSecurityIdentityOutcome {
    fingerprint_sha256: String,
}

#[derive(Debug, serde::Serialize)]
struct TrustedPeerMutationOutcome {
    row: TrustedPeerRow,
}

#[derive(Debug, serde::Serialize)]
struct TrustedPeerRemoveOutcome {
    removed: bool,
    host_name: AllowedHostName,
}

#[derive(Debug, serde::Serialize)]
struct TrustedPeerListOutcome {
    trusted_peers: Vec<TrustedPeerRow>,
}

impl DaemonCommand {
    pub fn run(self, _observability: &CliObservability) -> Result<()> {
        match self.command {
            DaemonSubcommand::Interfaces(command) => command.run(),
            DaemonSubcommand::Hosts(command) => command.run(),
            DaemonSubcommand::Security(command) => command.run(),
        }
    }
}

impl DaemonInterfacesCommand {
    fn run(self) -> Result<()> {
        match self.command {
            DaemonInterfacesSubcommand::Add(command) => command.run(),
            DaemonInterfacesSubcommand::Update(command) => command.run(),
            DaemonInterfacesSubcommand::Enable(command) => command.run(true),
            DaemonInterfacesSubcommand::Disable(command) => command.run(false),
            DaemonInterfacesSubcommand::Remove(command) => command.run(),
            DaemonInterfacesSubcommand::List(command) => command.run(),
        }
    }
}

impl DaemonHostsCommand {
    fn run(self) -> Result<()> {
        match self.command {
            DaemonHostsSubcommand::Allow(command) => command.run(),
            DaemonHostsSubcommand::Deny(command) => command.run(),
            DaemonHostsSubcommand::Remove(command) => command.run(),
            DaemonHostsSubcommand::List(command) => command.run(),
        }
    }
}

impl DaemonSecurityCommand {
    fn run(self) -> Result<()> {
        match self.command {
            DaemonSecuritySubcommand::Show(command) => command.run(),
            DaemonSecuritySubcommand::Mode(command) => command.run(),
            DaemonSecuritySubcommand::Identity(command) => command.run(),
            DaemonSecuritySubcommand::Trust(command) => command.run(),
        }
    }
}

impl DaemonSecurityTrustCommand {
    fn run(self) -> Result<()> {
        match self.command {
            DaemonSecurityTrustSubcommand::Approve(command) => command.run(),
            DaemonSecurityTrustSubcommand::Remove(command) => command.run(),
            DaemonSecurityTrustSubcommand::List(command) => command.run(),
        }
    }
}

impl AddInterfaceCommand {
    fn run(self) -> Result<()> {
        let configured_by = configured_by_identity()?;
        let outcome = with_default_peer_interface_config_store(|store| {
            Ok(PeerInterfaceMutationOutcome {
                row: store.add_interface(AddPeerInterfaceCommand::new(
                    self.interface_name,
                    self.bind_addr,
                    self.advertise_addr,
                    self.port,
                    self.interface_kind.into(),
                    configured_by,
                )?)?,
            })
        })?;
        print_mutation_outcome(&outcome, self.json)
    }
}

impl UpdateInterfaceCommand {
    fn run(self) -> Result<()> {
        let configured_by = configured_by_identity()?;
        let key = PeerInterfaceKey::new(self.interface_name, self.bind_addr, self.port)?;
        let current = with_default_peer_interface_config_store(|store| {
            store
                .list_interfaces()?
                .into_iter()
                .find(|row| {
                    row.interface_name == key.interface_name
                        && row.bind_addr == key.bind_addr
                        && row.port == key.port
                })
                .ok_or_else(|| missing_row_error(&key))
        })?;
        let outcome = with_default_peer_interface_config_store(|store| {
            Ok(PeerInterfaceMutationOutcome {
                row: store.update_interface(UpdatePeerInterfaceCommand::new(
                    key,
                    self.new_bind_addr.unwrap_or(current.bind_addr),
                    self.advertise_addr,
                    self.port,
                    self.interface_kind
                        .map(PeerInterfaceKind::from)
                        .unwrap_or(current.interface_kind),
                    configured_by,
                    self.enabled,
                )?)?,
            })
        })?;
        print_mutation_outcome(&outcome, self.json)
    }
}

impl ToggleInterfaceCommand {
    fn run(self, enabled: bool) -> Result<()> {
        let key = PeerInterfaceKey::new(self.interface_name, self.bind_addr, self.port)?;
        let outcome = with_default_peer_interface_config_store(|store| {
            Ok(PeerInterfaceMutationOutcome {
                row: store.set_interface_enabled(&key, enabled)?,
            })
        })?;
        print_mutation_outcome(&outcome, self.json)
    }
}

impl RemoveInterfaceCommand {
    fn run(self) -> Result<()> {
        let key = PeerInterfaceKey::new(self.interface_name, self.bind_addr, self.port)?;
        let removed =
            with_default_peer_interface_config_store(|store| store.remove_interface(&key))?;
        let outcome = PeerInterfaceRemoveOutcome { removed, key };
        if self.json {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        } else if outcome.removed {
            println!(
                "Removed daemon interface {} at {}:{}",
                outcome.key.interface_name, outcome.key.bind_addr, outcome.key.port
            );
        } else {
            println!(
                "No daemon interface row matched {} at {}:{}",
                outcome.key.interface_name, outcome.key.bind_addr, outcome.key.port
            );
        }
        Ok(())
    }
}

impl ListInterfacesCommand {
    fn run(self) -> Result<()> {
        let outcome = with_default_peer_interface_config_store(|store| {
            Ok(PeerInterfaceListOutcome {
                interfaces: store.list_interfaces()?,
            })
        })?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
            return Ok(());
        }
        if outcome.interfaces.is_empty() {
            println!("No daemon peer interfaces configured");
            return Ok(());
        }
        for row in &outcome.interfaces {
            println!(
                "{} {} bind={}:{} advertise={}:{} enabled={} stale_at={} last_bound_at={} last_bind_error={}",
                row.interface_name,
                row.interface_kind,
                row.bind_addr,
                row.port,
                row.advertise_addr,
                row.port,
                row.enabled,
                row.stale_at
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "-".to_string()),
                row.last_bound_at
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "-".to_string()),
                row.last_bind_error.as_deref().unwrap_or("-")
            );
        }
        Ok(())
    }
}

impl AllowHostCliCommand {
    fn run(self) -> Result<()> {
        let configured_by = configured_by_identity()?;
        let outcome = with_default_allowed_host_store(|store| {
            Ok(AllowedHostMutationOutcome {
                row: store.allow_host(AllowHostCommand::new(
                    self.host_name,
                    configured_by,
                    self.note,
                )?)?,
            })
        })?;
        print_allowed_host_mutation_outcome(&outcome, self.json)
    }
}

impl ToggleHostCliCommand {
    fn run(self) -> Result<()> {
        let host_name = self.host_name.parse::<AllowedHostName>()?;
        let outcome = with_default_allowed_host_store(|store| {
            Ok(AllowedHostMutationOutcome {
                row: store.deny_host(&host_name)?,
            })
        })?;
        print_allowed_host_mutation_outcome(&outcome, self.json)
    }
}

impl RemoveHostCliCommand {
    fn run(self) -> Result<()> {
        let host_name = self.host_name.parse::<AllowedHostName>()?;
        with_default_allowed_host_store(|store| store.remove_host(&host_name))?;
        let outcome = AllowedHostRemoveOutcome {
            removed: true,
            host_name,
        };
        if self.json {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        } else {
            println!("Removed daemon allowed host {}", outcome.host_name);
        }
        Ok(())
    }
}

impl ListHostsCliCommand {
    fn run(self) -> Result<()> {
        let outcome = with_default_allowed_host_store(|store| {
            Ok(AllowedHostListOutcome {
                hosts: store.list_hosts()?,
            })
        })?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
            return Ok(());
        }
        if outcome.hosts.is_empty() {
            println!("No daemon allowed hosts configured");
            return Ok(());
        }
        for row in &outcome.hosts {
            println!(
                "{} enabled={} disabled_at={} note={}",
                row.host_name,
                row.enabled,
                row.disabled_at
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "-".to_string()),
                row.note.as_deref().unwrap_or("-"),
            );
        }
        Ok(())
    }
}

impl ShowSecurityCommand {
    fn run(self) -> Result<()> {
        let outcome = with_default_peer_security_store(|store| {
            let settings = store.load_security_settings()?;
            let local_identity_fingerprint_sha256 = store
                .load_local_identity()?
                .map(|row| row.fingerprint_sha256);
            let trusted_peers = store.list_trusted_peers()?;
            Ok(PeerSecurityShowOutcome {
                settings,
                local_identity_fingerprint_sha256,
                trusted_peers,
            })
        })?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
            return Ok(());
        }
        println!("mode={}", outcome.settings.mode);
        println!(
            "local_identity_fingerprint_sha256={}",
            outcome
                .local_identity_fingerprint_sha256
                .as_deref()
                .unwrap_or("-")
        );
        println!("trusted_peers={}", outcome.trusted_peers.len());
        Ok(())
    }
}

impl SetSecurityModeCliCommand {
    fn run(self) -> Result<()> {
        let configured_by = configured_by_identity()?;
        let outcome = with_default_peer_security_store(|store| {
            store.set_security_mode(SetPeerSecurityModeCommand::new(
                self.mode.into(),
                configured_by,
            )?)
        })?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        } else {
            println!("Configured daemon peer security mode {}", outcome.mode);
        }
        Ok(())
    }
}

impl ShowSecurityIdentityCommand {
    fn run(self) -> Result<()> {
        let outcome = with_default_peer_security_store(|store| {
            let row = store.load_or_create_local_identity()?;
            Ok(PeerSecurityIdentityOutcome {
                fingerprint_sha256: row.fingerprint_sha256,
            })
        })?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        } else {
            println!(
                "Local daemon peer identity fingerprint_sha256={}",
                outcome.fingerprint_sha256
            );
        }
        Ok(())
    }
}

impl ApproveTrustedPeerCliCommand {
    fn run(self) -> Result<()> {
        let configured_by = configured_by_identity()?;
        let outcome = with_default_peer_security_store(|store| {
            Ok(TrustedPeerMutationOutcome {
                row: store.upsert_trusted_peer(UpsertTrustedPeerCommand::new(
                    self.host_name,
                    self.fingerprint,
                    self.display_name,
                    configured_by,
                )?)?,
            })
        })?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        } else {
            println!(
                "Approved trusted peer {} fingerprint_sha256={}",
                outcome.row.host_name, outcome.row.fingerprint_sha256
            );
        }
        Ok(())
    }
}

impl RemoveTrustedPeerCliCommand {
    fn run(self) -> Result<()> {
        let host_name = self.host_name.parse::<AllowedHostName>()?;
        let removed =
            with_default_peer_security_store(|store| store.remove_trusted_peer(&host_name))?;
        let outcome = TrustedPeerRemoveOutcome { removed, host_name };
        if self.json {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        } else if outcome.removed {
            println!("Removed trusted peer {}", outcome.host_name);
        } else {
            println!("No trusted peer row matched {}", outcome.host_name);
        }
        Ok(())
    }
}

impl ListTrustedPeersCliCommand {
    fn run(self) -> Result<()> {
        let outcome = with_default_peer_security_store(|store| {
            Ok(TrustedPeerListOutcome {
                trusted_peers: store.list_trusted_peers()?,
            })
        })?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
            return Ok(());
        }
        if outcome.trusted_peers.is_empty() {
            println!("No daemon trusted peers configured");
            return Ok(());
        }
        for row in &outcome.trusted_peers {
            println!(
                "{} fingerprint_sha256={} display_name={}",
                row.host_name,
                row.fingerprint_sha256,
                row.display_name.as_deref().unwrap_or("-"),
            );
        }
        Ok(())
    }
}

fn configured_by_identity() -> Result<String> {
    let context = resolve_cli_caller_context(CallerContextOverrides::default())?;
    Ok(format!(
        "{}@{}",
        context.caller_identity, context.caller_team
    ))
}

fn print_mutation_outcome(outcome: &PeerInterfaceMutationOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        let row = &outcome.row;
        println!(
            "Configured daemon interface {} bind={}:{} advertise={}:{} kind={} enabled={}",
            row.interface_name,
            row.bind_addr,
            row.port,
            row.advertise_addr,
            row.port,
            row.interface_kind,
            row.enabled
        );
    }
    Ok(())
}

fn print_allowed_host_mutation_outcome(
    outcome: &AllowedHostMutationOutcome,
    json: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        let row = &outcome.row;
        println!(
            "Configured daemon allowed host {} enabled={} disabled_at={} note={}",
            row.host_name,
            row.enabled,
            row.disabled_at
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "-".to_string()),
            row.note.as_deref().unwrap_or("-"),
        );
    }
    Ok(())
}

fn missing_row_error(key: &PeerInterfaceKey) -> AtmError {
    AtmError::validation(format!(
        "no daemon peer interface row matched {} at {}:{}",
        key.interface_name, key.bind_addr, key.port
    ))
    .with_recovery("Use `atm daemon interfaces list` to inspect the configured rows before retrying the update.")
}
