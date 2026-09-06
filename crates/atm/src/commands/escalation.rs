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
        let (target, json, action) = self.prepare()?;
        let assembly = assemble_default_runtime()?;
        let store = assembly.service_runtime.task_store()?;
        execute(&*store, target, json, action)
    }

    fn prepare(self) -> Result<(atm_storage::EscalationScope, bool, Action)> {
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
        let action = match action {
            Action::Add(address) => Action::Add(escalation_admin::validate_address(&address)?),
            action => action,
        };
        Ok((target, json, action))
    }
}

enum Action {
    Add(String),
    Remove(String),
    List,
}

fn execute(
    store: &(dyn atm_core::boundary::TaskStore + Send + Sync),
    target: atm_storage::EscalationScope,
    json: bool,
    action: Action,
) -> Result<()> {
    match action {
        Action::Add(address) => {
            let inserted =
                store.add_escalation_recipient(&target, &address, IsoTimestamp::now())?;
            print_mutation("add", &target, &address, inserted, json)?;
        }
        Action::Remove(address) => {
            let removed = escalation_admin::remove(store, &target, &address)?;
            print_mutation("remove", &target, &address, removed, json)?;
        }
        Action::List => {
            let recipients = escalation_admin::list(store, &target)?;
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

#[cfg(test)]
mod tests {
    use super::{
        Action, EscalationCommand, EscalationSubcommand, ListCommand, RecipientCommand, execute,
    };
    use atm_core::escalation_admin;
    use atm_runtime_test_support::open_isolated_sqlite_boundary;

    const TEST_TEAM: &str = "ax6-escalation-command-test";
    const TEST_RECIPIENT: &str = "ops@ax6-escalation-command-test";

    fn command_action(command: EscalationCommand) -> (atm_storage::EscalationScope, bool, Action) {
        command.prepare().expect("valid escalation command")
    }

    #[test]
    fn round_trip_uses_an_injected_isolated_task_store() {
        let root = tempfile::tempdir().expect("tempdir");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("isolated runtime");
        let store = assembly.service_runtime.task_store().expect("task store");

        let (target, json, action) = command_action(EscalationCommand {
            command: EscalationSubcommand::Add(RecipientCommand {
                address: " ops@ax6-escalation-command-test ".to_owned(),
                team: Some(TEST_TEAM.to_owned()),
                json: false,
            }),
        });
        execute(store.as_ref(), target.clone(), json, action).expect("add recipient");
        assert_eq!(
            escalation_admin::list(store.as_ref(), &target).expect("stored recipients"),
            vec![TEST_RECIPIENT]
        );

        let (list_target, json, action) = command_action(EscalationCommand {
            command: EscalationSubcommand::List(ListCommand {
                team: Some(TEST_TEAM.to_owned()),
                json: false,
            }),
        });
        execute(store.as_ref(), list_target.clone(), json, action).expect("list recipient");
        assert_eq!(
            escalation_admin::list(store.as_ref(), &list_target).expect("listed recipients"),
            vec![TEST_RECIPIENT]
        );

        let (remove_target, json, action) = command_action(EscalationCommand {
            command: EscalationSubcommand::Remove(RecipientCommand {
                address: TEST_RECIPIENT.to_owned(),
                team: Some(TEST_TEAM.to_owned()),
                json: false,
            }),
        });
        execute(store.as_ref(), remove_target.clone(), json, action).expect("remove recipient");
        assert!(
            escalation_admin::list(store.as_ref(), &remove_target)
                .expect("remaining recipients")
                .is_empty()
        );
    }
}
