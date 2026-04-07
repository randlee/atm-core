use anyhow::Result;
use atm_core::ack::AckOutcome;
use atm_core::clear::ClearOutcome;
use atm_core::doctor::{DoctorReport, DoctorSeverity, DoctorStatus};
use atm_core::observability::{AtmLogRecord, AtmLogSnapshot};
use atm_core::read::ReadOutcome;
use atm_core::send::SendOutcome;
use atm_core::team_admin::{
    AddMemberOutcome, BackupOutcome, MembersList, RestoreOutcome, RestorePlan, TeamsList,
};
use atm_core::types::DisplayBucket;

pub fn print_send_result(outcome: &SendOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!(
            "Sent to {}@{} [message_id: {}]",
            outcome.agent, outcome.team, outcome.message_id
        );
        for warning in &outcome.warnings {
            println!("{warning}");
        }
    }

    Ok(())
}

pub fn print_read_result(outcome: &ReadOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
        return Ok(());
    }

    println!("Queue: {}@{}", outcome.agent, outcome.team);
    println!(
        "Unread: {} | Pending-Ack: {} | History: {}",
        outcome.bucket_counts.unread,
        outcome.bucket_counts.pending_ack,
        outcome.bucket_counts.history
    );

    print_bucket(outcome, DisplayBucket::Unread, "Unread");
    print_bucket(outcome, DisplayBucket::PendingAck, "Pending Ack");

    if !outcome.history_collapsed {
        print_bucket(outcome, DisplayBucket::History, "History");
    } else if outcome.bucket_counts.history > 0 {
        println!();
        println!(
            "History: {} older messages hidden. Use --history or --all to show them.",
            outcome.bucket_counts.history
        );
    }

    Ok(())
}

pub fn print_ack_result(outcome: &AckOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!(
            "Acknowledged {} for {}@{} and sent reply {} to {}",
            outcome.message_id,
            outcome.agent,
            outcome.team,
            outcome.reply_message_id,
            outcome.reply_target
        );
    }

    Ok(())
}

pub fn print_clear_result(outcome: &ClearOutcome, dry_run: bool, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
        return Ok(());
    }

    if dry_run {
        println!(
            "Dry run: would remove {} message(s) from {}@{}",
            outcome.removed_total, outcome.agent, outcome.team
        );
    } else {
        println!(
            "Cleared {} message(s) from {}@{}",
            outcome.removed_total, outcome.agent, outcome.team
        );
    }

    println!(
        "Acknowledged: {} | Read: {} | Remaining: {}",
        outcome.removed_by_class.acknowledged,
        outcome.removed_by_class.read,
        outcome.remaining_total
    );

    Ok(())
}

pub fn print_log_snapshot(snapshot: &AtmLogSnapshot, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(snapshot)?);
        return Ok(());
    }

    for record in &snapshot.records {
        print_log_record_line(record);
    }

    Ok(())
}

pub fn print_log_records<I>(records: I, json: bool) -> Result<()>
where
    I: IntoIterator<Item = AtmLogRecord>,
{
    for record in records {
        if json {
            println!("{}", serde_json::to_string(&record)?);
        } else {
            print_log_record_line(&record);
        }
    }

    Ok(())
}

pub fn print_doctor_result(report: &DoctorReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!(
        "Doctor status: {}",
        match report.summary.status {
            DoctorStatus::Healthy => "healthy",
            DoctorStatus::Warning => "warning",
            DoctorStatus::Error => "error",
        }
    );
    println!("{}", report.summary.message);
    println!(
        "Active log path: {}",
        report
            .observability
            .active_log_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unavailable>".to_string())
    );
    println!(
        "Logging health: {} | Query readiness: {}",
        render_doctor_state(report.observability.logging_state),
        report
            .observability
            .query_state
            .map(render_doctor_state)
            .unwrap_or("unknown")
    );

    if report.environment.atm_home.is_some()
        || report.environment.atm_team.is_some()
        || report.environment.atm_identity.is_some()
        || report.environment.team_override.is_some()
    {
        println!();
        println!("Environment:");
        if let Some(path) = &report.environment.atm_home {
            println!("  ATM_HOME={}", path.display());
        }
        if let Some(team) = &report.environment.atm_team {
            println!("  ATM_TEAM={team}");
        }
        if let Some(identity) = &report.environment.atm_identity {
            println!("  ATM_IDENTITY={identity}");
        }
        if let Some(team_override) = &report.environment.team_override {
            println!("  --team={team_override}");
        }
    }

    if !report.findings.is_empty() {
        println!();
        println!("Findings:");
        for finding in &report.findings {
            println!(
                "  [{}] {} {}",
                render_finding_severity(finding.severity),
                finding.code,
                finding.message
            );
            if let Some(remediation) = &finding.remediation {
                println!("    remediation: {remediation}");
            }
        }
    }

    if !report.recommendations.is_empty() {
        println!();
        println!("Recommendations:");
        for recommendation in &report.recommendations {
            println!("  - {recommendation}");
        }
    }

    Ok(())
}

pub fn print_teams_result(outcome: &TeamsList, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
        return Ok(());
    }

    if outcome.teams.is_empty() {
        println!("No teams found");
        return Ok(());
    }

    println!("Teams:");
    for team in &outcome.teams {
        println!("  {} ({})", team.name, team.member_count);
    }
    Ok(())
}

pub fn print_members_result(outcome: &MembersList, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
        return Ok(());
    }

    println!("Team: {}", outcome.team);
    if outcome.members.is_empty() {
        println!("  No members");
        return Ok(());
    }

    for member in &outcome.members {
        println!(
            "  {} | type={} model={} cwd={} pane={}",
            member.name,
            empty_dash(&member.agent_type),
            empty_dash(&member.model),
            empty_dash(&member.cwd),
            empty_dash(&member.tmux_pane_id)
        );
    }
    Ok(())
}

pub fn print_add_member_result(outcome: &AddMemberOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!(
            "Added member {} to {} (created_inbox: {})",
            outcome.member, outcome.team, outcome.created_inbox
        );
    }
    Ok(())
}

pub fn print_backup_result(outcome: &BackupOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!("Backup created: {}", outcome.backup_path.display());
    }
    Ok(())
}

pub fn print_restore_plan(plan: &RestorePlan, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(plan)?);
        return Ok(());
    }

    println!(
        "Dry run — would restore from: {}",
        plan.backup_path.display()
    );
    println!("  Members: {}", plan.would_restore_members.join(", "));
    println!("  Inboxes: {}", plan.would_restore_inboxes.join(", "));
    println!("  Tasks: {}", plan.would_restore_tasks);
    Ok(())
}

pub fn print_restore_result(outcome: &RestoreOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!("Restored from: {}", outcome.backup_path.display());
        println!(
            "  members={} inboxes={} tasks={}",
            outcome.members_restored, outcome.inboxes_restored, outcome.tasks_restored
        );
    }
    Ok(())
}

fn print_bucket(outcome: &ReadOutcome, bucket: DisplayBucket, label: &str) {
    let messages = outcome
        .messages
        .iter()
        .filter(|message| message.bucket == bucket)
        .collect::<Vec<_>>();

    if messages.is_empty() {
        return;
    }

    println!();
    println!("{label}:");
    for message in messages {
        println!(
            "- {} {}: {}",
            message.envelope.timestamp.into_inner().to_rfc3339(),
            message.envelope.from,
            message
                .envelope
                .summary
                .as_deref()
                .unwrap_or(message.envelope.text.as_str())
        );
        if let Some(message_id) = message.envelope.message_id {
            println!("  message_id: {message_id}");
        }
    }
}

fn empty_dash(value: &str) -> &str {
    if value.is_empty() { "-" } else { value }
}

fn print_log_record_line(record: &AtmLogRecord) {
    let target = record.target.as_deref().unwrap_or("-");
    let action = record.action.as_deref().unwrap_or("-");
    let message = record.message.as_deref().unwrap_or("");

    println!(
        "{} {:?} {} {} {}",
        record.timestamp.into_inner().to_rfc3339(),
        record.severity,
        record.service,
        target,
        action
    );

    if !message.is_empty() {
        println!("  {message}");
    }

    if !record.fields.is_empty() {
        println!(
            "  fields: {}",
            serde_json::to_string(&record.fields).unwrap_or_else(|_| "{}".to_string())
        );
    }
}

fn render_doctor_state(
    state: atm_core::observability::AtmObservabilityHealthState,
) -> &'static str {
    match state {
        atm_core::observability::AtmObservabilityHealthState::Healthy => "healthy",
        atm_core::observability::AtmObservabilityHealthState::Degraded => "degraded",
        atm_core::observability::AtmObservabilityHealthState::Unavailable => "unavailable",
    }
}

fn render_finding_severity(severity: DoctorSeverity) -> &'static str {
    match severity {
        DoctorSeverity::Info => "info",
        DoctorSeverity::Warning => "warning",
        DoctorSeverity::Error => "error",
    }
}
