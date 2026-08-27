use anyhow::Result;
use atm_core::api::ApiRequest;
use atm_core::caller_context::{
    CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride,
    resolve_cli_mutation_caller_context_with_overrides,
};
use atm_core::protocol::{QueueGetNextRequest, RequestEnvelope, ResponseEnvelope};
use atm_daemon_client::resolve_daemon_local_ipc_endpoint;
use atm_http_runtime::SAME_HOST_REQUEST_DEADLINE;
use clap::Args;

use crate::observability::CliObservability;

#[derive(Debug, Args)]
#[command(name = "_internal-queue-get", hide = true)]
pub struct InternalQueueGetCommand {
    #[arg(long)]
    pub team: Option<String>,
    #[arg(long = "as")]
    pub actor: Option<String>,
}

impl InternalQueueGetCommand {
    pub async fn run(self, _observability: &CliObservability) -> Result<()> {
        let caller = resolve_cli_mutation_caller_context_with_overrides(CallerContextOverrides {
            identity_override: self.actor.as_deref().map(CallerIdentityOverride),
            chat_id_override: None,
            team_override: self.team.as_deref().map(CallerTeamOverride),
        })?;
        let request = RequestEnvelope::QueueGetNext(QueueGetNextRequest {
            team: caller.caller_team,
            member: caller.caller_identity,
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
            ResponseEnvelope::QueueGetNext(response) => {
                for message in response.messages {
                    println!("{}", serde_json::to_string(&message)?);
                }
                Ok(())
            }
            ResponseEnvelope::Error(error) => Err(error.into()),
            other => Err(anyhow::anyhow!(
                "unexpected response to internal queue get: {other:?}"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::commands::Cli;

    #[test]
    fn queue_get_cli_has_no_target_member_argument() {
        let result = Cli::try_parse_from(["atm", "_internal-queue-get", "--member", "other"]);
        assert!(result.is_err());
    }
}
