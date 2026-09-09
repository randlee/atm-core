use crate::output_contract::{HelpResult, HelpResultKind};
use anyhow::Result;
use atm_core::PickerMembersProjection;
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
use atm_core::types::HostName;
use std::fmt::Write as _;

/// Print one send result in human-readable or JSON form.
pub fn print_send_result(outcome: &SendOutcome, json: bool) -> Result<()> {
    print!("{}", render_send_stdout(outcome, json, None)?);
    print_warnings_to_stderr(&outcome.warnings);

    Ok(())
}

/// Print a send result with the peer host confirmed by the transport.
pub fn print_send_result_to_peer(
    outcome: &SendOutcome,
    json: bool,
    peer_host: &HostName,
) -> Result<()> {
    print!("{}", render_send_stdout(outcome, json, Some(peer_host))?);
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

fn render_send_stdout(
    outcome: &SendOutcome,
    json: bool,
    peer_host: Option<&HostName>,
) -> Result<String> {
    if json {
        let mut value = serde_json::to_value(outcome)?;
        if let Some(peer_host) = peer_host {
            value["delivered_to_peer_host"] = serde_json::Value::String(peer_host.to_string());
        }
        return Ok(format!("{}\n", serde_json::to_string_pretty(&value)?));
    }

    let mut rendered = format!(
        "Sent to {}@{} [message_id: {}]\n",
        outcome.agent, outcome.team, outcome.message_id
    );
    if let Some(peer_host) = peer_host {
        rendered.push_str(&format!("Delivered to peer host: {peer_host}\n"));
    }
    Ok(rendered)
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
    print_doctor_graft_receivers(report);
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
    print_doctor_herdr(report);
    print_doctor_peer_config(report);
    print_doctor_escalation_recipients(report);
    print_doctor_environment(report);
    print_doctor_findings(report);
    print_doctor_roster(report);
    print_doctor_alias_mismatches(report);
    print_doctor_recommendations(report);

    Ok(())
}

fn print_doctor_herdr(report: &DoctorReport) {
    print!("{}", render_doctor_herdr(&report.herdr));
}

fn render_doctor_herdr(report: &atm_core::doctor::HerdrDoctorReport) -> String {
    let configured = match report.configured {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    };
    let mut output = format!("Herdr: configured={configured}\n");
    if let Some(error) = &report.error {
        writeln!(output, "  Error: {} ({})", error.message, error.code)
            .expect("writing to String cannot fail");
    }
    for endpoint in &report.endpoints {
        let session = endpoint.session.as_ref().map_or(
            atm_core::HerdrSession::DEFAULT_NAME,
            atm_core::HerdrSession::as_str,
        );
        let provenance = match endpoint.provenance {
            atm_core::doctor::HerdrEndpointProvenance::Session => "session",
            atm_core::doctor::HerdrEndpointProvenance::SocketPath => "socket_path",
            atm_core::doctor::HerdrEndpointProvenance::HerdrDefault => "herdr_default",
        };
        let transport = match endpoint.transport {
            atm_core::doctor::HerdrTransportKind::Cli => "cli",
            atm_core::doctor::HerdrTransportKind::Socket => "socket",
        };
        let endpoint_path = endpoint
            .endpoint
            .as_ref()
            .map_or("<none>", atm_core::doctor::HerdrEndpointDisplay::as_str);
        let binary = endpoint.binary.as_ref().map_or_else(
            || "PATH".to_owned(),
            |binary| binary.path.display().to_string(),
        );
        let capability = endpoint
            .capabilities
            .live_handoff
            .map_or("unknown", |value| if value { "yes" } else { "no" });
        let state = serde_json::to_string(&endpoint.state)
            .expect("Herdr doctor state is always serializable");
        writeln!(
            output,
            "  Endpoint {session}: provenance={provenance} transport={transport} endpoint={endpoint_path} binary={binary} live_handoff={capability}"
        )
        .expect("writing to String cannot fail");
        writeln!(output, "    State: {state}").expect("writing to String cannot fail");
        writeln!(output, "    Remedy: {}", endpoint.remedy).expect("writing to String cannot fail");
        for member in &endpoint.members {
            let outcome = serde_json::to_string(&member.outcome)
                .expect("Herdr member outcome is always serializable");
            writeln!(output, "    Member {}: {outcome}", member.name)
                .expect("writing to String cannot fail");
        }
    }
    output
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
    for legacy_peer in &peer_config.legacy_literal_ip_peers {
        rendered.push_str(&format!(
            "\n  legacy literal-IP peer {} ({}): migrate with `{}` or retire with `{}`",
            legacy_peer.host,
            if legacy_peer.enabled {
                "enabled"
            } else {
                "disabled"
            },
            legacy_peer.migrate_command,
            legacy_peer.revoke_command
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
    print!("{}", render_doctor_observability(report));
}

fn render_doctor_observability(report: &DoctorReport) -> String {
    let mut output = format!(
        "Active log path: {}\n",
        report
            .observability
            .active_log_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unavailable>".to_string())
    );
    if let Some(maintenance) = &report.observability.maintenance {
        output.push_str(&format!(
            "Maintenance: {} | Rotated: {} | Pruned: {} | Last pass: {}",
            render_maintenance_state(maintenance.state),
            maintenance.rotated_files_total,
            maintenance.pruned_files_total,
            maintenance
                .last_pass_at
                .map(|timestamp| timestamp.into_inner().to_string())
                .unwrap_or_else(|| "never".to_string())
        ));
        output.push('\n');
    }
    output.push_str(&format!(
        "Observability: jsonl forwarded={} queue_full_dropped={} reentrant_dropped={}; timeline written={} queue_full_dropped={} persist_error_dropped={}\n",
        report.observability.jsonl.forwarded_total,
        report.observability.jsonl.dropped_queue_full_total,
        report.observability.jsonl.dropped_reentrant_total,
        report.observability.timeline.written_total,
        report.observability.timeline.dropped_queue_full_total,
        report.observability.timeline.dropped_persist_error_total,
    ));
    if !report.observability.degraded.is_empty() {
        output.push_str(&format!(
            "WARN: Retained observability degraded: {}\n",
            report.observability.degraded.join(", ")
        ));
    }
    output
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

fn print_doctor_escalation_recipients(report: &DoctorReport) {
    println!();
    println!("Escalation recipients (daemon default):");
    if report.escalation_recipients.daemon.is_empty() {
        println!("  none");
    } else {
        for recipient in &report.escalation_recipients.daemon {
            println!("  {recipient}");
        }
    }
    for team in &report.escalation_recipients.teams {
        println!("  team {} [{}]:", team.team, team.source);
        if team.recipients.is_empty() {
            println!("    none");
        } else {
            for recipient in &team.recipients {
                println!("    {recipient}");
            }
        }
    }
}

fn print_doctor_roster(report: &DoctorReport) {
    print!(
        "{}",
        render_doctor_rosters(report.member_roster.as_ref(), &report.team_rosters)
    );
}

fn print_doctor_alias_mismatches(report: &DoctorReport) {
    let rendered = render_doctor_alias_mismatches(&report.alias_mismatches);
    if rendered.is_empty() {
        return;
    }
    print!("{rendered}");
}

fn render_doctor_alias_mismatches(mismatches: &[atm_core::doctor::DoctorAliasMismatch]) -> String {
    if mismatches.is_empty() {
        return String::new();
    }

    let mut rendered = String::from("\nAlias mismatches:\n");
    for mismatch in mismatches {
        let _ = writeln!(
            rendered,
            "  team={} member={} config_alias={} roster_alias={}",
            mismatch.team,
            mismatch.member,
            mismatch.config_alias,
            empty_dash_opt(mismatch.roster_alias.as_deref()),
        );
    }
    rendered
}

fn render_doctor_rosters(
    member_roster: Option<&atm_core::team_admin::MembersList>,
    team_rosters: &[atm_core::team_admin::MembersList],
) -> String {
    let mut rendered = String::new();
    if let Some(roster) = member_roster {
        rendered.push_str(&render_doctor_roster_block(roster));
    }
    for roster in team_rosters {
        rendered.push_str(&render_doctor_roster_block(roster));
    }
    rendered
}

fn render_doctor_roster_block(roster: &atm_core::team_admin::MembersList) -> String {
    let mut rendered = format!("\nMembers: {}\n", roster.team);
    for member in &roster.members {
        let home_dir = member.home_dir.as_path().display().to_string();
        rendered.push_str(&format!(
            "  {} | type={} harness={} model={} home_dir={} live_cwd={} pane={}\n",
            member.name,
            empty_dash(&member.agent_type),
            member.harness,
            empty_dash(&member.model),
            empty_dash(&home_dir),
            empty_dash_opt(member.live_cwd.as_deref()),
            empty_dash_opt(member.tmux_pane_id.as_deref())
        ));
    }
    rendered
}

fn print_doctor_graft_receivers(report: &DoctorReport) {
    if report.graft_receivers.receivers.is_empty() {
        return;
    }
    println!();
    println!("Graft receivers:");
    for receiver in &report.graft_receivers.receivers {
        println!(
            "  {}@{} | endpoint={} last_seen_age={}s reachable_at_last_use={}",
            receiver.agent,
            receiver.team,
            receiver.endpoint,
            receiver.last_seen_age_seconds,
            receiver.reachable_at_last_use
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

/// Print the picker member projection (`atm teams --members`, ADR-055
/// decision (e), PRD §4.2/§5a) in human-readable or JSON form.
pub fn print_picker_members_projection(
    projection: &PickerMembersProjection,
    json: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(projection)?);
        return Ok(());
    }

    if projection.members.is_empty() {
        println!("No members found for team {}", projection.team);
        return Ok(());
    }

    println!("Members ({}):", projection.team);
    for member in &projection.members {
        println!(
            "  {} host={} cwd={} status={:?}",
            member.id,
            member
                .host
                .as_ref()
                .map(|host| host.as_str())
                .unwrap_or("-"),
            member.cwd.as_deref().unwrap_or("-"),
            member.status
        );
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
    use atm_core::HerdrSession;
    use atm_core::ack::AckOutcome;
    use atm_core::doctor::{
        BootstrapAutoStartOutcome, BootstrapConnectOutcome, BootstrapLaunchGateOutcome,
        BootstrapTraceReport, DoctorAliasMismatch, HerdrDoctorReport, HerdrDoctorState,
        HerdrEndpointCapabilitiesDoctorReport, HerdrEndpointDisplay, HerdrEndpointDisplayRoot,
        HerdrEndpointDoctorReport, HerdrEndpointProvenance, HerdrTransportKind, HerdrVersion,
        PeerConfigDoctorReport,
    };
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::team_admin::MembersList;
    use atm_core::types::HostName;
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::{
        render_bootstrap_trace_section, render_doctor_alias_mismatches, render_doctor_herdr,
        render_doctor_peer_config, render_doctor_rosters, render_send_stdout,
        render_warnings_to_stderr,
    };

    #[test]
    fn herdr_doctor_rendering_exposes_typed_state_and_remedy_without_raw_endpoint() {
        let report = HerdrDoctorReport {
            configured: Some(true),
            endpoints: vec![HerdrEndpointDoctorReport {
                session: None,
                provenance: HerdrEndpointProvenance::HerdrDefault,
                transport: HerdrTransportKind::Cli,
                endpoint: None,
                binary: None,
                state: HerdrDoctorState::NotConfigured,
                remedy: "Configure Herdr only if desired".to_owned(),
                capabilities: HerdrEndpointCapabilitiesDoctorReport {
                    live_handoff: Some(true),
                },
                members: Vec::new(),
                findings: Vec::new(),
            }],
            ..HerdrDoctorReport::default()
        };

        let rendered = render_doctor_herdr(&report);
        let json = serde_json::to_value(&report).expect("Herdr report serializes");

        assert!(rendered.contains("Herdr: configured=yes"));
        assert!(rendered.contains("provenance=herdr_default transport=cli endpoint=<none>"));
        assert!(rendered.contains("State: {\"kind\":\"not_configured\"}"));
        assert!(rendered.contains("Remedy: Configure Herdr only if desired"));
        assert_eq!(json["endpoints"][0]["capabilities"]["live_handoff"], true);
        assert!(json["endpoints"][0].get("live_handoff").is_none());
    }

    #[test]
    fn doctor_roster_rendering_prints_one_heading_per_team() {
        let rosters = ["team-a", "team-b"]
            .into_iter()
            .map(|team| MembersList {
                team: team.parse().expect("team"),
                members: Vec::new(),
            })
            .collect::<Vec<_>>();
        let rendered = render_doctor_rosters(None, &rosters);

        assert_eq!(rendered.matches("Members: ").count(), 2);
        assert!(rendered.contains("Members: team-a"));
        assert!(rendered.contains("Members: team-b"));
    }

    #[test]
    fn doctor_alias_mismatch_text_includes_both_alias_values() {
        let rendered = render_doctor_alias_mismatches(&[DoctorAliasMismatch {
            team: "workspace".parse().expect("team"),
            member: "member-a".parse().expect("member"),
            config_alias: "alias-a".to_owned(),
            roster_alias: Some("stale-alias".to_owned()),
        }]);

        assert!(rendered.contains("Alias mismatches:"));
        assert!(rendered.contains("team=workspace member=member-a"));
        assert!(rendered.contains("config_alias=alias-a"));
        assert!(rendered.contains("roster_alias=stale-alias"));
    }

    #[test]
    fn herdr_doctor_rendering_covers_every_state_with_json_kind_and_remedy() {
        let endpoint = || {
            HerdrEndpointDisplay::from_relative(
                HerdrEndpointDisplayRoot::Configured,
                Path::new("herdr/socket"),
                false,
            )
            .expect("safe endpoint")
        };
        let version = || HerdrVersion::parse("0.8.2").expect("valid version");
        let states = vec![
            HerdrDoctorState::Ok {
                version: version(),
                protocol: 20,
            },
            HerdrDoctorState::NotConfigured,
            HerdrDoctorState::BinaryNotFound {
                searched: vec![PathBuf::from("/usr/bin/herdr")],
            },
            HerdrDoctorState::BinaryNotExecutable {
                path: PathBuf::from("/opt/herdr"),
                cause: "permission denied".to_owned(),
            },
            HerdrDoctorState::BelowMinimum {
                version: HerdrVersion::parse("0.7.9").expect("valid version"),
                minimum: version(),
            },
            HerdrDoctorState::ServerNotRunning {
                endpoint_named_by_herdr: Some(endpoint()),
            },
            HerdrDoctorState::ClientServerMismatch {
                client: Some(version()),
                server: Some(version()),
            },
            HerdrDoctorState::EndpointUnreachable {
                endpoint: endpoint(),
            },
            HerdrDoctorState::PermissionDenied {
                endpoint: endpoint(),
            },
            HerdrDoctorState::ProbeTimedOut {
                after: Duration::from_secs(1),
            },
            HerdrDoctorState::UnexpectedResponse {
                code: Some("malformed_reply".to_owned()),
                detail: "invalid JSON".to_owned(),
            },
            HerdrDoctorState::Other {
                code: AtmErrorCode::HerdrUnavailable,
                detail: "future compatibility code".to_owned(),
            },
        ];
        let report = HerdrDoctorReport {
            configured: Some(true),
            endpoints: states
                .into_iter()
                .map(|state| HerdrEndpointDoctorReport {
                    session: None,
                    provenance: HerdrEndpointProvenance::HerdrDefault,
                    transport: HerdrTransportKind::Cli,
                    endpoint: None,
                    binary: None,
                    remedy: state.remedy().to_owned(),
                    state,
                    capabilities: HerdrEndpointCapabilitiesDoctorReport::default(),
                    members: Vec::new(),
                    findings: Vec::new(),
                })
                .collect(),
            ..HerdrDoctorReport::default()
        };

        let rendered = render_doctor_herdr(&report);
        let json = serde_json::to_value(&report).expect("Herdr report serializes");

        assert_eq!(json["endpoints"].as_array().map(Vec::len), Some(12));
        for (index, endpoint) in report.endpoints.iter().enumerate() {
            assert!(
                json["endpoints"][index]["state"]["kind"].is_string(),
                "state {index} has a tagged JSON snapshot"
            );
            assert_eq!(json["endpoints"][index]["remedy"], endpoint.remedy);
            assert!(rendered.contains(&format!("Remedy: {}", endpoint.remedy)));
        }
    }

    #[test]
    fn herdr_endpoint_report_keeps_order_provenance_and_mismatch_capability() {
        let configured_endpoint = HerdrEndpointDisplay::from_relative(
            HerdrEndpointDisplayRoot::Configured,
            Path::new("private/socket"),
            false,
        )
        .expect("safe endpoint");
        let version = HerdrVersion::parse("0.8.2").expect("valid version");
        let report = HerdrDoctorReport {
            configured: Some(true),
            endpoints: vec![
                HerdrEndpointDoctorReport {
                    session: None,
                    provenance: HerdrEndpointProvenance::HerdrDefault,
                    transport: HerdrTransportKind::Cli,
                    endpoint: None,
                    binary: None,
                    state: HerdrDoctorState::ClientServerMismatch {
                        client: Some(version.clone()),
                        server: Some(version.clone()),
                    },
                    remedy: "Use Herdr handoff coordination".to_owned(),
                    capabilities: HerdrEndpointCapabilitiesDoctorReport {
                        live_handoff: Some(true),
                    },
                    members: Vec::new(),
                    findings: Vec::new(),
                },
                HerdrEndpointDoctorReport {
                    session: Some(HerdrSession::new("alpha").expect("valid session")),
                    provenance: HerdrEndpointProvenance::Session,
                    transport: HerdrTransportKind::Cli,
                    endpoint: None,
                    binary: None,
                    state: HerdrDoctorState::Ok {
                        version: version.clone(),
                        protocol: 20,
                    },
                    remedy: "none".to_owned(),
                    capabilities: HerdrEndpointCapabilitiesDoctorReport::default(),
                    members: Vec::new(),
                    findings: Vec::new(),
                },
                HerdrEndpointDoctorReport {
                    session: Some(HerdrSession::new("beta").expect("valid session")),
                    provenance: HerdrEndpointProvenance::SocketPath,
                    transport: HerdrTransportKind::Socket,
                    endpoint: Some(configured_endpoint),
                    binary: None,
                    state: HerdrDoctorState::PermissionDenied {
                        endpoint: HerdrEndpointDisplay::from_relative(
                            HerdrEndpointDisplayRoot::Configured,
                            Path::new("private/socket"),
                            false,
                        )
                        .expect("safe endpoint"),
                    },
                    remedy: "Align per-user ownership or permissions".to_owned(),
                    capabilities: HerdrEndpointCapabilitiesDoctorReport::default(),
                    members: Vec::new(),
                    findings: Vec::new(),
                },
            ],
            ..HerdrDoctorReport::default()
        };

        let rendered = render_doctor_herdr(&report);
        let json = serde_json::to_value(&report).expect("Herdr report serializes");

        assert_eq!(json["endpoints"][0]["provenance"], "herdr_default");
        assert_eq!(json["endpoints"][1]["provenance"], "session");
        assert_eq!(json["endpoints"][2]["provenance"], "socket_path");
        assert!(json["endpoints"][0]["endpoint"].is_null());
        assert_eq!(json["endpoints"][0]["capabilities"]["live_handoff"], true);
        assert_eq!(json["endpoints"][2]["endpoint"], "<configured>/socket");
        let default_index = rendered.find("Endpoint default").expect("default renders");
        let alpha_index = rendered.find("Endpoint alpha").expect("alpha renders");
        let beta_index = rendered.find("Endpoint beta").expect("beta renders");
        assert!(default_index < alpha_index && alpha_index < beta_index);
    }

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

        let stdout = render_send_stdout(&outcome, true, None).expect("JSON stdout");
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
    fn peer_send_output_names_confirmed_peer_host_in_both_formats() {
        let outcome: atm_core::send::SendOutcome = serde_json::from_value(json!({
            "action": "send",
            "team": "test-team",
            "agent": "recipient",
            "sender": "sender",
            "outcome": "sent",
            "message_id": "01KX5TEST00000000000000001",
            "requires_ack": false
        }))
        .expect("send outcome");
        let peer_host: HostName = "peer.example.test".parse().expect("peer host");

        let human = render_send_stdout(&outcome, false, Some(&peer_host)).expect("human output");
        assert!(human.contains("Delivered to peer host: peer.example.test"));

        let json_output =
            render_send_stdout(&outcome, true, Some(&peer_host)).expect("JSON output");
        let parsed: serde_json::Value = serde_json::from_str(&json_output).expect("JSON output");
        assert_eq!(parsed["delivered_to_peer_host"], "peer.example.test");
    }

    #[test]
    fn send_json_includes_task_completion_when_requested() {
        let outcome: atm_core::send::SendOutcome = serde_json::from_value(json!({
            "action": "send",
            "team": "test-team",
            "agent": "recipient",
            "sender": "assigner",
            "outcome": "sent",
            "message_id": "01KX5TEST00000000000000001",
            "requires_ack": false,
            "task_complete": "t-42"
        }))
        .expect("completion send outcome");

        let rendered = render_send_stdout(&outcome, true, None).expect("JSON stdout");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");
        assert_eq!(parsed["task_complete"], "t-42");
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
            legacy_literal_ip_peers: Vec::new(),
        });

        assert!(rendered.contains("sha256:public-fingerprint"));
        assert!(rendered.contains("peer.example.test:43101 (enabled)"));
        assert!(!rendered.contains("private_key_ref"));
        assert!(!rendered.contains("keychain:secret"));
    }

    /// ATM-QA-001 / RBQA-F001: the human-readable `atm doctor` text must
    /// surface every legacy literal-IP trusted-peer row and its exact
    /// remediation commands, not just the `--json` output.
    #[test]
    fn doctor_peer_text_surfaces_legacy_literal_ip_peers_and_remediation() {
        let rendered = render_doctor_peer_config(&PeerConfigDoctorReport {
            configured_interface_count: 1,
            enabled_interface_count: 1,
            certificate_fingerprint: Some("sha256:public-fingerprint".to_string()),
            trusted_peer_count: 2,
            enabled_trusted_peer_count: 1,
            trusted_peers: Vec::new(),
            validation_failure: None,
            legacy_literal_ip_peers: vec![
                atm_core::doctor::LegacyLiteralIpPeerDoctorReport {
                    host: "192.168.128.29".to_string(),
                    enabled: true,
                    migrate_command: "atm peer trust migrate --map 192.168.128.29=<hostname> --yes"
                        .to_string(),
                    revoke_command: "atm peer trust revoke --host 192.168.128.29 --yes".to_string(),
                },
                atm_core::doctor::LegacyLiteralIpPeerDoctorReport {
                    host: "10.0.0.5".to_string(),
                    enabled: false,
                    migrate_command: "atm peer trust migrate --map 10.0.0.5=<hostname> --yes"
                        .to_string(),
                    revoke_command: "atm peer trust revoke --host 10.0.0.5 --yes".to_string(),
                },
            ],
        });

        assert!(rendered.contains("192.168.128.29"));
        assert!(rendered.contains("(enabled)"));
        assert!(rendered.contains("atm peer trust migrate --map 192.168.128.29=<hostname> --yes"));
        assert!(rendered.contains("atm peer trust revoke --host 192.168.128.29 --yes"));

        assert!(rendered.contains("10.0.0.5"));
        assert!(rendered.contains("(disabled)"));
        assert!(rendered.contains("atm peer trust migrate --map 10.0.0.5=<hostname> --yes"));
        assert!(rendered.contains("atm peer trust revoke --host 10.0.0.5 --yes"));
    }
}
