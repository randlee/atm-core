use crate::output_contract::{HelpResult, HelpResultKind};
use anyhow::Result;
use atm_core::ack::AckOutcome;
use atm_core::clear::ClearOutcome;
use atm_core::doctor::{
    BootstrapAutoStartOutcome, BootstrapConnectOutcome, BootstrapLaunchGateOutcome,
    BootstrapTraceReport, DoctorReport, DoctorSeverity, DoctorStatus,
};
use atm_core::list::ListOutcome;
use atm_core::observability::{AtmLogRecord, AtmLogSnapshot};
use atm_core::protocol::{RuntimeLivenessState, RuntimeReadinessState, RuntimeStatusSnapshot};
use atm_core::read::ReadOutcome;
use atm_core::send::SendOutcome;
use atm_core::send::WarningEntry;
use atm_core::team_admin::{
    AddMemberOutcome, BackupOutcome, ClearNudgeTemplateOverrideOutcome,
    DisableNudgeTemplateOverrideOutcome, RemoveMemberOutcome, RestoreOutcome, RestorePlan,
    SetNudgeTemplateOverrideOutcome, TeamsList, UpdateMemberOutcome,
};

/// Print one send result in human-readable or JSON form.
pub fn print_send_result(outcome: &SendOutcome, json: bool) -> Result<()> {
    print!("{}", render_send_stdout(outcome, json)?);
    print_warnings_to_stderr(&outcome.warnings);

    Ok(())
}

/// Print one help result in human-readable or JSON form.
pub fn print_help_result(result: &HelpResult, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(result)?);
        return Ok(());
    }

    match result.kind {
        HelpResultKind::CommandHelp => {
            print!("{}", result.body);
            if !result.body.ends_with('\n') {
                println!();
            }
        }
        _ => println!("{}", result.body),
    }

    Ok(())
}

/// Print one list result in human-readable or JSON form.
pub fn print_list_result(outcome: &ListOutcome, json: bool) -> Result<()> {
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

    for row in &outcome.rows {
        println!(
            "- {} {}: {}",
            row.timestamp.into_inner().to_rfc3339(),
            row.from,
            row.summary
        );
        println!(
            "  message_id: {}",
            row.message_id
                .map(|message_id| message_id.to_string())
                .unwrap_or_else(|| "<none>".to_string())
        );
        if let Some(task_id) = &row.task_id {
            println!("  task_id: {task_id}");
        }
        println!(
            "  state: {}{}",
            if row.read { "read" } else { "unread" },
            if row.pending_ack { " pending-ack" } else { "" }
        );
    }

    if outcome.history_collapsed && outcome.bucket_counts.history > 0 {
        println!();
        println!(
            "History: {} older messages hidden. Use --all to show them.",
            outcome.bucket_counts.history
        );
    }

    Ok(())
}

/// Print one read result in human-readable or JSON form.
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
    println!(
        "Selected: {} | Matches: {} | Additional: {}",
        outcome
            .selected_message_id
            .map(|message_id| message_id.to_string())
            .unwrap_or_else(|| "<none>".to_string()),
        outcome.match_count,
        outcome.additional_match_count
    );
    if let Some(message) = &outcome.message {
        println!();
        println!("From: {}", message.envelope.from);
        println!(
            "At: {}",
            message.envelope.timestamp.into_inner().to_rfc3339()
        );
        if let Some(task_id) = &message.envelope.task_id {
            println!("Task: {task_id}");
        }
        if let Some(summary) = message.envelope.summary.as_deref() {
            println!("Summary: {summary}");
        }
        println!("Body:");
        println!("{}", message.envelope.text);
    } else {
        println!();
        println!("No matching message.");
    }

    if outcome.additional_match_count > 0 {
        println!();
        println!(
            "Additional matches remain. Use `atm list` with the same filters to inspect them."
        );
    }

    Ok(())
}

/// Print one acknowledgement result in human-readable or JSON form.
pub fn print_ack_result(outcome: &AckOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!("{}", render_ack_result_line(outcome));
    }

    print_warnings_to_stderr(&outcome.warnings);

    Ok(())
}

fn render_send_stdout(outcome: &SendOutcome, json: bool) -> Result<String> {
    if json {
        return Ok(format!("{}\n", serde_json::to_string_pretty(outcome)?));
    }

    Ok(format!(
        "Sent to {}@{} [message_id: {}]\n",
        outcome.agent, outcome.team, outcome.message_id
    ))
}

fn print_warnings_to_stderr(warnings: &[WarningEntry]) {
    let rendered = render_warnings_to_stderr(warnings);
    if !rendered.is_empty() {
        eprint!("{rendered}");
    }
}

fn render_warnings_to_stderr(warnings: &[WarningEntry]) -> String {
    warnings
        .iter()
        .map(|warning| format!("{}\n", warning.render()))
        .collect()
}

fn render_ack_result_line(outcome: &AckOutcome) -> String {
    match &outcome.reply_disposition {
        atm_core::ack::AckReplyDisposition::Sent {
            reply_message_id,
            reply_target,
        } => format!(
            "Acknowledged {} for {}@{} and sent reply {} to {}",
            outcome.message_id, outcome.agent, outcome.team, reply_message_id, reply_target
        ),
    }
}

/// Print one clear result in human-readable or JSON form.
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

/// Print one retained log snapshot in human-readable or JSON form.
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

/// Print one stream of retained log records in human-readable or JSON form.
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

/// Print one doctor report in human-readable or JSON form.
pub fn print_doctor_result(report: &DoctorReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    print_doctor_summary(report);
    print_doctor_observability(report);
    print_doctor_post_send(report);
    println!(
        "Logging health: {} | Query readiness: {}",
        render_doctor_state(report.observability.logging_state),
        report
            .observability
            .query_state
            .map(render_doctor_state)
            .unwrap_or("unknown")
    );
    if let Some(runtime_status) = &report.runtime_status {
        print_runtime_status(runtime_status);
    }
    if let Some(bootstrap_trace) = &report.bootstrap_trace {
        print_bootstrap_trace(bootstrap_trace);
    }
    print_doctor_peer_config(report);
    print_doctor_environment(report);
    print_doctor_findings(report);
    print_doctor_roster(report);
    print_doctor_recommendations(report);

    Ok(())
}

fn print_doctor_peer_config(report: &DoctorReport) {
    let Some(peer_config) = report
        .daemon_runtime
        .as_ref()
        .and_then(|runtime| runtime.peer_config.as_ref())
    else {
        return;
    };
    println!("{}", render_doctor_peer_config(peer_config));
}

fn render_doctor_peer_config(peer_config: &atm_core::doctor::PeerConfigDoctorReport) -> String {
    let mut rendered = format!(
        "Peer HTTPS: interfaces={}/{} trusted_peers={}/{} certificate={}",
        peer_config.enabled_interface_count,
        peer_config.configured_interface_count,
        peer_config.enabled_trusted_peer_count,
        peer_config.trusted_peer_count,
        peer_config
            .certificate_fingerprint
            .as_deref()
            .unwrap_or("<not configured>")
    );
    if let Some(failure) = &peer_config.validation_failure {
        rendered.push_str(&format!(
            "\n  peer configuration failure: [{}] {}",
            failure.code, failure.message
        ));
    }
    for peer in &peer_config.trusted_peers {
        rendered.push_str(&format!(
            "\n  peer {}:{} ({})",
            peer.host,
            peer.https_port,
            if peer.enabled { "enabled" } else { "disabled" }
        ));
    }
    rendered
}

fn print_doctor_post_send(report: &DoctorReport) {
    let post_send = &report.post_send;
    if post_send.config_root.as_os_str().is_empty()
        && post_send.external_rules.is_empty()
        && post_send.recipient_paths.is_empty()
    {
        return;
    }
    println!(
        "Post-send configuration: {}",
        post_send.config_root.display()
    );
    for rule in &post_send.external_rules {
        println!(
            "  override recipient={} executable={} argv={:?}",
            rule.recipient_matcher,
            rule.executable.display(),
            rule.argv
        );
    }
    for recipient in &post_send.recipient_paths {
        println!(
            "  recipient={} path={:?}",
            recipient.recipient, recipient.path
        );
    }
}

fn print_doctor_summary(report: &DoctorReport) {
    println!(
        "Doctor status: {}",
        render_doctor_status(report.summary.status)
    );
    println!("{}", report.summary.message);
}

fn print_doctor_observability(report: &DoctorReport) {
    println!(
        "Active log path: {}",
        report
            .observability
            .active_log_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unavailable>".to_string())
    );
    if let Some(maintenance) = &report.observability.maintenance {
        println!(
            "Maintenance: {} | Rotated: {} | Pruned: {} | Last pass: {}",
            render_maintenance_state(maintenance.state),
            maintenance.rotated_files_total,
            maintenance.pruned_files_total,
            maintenance
                .last_pass_at
                .map(|timestamp| timestamp.into_inner().to_string())
                .unwrap_or_else(|| "never".to_string())
        );
    }
}

fn print_doctor_environment(report: &DoctorReport) {
    if report.environment.atm_home.is_none()
        && report.environment.atm_team.is_none()
        && report.environment.atm_identity.is_none()
        && report.environment.team_override.is_none()
        && report.client_context.team.is_none()
        && report.client_context.identity.is_none()
        && report.client_context.version.is_none()
        && report.daemon_context.as_ref().is_none_or(|context| {
            context.team.is_none() && context.identity.is_none() && context.version.is_none()
        })
    {
        return;
    }

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
    if report.client_context.team.is_some()
        || report.client_context.identity.is_some()
        || report.client_context.version.is_some()
    {
        println!("  client_context:");
        if let Some(team) = &report.client_context.team {
            println!("    team={team}");
        }
        if let Some(identity) = &report.client_context.identity {
            println!("    identity={identity}");
        }
        if let Some(version) = &report.client_context.version {
            println!("    version={version}");
        }
    }
    if let Some(daemon_context) = &report.daemon_context
        && (daemon_context.team.is_some()
            || daemon_context.identity.is_some()
            || daemon_context.version.is_some())
    {
        println!("  daemon_context (daemon launch-time process env, not the caller):");
        if let Some(team) = &daemon_context.team {
            println!("    team={team}");
        }
        if let Some(identity) = &daemon_context.identity {
            println!("    identity={identity}");
        }
        if let Some(version) = &daemon_context.version {
            println!("    version={version}");
        }
    }
}

fn print_doctor_findings(report: &DoctorReport) {
    if report.findings.is_empty() {
        return;
    }

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

fn print_doctor_roster(report: &DoctorReport) {
    let Some(roster) = &report.member_roster else {
        return;
    };
    println!();
    println!("Members: {}", roster.team);
    for member in &roster.members {
        let home_dir = member.home_dir.as_path().display().to_string();
        println!(
            "  {} | type={} harness={} model={} home_dir={} live_cwd={} pane={}",
            member.name,
            empty_dash(&member.agent_type),
            member.harness,
            empty_dash(&member.model),
            empty_dash(&home_dir),
            empty_dash_opt(member.live_cwd.as_deref()),
            empty_dash_opt(member.tmux_pane_id.as_deref())
        );
    }
}

fn print_doctor_recommendations(report: &DoctorReport) {
    if report.recommendations.is_empty() {
        return;
    }

    println!();
    println!("Recommendations:");
    for recommendation in &report.recommendations {
        println!("  - {recommendation}");
    }
}

/// Print one teams listing in human-readable or JSON form.
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

/// Print one add-member result in human-readable or JSON form.
pub fn print_add_member_result(outcome: &AddMemberOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!(
            "Added member {} to {} (created_inbox: {})",
            outcome.member, outcome.team, outcome.created_inbox
        );
    }
    for warning in &outcome.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(())
}

/// Print one update-member result in human-readable or JSON form.
pub fn print_update_member_result(outcome: &UpdateMemberOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!("Updated member {} in {}", outcome.member, outcome.team);
    }
    for warning in &outcome.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(())
}

/// Print one remove-member result in human-readable or JSON form.
pub fn print_remove_member_result(outcome: &RemoveMemberOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!("Removed member {} from {}", outcome.member, outcome.team);
    }
    Ok(())
}

/// Print one set-nudge-template result in human-readable or JSON form.
pub fn print_set_nudge_template_override_result(
    outcome: &SetNudgeTemplateOverrideOutcome,
    json: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!(
            "Set nudge template override {} for {} at {}",
            outcome.kind, outcome.team, outcome.updated_at
        );
    }
    Ok(())
}

/// Print one disable-nudge-template result in human-readable or JSON form.
pub fn print_disable_nudge_template_override_result(
    outcome: &DisableNudgeTemplateOverrideOutcome,
    json: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!(
            "Disabled nudge template override {} for {} at {}",
            outcome.kind, outcome.team, outcome.updated_at
        );
    }
    Ok(())
}

/// Print one clear-nudge-template result in human-readable or JSON form.
pub fn print_clear_nudge_template_override_result(
    outcome: &ClearNudgeTemplateOverrideOutcome,
    json: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        let status = if outcome.cleared {
            "cleared"
        } else {
            "already at product default"
        };
        println!(
            "Clear nudge template override {} for {}: {}",
            outcome.kind, outcome.team, status
        );
    }
    Ok(())
}

/// Print one backup result in human-readable or JSON form.
pub fn print_backup_result(outcome: &BackupOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!("Backup created: {}", outcome.backup_path.display());
    }
    Ok(())
}

/// Print one restore dry-run plan in human-readable or JSON form.
pub fn print_restore_plan(plan: &RestorePlan, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(plan)?);
        return Ok(());
    }

    println!(
        "Dry run — would restore from: {}",
        plan.backup_path.display()
    );
    println!(
        "  Members: {}",
        plan.would_restore_members
            .iter()
            .map(|member| member.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("  Inboxes: {}", plan.would_restore_inboxes.join(", "));
    println!("  Tasks: {}", plan.would_restore_tasks);
    Ok(())
}

/// Print one applied restore result in human-readable or JSON form.
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

fn empty_dash(value: &str) -> &str {
    if value.is_empty() { "-" } else { value }
}

fn empty_dash_opt(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("-")
}

fn print_log_record_line(record: &AtmLogRecord) {
    let target = record.target.as_deref().unwrap_or("-");
    let action = record.action.as_deref().unwrap_or("-");
    let message = record.message.as_deref().unwrap_or("");

    println!(
        "{} {:?} {} {} {}",
        record.timestamp.into_inner().to_rfc3339(),
        record.level,
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

fn render_doctor_status(status: DoctorStatus) -> &'static str {
    match status {
        DoctorStatus::Healthy => "healthy",
        DoctorStatus::Warning => "warning",
        DoctorStatus::Error => "error",
    }
}

fn render_maintenance_state(
    state: atm_core::observability::AtmMaintenanceWorkerState,
) -> &'static str {
    match state {
        atm_core::observability::AtmMaintenanceWorkerState::Running => "running",
        atm_core::observability::AtmMaintenanceWorkerState::Degraded => "degraded",
        atm_core::observability::AtmMaintenanceWorkerState::Stopped => "stopped",
    }
}

fn render_finding_severity(severity: DoctorSeverity) -> &'static str {
    match severity {
        DoctorSeverity::Info => "info",
        DoctorSeverity::Warning => "warning",
        DoctorSeverity::Error => "error",
    }
}

fn print_runtime_status(runtime_status: &RuntimeStatusSnapshot) {
    println!();
    println!("Runtime status:");
    println!(
        "  Liveness: {} | Readiness: {}",
        render_runtime_liveness(runtime_status.liveness),
        render_runtime_readiness(runtime_status.readiness)
    );
    println!(
        "  Members: active={} idle={} offline={} unknown={}",
        runtime_status.member_counts.active_members,
        runtime_status.member_counts.idle_members,
        runtime_status.member_counts.offline_members,
        runtime_status.member_counts.unknown_members
    );
    println!(
        "  Degraded ingest: {}",
        render_bool(runtime_status.degraded_ingest)
    );
    if let Some(owner_pid) = runtime_status.singleton_owner_pid {
        println!("  Singleton owner pid: {owner_pid}");
    }
    if let Some(detail) = &runtime_status.detail {
        println!("  Detail: {detail}");
    }
}

fn print_bootstrap_trace(trace: &BootstrapTraceReport) {
    print!("{}", render_bootstrap_trace_section(trace));
}

fn render_runtime_liveness(state: RuntimeLivenessState) -> &'static str {
    match state {
        RuntimeLivenessState::Running => "running",
        RuntimeLivenessState::Unavailable => "unavailable",
    }
}

fn render_runtime_readiness(state: RuntimeReadinessState) -> &'static str {
    match state {
        RuntimeReadinessState::Ready => "ready",
        RuntimeReadinessState::Degraded => "degraded",
        RuntimeReadinessState::Unavailable => "unavailable",
    }
}

fn render_bootstrap_connect(state: BootstrapConnectOutcome) -> &'static str {
    match state {
        BootstrapConnectOutcome::Connected => "connected",
        BootstrapConnectOutcome::NotFound => "not_found",
        BootstrapConnectOutcome::Timeout => "timeout",
        BootstrapConnectOutcome::Failed => "failed",
    }
}

fn render_bootstrap_launch_gate(state: BootstrapLaunchGateOutcome) -> &'static str {
    match state {
        BootstrapLaunchGateOutcome::Launched => "launched",
        BootstrapLaunchGateOutcome::Failed => "failed",
        BootstrapLaunchGateOutcome::Skipped => "skipped",
    }
}

fn render_bootstrap_auto_start(state: BootstrapAutoStartOutcome) -> &'static str {
    match state {
        BootstrapAutoStartOutcome::AutoStarted => "auto_started",
        BootstrapAutoStartOutcome::Failed => "failed",
        BootstrapAutoStartOutcome::Skipped => "skipped",
    }
}

fn render_bool(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn render_bootstrap_trace_section(trace: &BootstrapTraceReport) -> String {
    let mut output = String::from("\nBootstrap trace:\n");
    output.push_str(&format!(
        "  Daemon connect: {}\n",
        render_bootstrap_connect(trace.daemon_connect)
    ));
    output.push_str(&format!(
        "  Launch gate: {}\n",
        render_bootstrap_launch_gate(trace.daemon_launch_gate)
    ));
    output.push_str(&format!(
        "  Auto-start: {}\n",
        render_bootstrap_auto_start(trace.daemon_auto_start)
    ));
    if let Some(detail) = &trace.connect_detail {
        output.push_str(&format!("  Connect detail: {detail}\n"));
    }
    if let Some(detail) = &trace.launch_gate_detail {
        output.push_str(&format!("  Launch-gate detail: {detail}\n"));
    }
    if let Some(detail) = &trace.auto_start_detail {
        output.push_str(&format!("  Auto-start detail: {detail}\n"));
    }
    output
}

#[cfg(test)]
mod tests {
    use atm_core::ack::AckOutcome;
    use atm_core::doctor::{
        BootstrapAutoStartOutcome, BootstrapConnectOutcome, BootstrapLaunchGateOutcome,
        BootstrapTraceReport, PeerConfigDoctorReport,
    };
    use serde_json::json;

    use super::{
        render_bootstrap_trace_section, render_doctor_peer_config, render_send_stdout,
        render_warnings_to_stderr,
    };

    #[test]
    fn send_outcome_json_preserves_unrostered_sender_advisory() {
        let outcome = json!({
            "action": "send",
            "team": "test-team",
            "agent": "recipient",
            "sender": "unregistered-tool",
            "outcome": "sent",
            "message_id": "01KX5TEST00000000000000001",
            "requires_ack": false,
            "warnings": [{
                "message": "declared sender unregistered-tool@test-team is not on the ATM roster; this identity has no inbox and cannot receive replies or assignments.",
                "recovery": "Add it with `atm teams add-member test-team unregistered-tool` if this identity needs an inbox."
            }]
        });

        let outcome: atm_core::send::SendOutcome =
            serde_json::from_value(outcome).expect("send outcome with advisory");
        let rendered = serde_json::to_value(outcome).expect("JSON output");

        assert_eq!(
            rendered["warnings"][0]["message"],
            "declared sender unregistered-tool@test-team is not on the ATM roster; this identity has no inbox and cannot receive replies or assignments."
        );
        assert_eq!(
            rendered["warnings"][0]["recovery"],
            "Add it with `atm teams add-member test-team unregistered-tool` if this identity needs an inbox."
        );
    }

    #[test]
    fn sender_advisory_stays_on_stderr_while_json_stdout_remains_parseable() {
        let outcome: atm_core::send::SendOutcome = serde_json::from_value(json!({
            "action": "send",
            "team": "test-team",
            "agent": "recipient",
            "sender": "unregistered-tool",
            "outcome": "sent",
            "message_id": "01KX5TEST00000000000000001",
            "requires_ack": false,
            "warnings": [{
                "message": "declared sender unregistered-tool@test-team is not on the ATM roster; this identity has no inbox and cannot receive replies or assignments.",
                "recovery": "Add it with `atm teams add-member test-team unregistered-tool` if this identity needs an inbox."
            }]
        }))
        .expect("send outcome");

        let stdout = render_send_stdout(&outcome, true).expect("JSON stdout");
        let stderr = render_warnings_to_stderr(&outcome.warnings);

        let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON stdout");
        assert_eq!(
            parsed["warnings"][0]["message"],
            outcome.warnings[0].message
        );
        assert!(!stdout.contains("Recovery:"));
        assert!(stderr.contains("Recovery:"));
        assert!(stderr.contains("unregistered-tool@test-team"));
    }

    #[test]
    fn bootstrap_trace_section_renders_doctor_output_block() {
        let rendered = render_bootstrap_trace_section(&BootstrapTraceReport {
            daemon_connect: BootstrapConnectOutcome::Connected,
            daemon_launch_gate: BootstrapLaunchGateOutcome::Launched,
            daemon_auto_start: BootstrapAutoStartOutcome::AutoStarted,
            connect_detail: Some("connect detail".to_string()),
            launch_gate_detail: None,
            auto_start_detail: Some("auto-start detail".to_string()),
        });

        assert!(rendered.contains("Bootstrap trace:"));
        assert!(rendered.contains("Daemon connect: connected"));
        assert!(rendered.contains("Launch gate: launched"));
        assert!(rendered.contains("Auto-start: auto_started"));
        assert!(rendered.contains("Connect detail: connect detail"));
        assert!(rendered.contains("Auto-start detail: auto-start detail"));
    }

    #[test]
    fn ack_output_json_shape_preserves_sent_reply_disposition() {
        let outcome: AckOutcome = serde_json::from_value(json!({
            "action": "ack",
            "team": "test-team",
            "agent": "sender-a",
            "message_id": "01KX5TEST00000000000000002",
            "task_id": null,
            "reply_disposition": {
                "kind": "sent",
                "reply_target": "team-lead@test-team",
                "reply_message_id": "01KX5TEST00000000000000003"
            },
            "reply_text": "received",
            "warnings": []
        }))
        .expect("ack outcome");

        let rendered = serde_json::to_value(&outcome).expect("json outcome");
        assert_eq!(rendered["reply_disposition"]["kind"], "sent");
        assert_eq!(
            rendered["reply_disposition"]["reply_target"],
            "team-lead@test-team"
        );
        assert_eq!(
            rendered["reply_disposition"]["reply_message_id"],
            "01KX5TEST00000000000000003"
        );
    }

    #[test]
    fn doctor_peer_text_redacts_private_key_material() {
        let rendered = render_doctor_peer_config(&PeerConfigDoctorReport {
            configured_interface_count: 2,
            enabled_interface_count: 1,
            certificate_fingerprint: Some("sha256:public-fingerprint".to_string()),
            trusted_peer_count: 3,
            enabled_trusted_peer_count: 2,
            trusted_peers: vec![atm_core::doctor::PeerAuthorityDoctorReport {
                host: "peer.example.test".to_string(),
                https_port: 43101,
                enabled: true,
            }],
            validation_failure: None,
        });

        assert!(rendered.contains("sha256:public-fingerprint"));
        assert!(rendered.contains("peer.example.test:43101 (enabled)"));
        assert!(!rendered.contains("private_key_ref"));
        assert!(!rendered.contains("keychain:secret"));
    }
}
