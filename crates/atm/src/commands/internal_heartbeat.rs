use anyhow::Result;
use atm_core::api::ApiRequest;
use atm_core::caller_context::{
    CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride,
    resolve_cli_mutation_caller_context_with_overrides,
};
use atm_core::protocol::{
    HeartbeatActivity, RequestEnvelope, ResponseEnvelope, TeamMemberHeartbeatRequest,
};
use atm_core::types::IsoTimestamp;
use atm_daemon_client::resolve_daemon_local_ipc_endpoint;
use atm_http_runtime::SAME_HOST_REQUEST_DEADLINE;
use clap::{Args, ValueEnum};

use crate::observability::CliObservability;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum InternalHeartbeatActivity {
    ActiveToolUse,
    Idle,
    SessionEnded,
}

impl From<InternalHeartbeatActivity> for HeartbeatActivity {
    fn from(value: InternalHeartbeatActivity) -> Self {
        match value {
            InternalHeartbeatActivity::ActiveToolUse => Self::ActiveToolUse,
            InternalHeartbeatActivity::Idle => Self::Idle,
            InternalHeartbeatActivity::SessionEnded => Self::SessionEnded,
        }
    }
}

#[derive(Debug, Args)]
#[command(name = "_internal-heartbeat", hide = true)]
pub struct InternalHeartbeatCommand {
    #[arg(long, value_enum)]
    pub activity: InternalHeartbeatActivity,
    #[arg(long)]
    pub team: Option<String>,
    #[arg(long = "as")]
    pub actor: Option<String>,
}

impl InternalHeartbeatCommand {
    pub async fn run(self, _observability: &CliObservability) -> Result<()> {
        let caller = resolve_cli_mutation_caller_context_with_overrides(CallerContextOverrides {
            identity_override: self.actor.as_deref().map(CallerIdentityOverride),
            chat_id_override: None,
            team_override: self.team.as_deref().map(CallerTeamOverride),
        })?;
        let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: caller.caller_team,
            member: caller.caller_identity,
            pid: std::process::id(),
            observed_at: IsoTimestamp::now(),
            activity: self.activity.into(),
            session_id: caller
                .activity_observation
                .and_then(|observation| observation.session_id),
        });
        let endpoint = match resolve_daemon_local_ipc_endpoint() {
            Ok(endpoint) => endpoint,
            Err(error) if error.is_daemon_unavailable() => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let transport = match atm_http_runtime::preferred_local_client(
            endpoint.as_ref(),
            SAME_HOST_REQUEST_DEADLINE,
        ) {
            Ok(transport) => transport,
            Err(error) if error.is_daemon_unavailable() => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let response = match transport.execute(ApiRequest::new(request)).await {
            Ok(response) => response.into_inner(),
            Err(error) if error.is_daemon_unavailable() => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        match response {
            ResponseEnvelope::Heartbeat(_) => Ok(()),
            ResponseEnvelope::Error(error) => Err(error.into()),
            other => Err(anyhow::anyhow!(
                "unexpected response to internal heartbeat: {other:?}"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InternalHeartbeatActivity;
    use clap::ValueEnum;

    #[test]
    fn heartbeat_cli_accepts_all_three_activity_values() {
        assert_eq!(InternalHeartbeatActivity::value_variants().len(), 3);
    }
}
