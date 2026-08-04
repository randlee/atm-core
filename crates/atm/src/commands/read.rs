use anyhow::Result;
use atm_core::read::{MAX_TIMEOUT_SECS, ReadQuery};
use atm_core::types::ReadSelection;
use clap::Args;

use crate::commands::caller_context::{
    CallerChatIdOverride, CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride,
    resolve_cli_mutation_caller_context, resolve_cli_mutation_caller_context_with_overrides,
};
use crate::commands::util::parse_timestamp;
use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// Read one ATM mailbox message and optionally update read state.
pub struct ReadCommand {
    #[arg(long)]
    team: Option<String>,

    #[arg(long = "chat-id", conflicts_with = "actor")]
    chat_id: Option<String>,

    #[arg(long = "as")]
    actor: Option<String>,

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
}

impl ReadCommand {
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let warnings = self.deprecation_warnings();
        let (home_dir, current_dir) = resolve_command_runtime_context("read")?;
        let json = self.json;
        let query = self.build_query(home_dir.clone(), current_dir.clone())?;
        let composition = CliComposition::bootstrap(
            "read",
            observability,
            InvocationDir::new(&current_dir),
            AtmHomePath::new(&home_dir),
        )?;
        let outcome = composition.receive(query)?;
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
    ) -> Result<ReadQuery> {
        let caller_context = if self.actor.is_some() || self.chat_id.is_some() {
            resolve_cli_mutation_caller_context_with_overrides(CallerContextOverrides {
                identity_override: self.actor.as_deref().map(CallerIdentityOverride),
                chat_id_override: self.chat_id.as_deref().map(CallerChatIdOverride),
                team_override: self.team.as_deref().map(CallerTeamOverride),
            })?
        } else {
            resolve_cli_mutation_caller_context(self.team.as_deref().map(CallerTeamOverride))?
        };
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
        ReadQuery::new(
            home_dir,
            current_dir,
            caller_context.caller_identity,
            None,
            caller_context.caller_team,
            selection_mode,
            !self.no_since_last_seen && selection_mode != ReadSelection::All,
            true,
            self.message_id.as_deref(),
            self.from.as_deref(),
            timestamp_filter,
            self.task.as_deref(),
            self.contains.as_deref(),
            self.timeout,
        )
        .map(|query| {
            query
                .with_caller_chat_id(caller_context.caller_chat_id)
                .with_activity_observation(caller_context.activity_observation)
        })
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
                "warning: `atm read --unread-only` is deprecated; use `atm read --unread`."
                    .to_string(),
            );
        }
        if self.pending_ack_only {
            warnings.push(
                "warning: `atm read --pending-ack-only` is deprecated; use `atm read --pending-ack`."
                    .to_string(),
            );
        }
        if self.history {
            warnings.push(
                "warning: `atm read --history` is deprecated; use `atm read --all`.".to_string(),
            );
        }
        if self.since_last_seen {
            warnings.push(
                "note: `atm read --since-last-seen` remains supported as an explicit restatement of the default seen-state behavior."
                    .to_string(),
            );
        }
        warnings
    }
}

#[cfg(test)]
mod tests {
    use atm_core::test_support::{EnvGuard, TEST_SENDER, TEST_TEAM};
    use atm_core::types::ReadSelection;
    use clap::Parser;
    use serial_test::serial;

    use super::ReadCommand;

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
    fn build_query_propagates_filters_and_exact_message_id() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("sender-a")),
            ("ATM_TEAM", Some("env-team")),
        ]);
        let mut command = base_command();
        command.team = Some("override-team".to_string());
        command.message_id = Some("01KRFK5QTF2R6NRS3Q0F8Z9K0S".to_string());
        command.no_since_last_seen = true;
        command.timeout = Some(9);

        let query = command.build_query(".".into(), ".".into()).expect("query");

        assert_eq!(query.selection_mode(), ReadSelection::Actionable);
        assert!(!query.seen_state_filter());
        assert!(query.seen_state_update());
        assert_eq!(query.timeout_secs(), Some(9));
        assert!(query.message_id_filter().is_some());
    }

    #[test]
    fn deprecation_warnings_cover_legacy_aliases() {
        let mut command = base_command();
        command.unread_only = true;
        command.pending_ack_only = true;
        command.history = true;
        command.since_last_seen = true;

        let warnings = command.deprecation_warnings();
        assert_eq!(warnings.len(), 4);
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

        assert_eq!(
            query.team_override().map(|team| team.as_str()),
            Some("env-team")
        );
    }

    #[test]
    #[serial(env)]
    fn build_query_prefers_cli_overrides_over_environment() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("env-sender")),
            ("ATM_TEAM", Some("env-team")),
        ]);
        let mut command = base_command();
        command.team = Some("override-team".to_string());

        let query = command.build_query(".".into(), ".".into()).expect("query");

        assert_eq!(
            query.team_override().map(|team| team.as_str()),
            Some("override-team")
        );
    }

    #[test]
    #[serial(env)]
    fn chat_id_and_equivalent_as_construct_the_same_read_caller() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let mut shorthand = base_command();
        shorthand.chat_id = Some("1234".to_string());
        let mut explicit = base_command();
        explicit.actor = Some(format!("{TEST_SENDER}:1234"));

        let shorthand = shorthand
            .build_query(".".into(), ".".into())
            .expect("shorthand query");
        let explicit = explicit
            .build_query(".".into(), ".".into())
            .expect("explicit query");

        assert_eq!(shorthand.caller_chat_id(), explicit.caller_chat_id());
        assert_eq!(shorthand.caller_chat_id().unwrap().as_str(), "1234");
    }

    #[test]
    #[serial(env)]
    fn read_rejects_as_for_a_different_base_agent() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let mut command = base_command();
        command.actor = Some(format!("{TEST_SENDER}-other:1234"));

        let error = command
            .build_query(".".into(), ".".into())
            .expect_err("different agent must be rejected");
        assert!(error.to_string().contains("same base agent"));
    }

    #[test]
    fn cli_rejects_positional_mailbox_target_for_owner_only_read() {
        let error = crate::commands::Cli::try_parse_from(["atm", "read", "recipient@test-team"])
            .expect_err("owner-only read must reject positional target");

        let rendered = error.to_string();
        assert!(
            rendered.contains("unexpected argument 'recipient@test-team'")
                || rendered.contains("unexpected argument"),
            "{rendered}"
        );
    }

    fn base_command() -> ReadCommand {
        ReadCommand {
            team: None,
            chat_id: None,
            actor: None,
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
        }
    }
}
