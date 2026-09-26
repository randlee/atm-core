//! `atm task history`: the past N tasks for a team, open and completed.
//!
//! Every other `task` view (`list`, `events`) filters completed rows out at
//! some layer, so once a task closes it is only reachable by `atm task
//! events <id>` if the caller still has the id. This command answers "what
//! were the past N tasks" without one.

use std::path::PathBuf;

use anyhow::Result;
use atm_core::caller_context::CallerContext;
use atm_core::list::{ListQuery, TaskLedgerQuery};
use atm_core::protocol::HttpApiVersion;
use atm_core::types::{AgentName, ReadSelection, TaskId};
use atm_storage::{TaskCloseOutcome, TaskEventKind, TaskEventRow, TaskRow};
use chrono::NaiveDate;
use clap::Args;

use super::{
    CallerArgs, composition, preflight_daemon_api, render_task_event_line, resolve_context,
};
use crate::composition::CliComposition;
use crate::composition::resolve_command_runtime_context;
use crate::observability::CliObservability;

/// Default number of past tasks shown when `--limit` is not given.
///
/// Chosen to fit one screen for the common "what did the team do today"
/// question; `--limit` widens it for a full-day review.
const DEFAULT_HISTORY_LIMIT: usize = 10;

#[derive(Debug, Args)]
pub(crate) struct TaskHistoryCommand {
    #[arg(long)]
    member: Option<AgentName>,
    #[arg(long, value_name = "N")]
    limit: Option<usize>,
    #[arg(long)]
    events: bool,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

/// One task's ledger-derived history row: the stored [`TaskRow`] plus the
/// started/closed timestamps and outcome recovered from its event ledger.
struct HistoryRow {
    row: TaskRow,
    started_at: Option<atm_storage::IsoTimestamp>,
    closed_at: Option<atm_storage::IsoTimestamp>,
    outcome: &'static str,
    events: Vec<TaskEventRow>,
}

impl HistoryRow {
    fn from_row_and_events(row: TaskRow, events: Vec<TaskEventRow>) -> Self {
        let started_at = events
            .iter()
            .find(|event| event.event == TaskEventKind::Started)
            .map(|event| event.at);
        let closed_at = events
            .iter()
            .rev()
            .find(|event| close_event_outcome(event.event).is_some())
            .map(|event| event.at);
        let outcome = events
            .iter()
            .rev()
            .find_map(|event| close_event_outcome(event.event))
            .unwrap_or(row.state.as_str());
        Self {
            row,
            started_at,
            closed_at,
            outcome,
            events,
        }
    }

    fn duration(&self) -> chrono::Duration {
        let end = self
            .closed_at
            .unwrap_or_else(atm_storage::IsoTimestamp::now);
        end.into_inner() - self.row.assigned_at.into_inner()
    }
}

const fn close_event_outcome(event: TaskEventKind) -> Option<&'static str> {
    match event {
        TaskEventKind::Completed => Some(TaskCloseOutcome::Completed.as_str()),
        TaskEventKind::Refused => Some(TaskCloseOutcome::Refused.as_str()),
        TaskEventKind::Cancelled => Some(TaskCloseOutcome::Cancelled.as_str()),
        _ => None,
    }
}

impl TaskHistoryCommand {
    fn resolved_limit(&self) -> Result<usize, atm_core::error::AtmError> {
        let limit = self.limit.unwrap_or(DEFAULT_HISTORY_LIMIT);
        // Client-side pre-check only: reject an obviously bad `--limit`
        // before a round trip. `list_task_ledger_with_runtime_async`'s
        // `History` arm re-checks the same `TaskPage::bounded` ceiling at
        // the daemon/list boundary, since a `TaskLedgerQuery::History`
        // request can also be built without going through this CLI path.
        atm_core::task_query::TaskPage::bounded(limit)?;
        Ok(limit)
    }

    pub(crate) async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("task history")?;
        let caller = resolve_context(&self.caller)?;
        let composition = composition("task history", observability, &home_dir, &current_dir)?;
        print!(
            "{}",
            self.execute(&composition, caller, home_dir, current_dir)
                .await?
        );
        Ok(())
    }

    async fn execute(
        self,
        composition: &CliComposition<'_>,
        caller: CallerContext,
        home_dir: PathBuf,
        current_dir: PathBuf,
    ) -> Result<String> {
        let limit = self.resolved_limit()?;
        preflight_daemon_api(
            composition,
            HttpApiVersion::parse("1.10.0")?,
            "task history",
        )
        .await?;
        let history_query = ListQuery::new(
            home_dir.clone(),
            current_dir.clone(),
            caller.caller_identity.clone(),
            None,
            caller.caller_team.clone(),
            ReadSelection::Actionable,
            false,
            None,
            None,
            None,
            None,
            None,
        )?
        .with_task_ledger(TaskLedgerQuery::History {
            member: self.member.clone(),
            limit,
        });
        let outcome = composition.list(history_query).await?;

        // The History arm of `list_task_ledger_with_runtime_async` already
        // batches every selected task's events into one ledger read, so
        // this only needs to regroup `task_event_rows` by task id, not issue
        // a further round trip per row.
        let mut events_by_task: std::collections::HashMap<TaskId, Vec<TaskEventRow>> =
            std::collections::HashMap::new();
        for event in outcome.task_event_rows {
            events_by_task
                .entry(event.task_id.clone())
                .or_default()
                .push(event);
        }
        let mut rows = Vec::with_capacity(outcome.task_rows.len());
        for row in outcome.task_rows {
            let events = events_by_task.remove(&row.task_id).unwrap_or_default();
            rows.push(HistoryRow::from_row_and_events(row, events));
        }

        if self.json {
            render_history_json(&rows, self.events)
        } else {
            Ok(render_history_table(&rows, self.events))
        }
    }
}

fn render_history_json(rows: &[HistoryRow], include_events: bool) -> Result<String> {
    let tasks = rows
        .iter()
        .map(|entry| {
            serde_json::json!({
                "task_id": entry.row.task_id.as_str(),
                "team": entry.row.team.as_str(),
                "assignee": entry.row.assignee.as_str(),
                "assigner": entry.row.assigner.as_str(),
                "state": entry.row.state.tag(),
                "assigned_at": entry.row.assigned_at.to_string(),
                "started_at": entry.started_at.map(|at| at.to_string()),
                "closed_at": entry.closed_at.map(|at| at.to_string()),
                "outcome": entry.outcome,
                "duration_secs": entry.duration().num_seconds(),
                "reminders": entry.row.reminder_count,
                "escalations": entry.row.lead_notified_count,
            })
        })
        .collect::<Vec<_>>();
    let mut payload = serde_json::json!({ "tasks": tasks });
    if include_events {
        payload["events"] = serde_json::to_value(chronological_events(rows))?;
    }
    Ok(format!("{}\n", serde_json::to_string_pretty(&payload)?))
}

/// Every selected task's ledger rows, sorted chronologically across tasks.
fn chronological_events(rows: &[HistoryRow]) -> Vec<&TaskEventRow> {
    let mut events = rows
        .iter()
        .flat_map(|entry| entry.events.iter())
        .collect::<Vec<_>>();
    events.sort_by_key(|event| (event.at, &event.task_id, event.seq));
    events
}

fn render_history_table(rows: &[HistoryRow], include_events: bool) -> String {
    use std::fmt::Write as _;

    let mut output = String::from(
        "task_id     assignee   assigner   assigned started closed outcome    duration reminders escalations\n",
    );
    let mut current_date: Option<NaiveDate> = None;
    for entry in rows {
        let date = entry.row.assigned_at.into_inner().date_naive();
        if current_date != Some(date) {
            writeln!(output, "{date}").expect("writing to String cannot fail");
            current_date = Some(date);
        }
        writeln!(
            output,
            "{:<11} {:<10} {:<10} {:<8} {:<7} {:<6} {:<10} {:<8} {:<9} {}",
            entry.row.task_id.as_str(),
            entry.row.assignee.as_str(),
            entry.row.assigner.as_str(),
            hhmm(entry.row.assigned_at),
            entry.started_at.map_or("-".to_string(), hhmm),
            entry.closed_at.map_or("-".to_string(), hhmm),
            entry.outcome,
            format_duration(entry.duration()),
            entry.row.reminder_count,
            entry.row.lead_notified_count,
        )
        .expect("writing to String cannot fail");
    }
    if include_events {
        output.push('\n');
        output.push_str("seq at event from→to actor detail\n");
        for event in chronological_events(rows) {
            render_task_event_line(&mut output, event, Some(&event.task_id));
        }
    }
    output
}

fn hhmm(at: atm_storage::IsoTimestamp) -> String {
    at.into_inner().format("%H:%M").to_string()
}

/// Formats an elapsed span like `2h 38m`, `32m`, `9h 22m`, or `3d 4h`.
fn format_duration(duration: chrono::Duration) -> String {
    let total_minutes = duration.num_minutes().max(0);
    let days = total_minutes / (24 * 60);
    let hours = (total_minutes % (24 * 60)) / 60;
    let minutes = total_minutes % 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests;
