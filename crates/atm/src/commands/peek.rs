use anyhow::Result;
use atm_core::read::{MAX_TIMEOUT_SECS, PeekQuery};
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
    pub fn run(self, observability: &CliObservability) -> Result<()> {
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
        let outcome = composition.peek(query)?;
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
            team_override: self.team.as_deref().map(CallerTeamOverride),
        })?;
        if let Some(timeout_secs) = self.timeout
            && timeout_secs > MAX_TIMEOUT_SECS
        {
            return Err(atm_core::error::AtmError::validation(format!(
                "timeout exceeds the {} second maximum",
                MAX_TIMEOUT_SECS
            ))
            .with_recovery("Use a timeout no greater than one hour before retrying `atm peek`.")
            .into());
        }
        let _ = self.since_last_seen;
        let selection_mode = self.selection_mode();
        let timestamp_filter = self.since.as_deref().map(parse_timestamp).transpose()?;
        PeekQuery::new(
            home_dir,
            current_dir,
            caller_context.caller_identity,
            self.target.as_deref(),
            caller_context.caller_team,
            selection_mode,
            !self.no_since_last_seen && selection_mode != ReadSelection::All,
            self.message_id.as_deref(),
            self.from.as_deref(),
            timestamp_filter,
            self.task.as_deref(),
            self.contains.as_deref(),
            self.timeout,
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
