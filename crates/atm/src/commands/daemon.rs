use std::net::IpAddr;

use anyhow::Result;
use atm_core::error::AtmError;
use atm_runtime::{
    AddPeerInterfaceCommand, AllowHostCommand, AllowedHostName, AllowedHostRow, PeerInterfaceKey,
    PeerInterfaceKind, PeerInterfaceRow, UpdatePeerInterfaceCommand,
    with_default_allowed_host_store, with_default_peer_interface_config_store,
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

impl DaemonCommand {
    pub fn run(self, _observability: &CliObservability) -> Result<()> {
        match self.command {
            DaemonSubcommand::Interfaces(command) => command.run(),
            DaemonSubcommand::Hosts(command) => command.run(),
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
