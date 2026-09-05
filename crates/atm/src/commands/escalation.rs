use anyhow::Result;
use atm_core::escalation_admin;
use atm_core::types::IsoTimestamp;
use atm_daemon_bootstrap::assemble_default_runtime;
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
/// Manage daemon-wide and per-team escalation recipients.
pub struct EscalationCommand {
    #[command(subcommand)]
    command: EscalationSubcommand,
}

#[derive(Debug, Subcommand)]
enum EscalationSubcommand {
    Add(RecipientCommand),
    Remove(RecipientCommand),
    List(ListCommand),
}

#[derive(Debug, Args)]
struct RecipientCommand {
    address: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ListCommand {
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    json: bool,
}

impl EscalationCommand {
    pub async fn run(self) -> Result<()> {
        let (target, json, action) = match self.command {
            EscalationSubcommand::Add(command) => (
                escalation_admin::scope(command.team.as_deref())?,
                command.json,
                Action::Add(command.address),
            ),
            EscalationSubcommand::Remove(command) => (
                escalation_admin::scope(command.team.as_deref())?,
                command.json,
                Action::Remove(command.address),
            ),
            EscalationSubcommand::List(command) => (
                escalation_admin::scope(command.team.as_deref())?,
                command.json,
                Action::List,
            ),
        };
        let assembly = assemble_default_runtime()?;
        let store = assembly.service_runtime.task_store()?;
        match action {
            Action::Add(address) => {
                let inserted =
                    escalation_admin::add(store.as_ref(), &target, &address, IsoTimestamp::now())?;
                print_mutation("add", &target, &address, inserted, json)?;
            }
            Action::Remove(address) => {
                let removed = escalation_admin::remove(store.as_ref(), &target, &address)?;
                print_mutation("remove", &target, &address, removed, json)?;
            }
            Action::List => {
                let recipients = escalation_admin::list(store.as_ref(), &target)?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "scope": escalation_admin::scope_label(&target),
                            "recipients": recipients,
                        })
                    );
                } else {
                    for recipient in recipients {
                        println!("{recipient}");
                    }
                }
            }
        }
        Ok(())
    }
}

enum Action {
    Add(String),
    Remove(String),
    List,
}

fn print_mutation(
    action: &str,
    scope: &atm_storage::EscalationScope,
    address: &str,
    changed: bool,
    json: bool,
) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "action": action,
                "scope": escalation_admin::scope_label(scope),
                "address": address,
                "changed": changed,
            })
        );
    } else {
        println!(
            "{} {} {}",
            action,
            address,
            if changed { "ok" } else { "unchanged" }
        );
    }
    Ok(())
}
