use anyhow::Result;
use atm_core::read::{MAX_TIMEOUT_SECS, MailboxQueryFilters, PeekQuery};
use atm_core::types::ReadSelection;
use clap::Args;

use crate::commands::caller_context::{
    CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride,
    resolve_cli_inspection_caller_context,
};
use crate::commands::util::parse_timestamp;
use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// Inspect one ATM mailbox message without mutating mailbox state.
pub struct PeekCommand {
    target: Option<String>,

    #[arg(long)]
    team: Option<String>,

    #[arg(long, conflicts_with_all = ["unread", "unread_only", "pending_ack", "pending_ack_only"])]
    all: bool,

    #[arg(long, conflicts_with_all = ["pending_ack", "pending_ack_only", "all"])]
    unread: bool,

    #[arg(long = "unread-only", conflicts_with_all = ["pending_ack", "pending_ack_only", "all"])]
    unread_only: bool,

    #[arg(long = "pending-ack", conflicts_with_all = ["unread", "unread_only", "all"])]
    pending_ack: bool,

    #[arg(long = "pending-ack-only", conflicts_with_all = ["unread", "unread_only", "all"])]
    pending_ack_only: bool,

    #[arg(long, conflicts_with_all = ["message_id", "task", "contains", "from", "since"])]
    history: bool,

    #[arg(long = "message-id", conflicts_with_all = ["task", "contains", "from", "since"])]
    message_id: Option<String>,

    #[arg(long)]
    task: Option<String>,

    #[arg(long)]
    contains: Option<String>,

    #[arg(long)]
    since_last_seen: bool,

    #[arg(long = "no-since-last-seen", default_value_t = false)]
    no_since_last_seen: bool,

    #[arg(long)]
    since: Option<String>,

    #[arg(long)]
    from: Option<String>,

    #[arg(long)]
    json: bool,

    #[arg(long)]
    timeout: Option<u64>,

    #[arg(long = "as")]
    actor: Option<String>,
}

impl PeekCommand {
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        let warnings = self.deprecation_warnings();
        let (home_dir, current_dir) = resolve_command_runtime_context("peek")?;
        let json = self.json;
        let query = self.build_query(home_dir.clone(), current_dir.clone())?;
        let composition = CliComposition::bootstrap(
            "peek",
            observability,
            InvocationDir::new(&current_dir),
            AtmHomePath::new(&home_dir),
        )?;
        let outcome = composition.peek(query).await?;
        output::print_read_result(&outcome, json)?;
        for warning in warnings {
            eprintln!("{warning}");
        }
        Ok(())
    }

    fn build_query(
        &self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<PeekQuery> {
        let caller_context = resolve_cli_inspection_caller_context(CallerContextOverrides {
            identity_override: self.actor.as_deref().map(CallerIdentityOverride),
            chat_id_override: None,
            team_override: self.team.as_deref().map(CallerTeamOverride),
        })?;
        if let Some(timeout_secs) = self.timeout
            && timeout_secs > MAX_TIMEOUT_SECS
        {
            return Err(atm_core::error::AtmError::validation(format!(
                "timeout exceeds the {} second maximum",
                MAX_TIMEOUT_SECS
            ))
            .into());
        }
        let _ = self.since_last_seen;
        let selection_mode = self.selection_mode();
        let timestamp_filter = self.since.as_deref().map(parse_timestamp).transpose()?;
        let filters = MailboxQueryFilters::default()
            .target_address_opt(self.target.as_deref())
            .message_id_opt(self.message_id.as_deref())
            .sender_opt(self.from.as_deref())
            .timestamp_opt(timestamp_filter)
            .task_opt(self.task.as_deref())
            .contains_opt(self.contains.as_deref())
            .timeout_secs_opt(self.timeout);
        PeekQuery::from_filters(
            home_dir,
            current_dir,
            caller_context.caller_identity,
            caller_context.caller_team,
            selection_mode,
            !self.no_since_last_seen && selection_mode != ReadSelection::All,
            filters,
        )
        .map_err(Into::into)
    }

    fn selection_mode(&self) -> ReadSelection {
        if self.all || self.history {
            ReadSelection::All
        } else if self.unread || self.unread_only {
            ReadSelection::Unread
        } else if self.pending_ack || self.pending_ack_only {
            ReadSelection::PendingAck
        } else {
            ReadSelection::Actionable
        }
    }

    fn deprecation_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.unread_only {
            warnings.push(
                "warning: `atm peek --unread-only` is deprecated; use `atm peek --unread`."
                    .to_string(),
            );
        }
        if self.pending_ack_only {
            warnings.push(
                "warning: `atm peek --pending-ack-only` is deprecated; use `atm peek --pending-ack`."
                    .to_string(),
            );
        }
        if self.history {
            warnings.push(
                "warning: `atm peek --history` is deprecated; use `atm peek --all`.".to_string(),
            );
        }
        if self.since_last_seen {
            warnings.push(
                "note: `atm peek --since-last-seen` remains supported as an explicit restatement of the default seen-state behavior."
                    .to_string(),
            );
        }
        warnings
    }
}

#[cfg(test)]
mod tests {
    use atm_core::test_support::EnvGuard;
    use atm_core::test_support::{ROLE_TEAM_LEAD, TEST_TEAM};
    use atm_core::types::ReadSelection;
    use serde_json::json;
    use serial_test::serial;

    use super::PeekCommand;

    #[test]
    fn selection_mode_maps_cli_flags_to_expected_bucket() {
        let mut command = base_command();
        command.all = true;
        assert_eq!(command.selection_mode(), ReadSelection::All);

        let mut command = base_command();
        command.unread = true;
        assert_eq!(command.selection_mode(), ReadSelection::Unread);

        let mut command = base_command();
        command.pending_ack = true;
        assert_eq!(command.selection_mode(), ReadSelection::PendingAck);

        let command = base_command();
        assert_eq!(command.selection_mode(), ReadSelection::Actionable);
    }

    #[test]
    #[serial(env)]
    fn build_query_uses_environment_when_overrides_are_absent() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("sender-a")),
            ("ATM_TEAM", Some("env-team")),
        ]);

        let query = base_command()
            .build_query(".".into(), ".".into())
            .expect("query");
        let query_json = serde_json::to_value(&query).expect("serialized query");

        assert_eq!(query_json["caller_identity"], json!("sender-a"));
        assert_eq!(query_json["caller_team"], json!("env-team"));
    }

    #[test]
    #[serial(env)]
    fn build_query_prefers_cli_as_overrides_for_cross_agent_peek() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("env-sender")),
            ("ATM_TEAM", Some("env-team")),
        ]);
        let mut command = base_command();
        command.target = Some("recipient-a@test-team".to_string());
        command.team = Some("override-team".to_string());
        command.actor = Some(ROLE_TEAM_LEAD.to_string());
        command.message_id = Some("01KRFK5QTF2R6NRS3Q0F8Z9K0S".to_string());
        command.all = true;

        let query = command.build_query(".".into(), ".".into()).expect("query");
        let query_json = serde_json::to_value(&query).expect("serialized query");

        assert_eq!(query_json["caller_identity"], json!(ROLE_TEAM_LEAD));
        assert_eq!(query_json["caller_team"], json!("override-team"));
        assert_eq!(
            query_json["mailbox"]["target_address"],
            json!({
                "agent": "recipient-a",
                "team": "test-team"
            })
        );
        assert_eq!(query_json["mailbox"]["selection_mode"], json!("all"));
        assert_eq!(query_json["mailbox"]["seen_state_filter"], json!(false));
        assert_eq!(
            query_json["mailbox"]["message_id_filter"],
            json!("01KRFK5QTF2R6NRS3Q0F8Z9K0S")
        );
    }

    #[test]
    #[serial(env)]
    fn build_query_accepts_as_without_ambient_identity() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", None),
            ("ATM_CHAT_ID", None),
            ("ATM_TEAM", None),
        ]);
        let mut command = base_command();
        command.actor = Some(ROLE_TEAM_LEAD.to_string());
        command.team = Some(TEST_TEAM.to_string());

        let query = command.build_query(".".into(), ".".into()).expect("query");
        let query_json = serde_json::to_value(&query).expect("serialized query");

        assert_eq!(query_json["caller_identity"], json!(ROLE_TEAM_LEAD));
        assert_eq!(query_json["caller_team"], json!(TEST_TEAM));
    }

    fn base_command() -> PeekCommand {
        PeekCommand {
            target: None,
            team: None,
            all: false,
            unread: false,
            unread_only: false,
            pending_ack: false,
            pending_ack_only: false,
            history: false,
            message_id: None,
            task: None,
            contains: None,
            since_last_seen: false,
            no_since_last_seen: false,
            since: None,
            from: None,
            json: false,
            timeout: None,
            actor: None,
        }
    }
}
