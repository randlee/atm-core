use std::path::Path;

use serde_json::Map;
use tracing::warn;

use crate::delivery_execution::execute_delivery_plan;
use crate::delivery_plan::{
    DeliveryPlan, delivery_plan_disposition, delivery_target_for_snapshot,
    logical_messages_from_persistence,
};
use crate::delivery_policy::{DeliveryPolicyCoordinator, DeliveryRecipientSnapshot};
use crate::error::{AtmError, AtmErrorCode};
use crate::roles::ROLE_TEAM_LEAD;
use crate::schema::{AtmMessageId, MessageEnvelope};
use crate::service_runtime::RetainedServiceRuntime;
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::types::{AgentName, IsoTimestamp, TeamName};

use super::{
    DeliveryPersistenceResult, ResolvedRecipient, SendRequest, WarningEntry,
    persist_message_and_seed_workflow,
};

pub(super) fn warn_missing_team_config<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &SendRequest,
    recipient: &ResolvedRecipient,
    team_dir: &Path,
    inbox_path: &Path,
    warnings: &mut Vec<WarningEntry>,
) -> Result<(), AtmError> {
    if !inbox_path.exists() {
        return Err(build_missing_config_error(team_dir, inbox_path));
    }
    warnings.push(missing_config_warning(recipient, team_dir));
    warn!(
        code = %AtmErrorCode::WarningMissingTeamConfigFallback,
        config_path = %team_dir.join("config.json").display(),
        recipient = %recipient.agent,
        team = %recipient.team,
        "send used existing inbox fallback; team config is missing"
    );
    if !request.dry_run {
        notify_team_lead_missing_config(
            runtime,
            &request.home_dir,
            team_dir,
            &recipient.team,
            &recipient.agent,
        );
    }
    Ok(())
}

fn build_missing_config_error(team_dir: &Path, inbox_path: &Path) -> AtmError {
    AtmError::missing_document(format!(
        "team config is missing at {} and inbox {} does not exist, so send cannot safely proceed",
        team_dir.join("config.json").display(),
        inbox_path.display()
    ))
    .with_recovery(
        "Restore config.json for the team or create the intended inbox by an approved workflow before retrying.",
    )
}

fn missing_config_warning(recipient: &ResolvedRecipient, team_dir: &Path) -> WarningEntry {
    WarningEntry::new(
        format!(
            "warning: team config is missing at {}; send used existing inbox fallback for {}@{}.",
            team_dir.join("config.json").display(),
            recipient.agent,
            recipient.team
        ),
        Some("Restore the team config."),
    )
}

fn notify_team_lead_missing_config(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    home_dir: &Path,
    team_dir: &Path,
    team: &TeamName,
    recipient: &AgentName,
) {
    let alert_key = super::alert_state::missing_team_config_alert_key(team_dir);
    if !super::alert_state::register_missing_team_config_alert(home_dir, &alert_key) {
        return;
    }

    let team_lead_agent = AgentName::from_validated(ROLE_TEAM_LEAD);
    let team_lead_inbox = match runtime.inbox_path(home_dir, team, &team_lead_agent) {
        Ok(path) => path,
        Err(error) => {
            warn!(
                code = %AtmErrorCode::WarningMissingTeamConfigFallback,
                %error,
                team = %team,
                "failed to resolve reserved missing-config inbox for notice"
            );
            return;
        }
    };

    let notice = build_missing_config_notice(
        team,
        recipient,
        &team_dir.join("config.json"),
        IsoTimestamp::now(),
    );
    let snapshot = match resolve_team_lead_snapshot(runtime, team) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            warn!(
                code = %AtmErrorCode::WarningMissingTeamConfigFallback,
                %error,
                team = %team,
                "failed to resolve reserved missing-config delivery snapshot"
            );
            return;
        }
    };
    if let Err(error) =
        persist_missing_config_notice(runtime, home_dir, &snapshot, &team_lead_inbox, &notice)
    {
        warn!(
            code = %AtmErrorCode::WarningMissingTeamConfigFallback,
            %error,
            path = %team_lead_inbox.display(),
            team = %team,
            "failed to persist missing-config notice via shared mailbox/workflow commit path"
        );
    }
}

fn build_missing_config_notice(
    team: &TeamName,
    recipient: &AgentName,
    config_path: &Path,
    timestamp: IsoTimestamp,
) -> MessageEnvelope {
    MessageEnvelope {
        from: AgentName::from_validated("atm-identity-missing"),
        text: format!(
            "ATM warning: send used existing inbox fallback for {recipient}@{team} because team config is missing at {}. Please restore config.json.",
            config_path.display()
        ),
        timestamp,
        read: false,
        source_team: Some(team.clone()),
        summary: Some(format!(
            "ATM warning: missing team config fallback used for {recipient}@{team}"
        )),
        message_id: Some(AtmMessageId::new()),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        extra: Map::new(),
    }
}

fn resolve_team_lead_snapshot(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    team: &TeamName,
) -> Result<DeliveryRecipientSnapshot, AtmError> {
    DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(
        runtime,
        team,
        &AgentName::from_validated(ROLE_TEAM_LEAD),
    )
}

fn persist_missing_config_notice(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    home_dir: &Path,
    snapshot: &DeliveryRecipientSnapshot,
    team_lead_inbox: &Path,
    notice: &MessageEnvelope,
) -> Result<(), AtmError> {
    let persistence = persist_message_and_seed_workflow(
        runtime,
        home_dir,
        snapshot,
        team_lead_inbox,
        notice,
        true,
    )?;
    let plan = build_missing_config_delivery_plan(snapshot, team_lead_inbox, &persistence)?;
    let execution = execute_delivery_plan(runtime, None, &plan)?;
    for warning in plan.warnings {
        warn!(message = %warning.message, "compatibility append degraded for missing-config notice");
    }
    for warning in execution.warnings {
        warn!(message = %warning.message, "compatibility append degraded for missing-config notice");
    }
    Ok(())
}

fn build_missing_config_delivery_plan(
    snapshot: &DeliveryRecipientSnapshot,
    team_lead_inbox: &Path,
    persistence: &DeliveryPersistenceResult,
) -> Result<DeliveryPlan, AtmError> {
    Ok(DeliveryPlan::new(
        delivery_plan_disposition(persistence.disposition),
        delivery_target_for_snapshot(team_lead_inbox, snapshot),
        ResolvedRecipient {
            agent: AgentName::from_validated(ROLE_TEAM_LEAD),
            team: snapshot.team.clone(),
        },
        snapshot.recipient_pane_id.clone(),
        logical_messages_from_persistence(persistence, false, false)
            .map_err(|error| AtmError::mailbox_write(error.to_string()))?,
        persistence.warnings.clone(),
    ))
}
