use anyhow::Result;
use atm_core::api::ApiRequest;
use atm_core::caller_context::{
    CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride,
    resolve_cli_mutation_caller_context_with_overrides,
};
use atm_core::error::AtmError;
use atm_core::protocol::{QueueGetNextRequest, RequestEnvelope, ResponseEnvelope};
use atm_daemon_client::{DaemonLocalIpcEndpoint, resolve_daemon_local_ipc_endpoint};
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
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        self.run_with_endpoint(resolve_daemon_local_ipc_endpoint(), observability)
            .await
    }

    /// Testable core of [`Self::run`].
    ///
    /// Production always resolves `endpoint` from
    /// [`resolve_daemon_local_ipc_endpoint`], the OS-account singleton
    /// runtime path. Accepting it as a parameter lets tests simulate daemon
    /// unavailability (a closed socket / refused connect) against an
    /// isolated, caller-controlled endpoint instead of that shared,
    /// process-wide singleton.
    async fn run_with_endpoint(
        self,
        endpoint: Result<DaemonLocalIpcEndpoint, AtmError>,
        _observability: &CliObservability,
    ) -> Result<()> {
        let caller = resolve_cli_mutation_caller_context_with_overrides(CallerContextOverrides {
            identity_override: self.actor.as_deref().map(CallerIdentityOverride),
            chat_id_override: None,
            team_override: self.team.as_deref().map(CallerTeamOverride),
        })?;
        let request = RequestEnvelope::QueueGetNext(QueueGetNextRequest {
            team: caller.caller_team,
            member: caller.caller_identity,
        });
        let endpoint = match endpoint {
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
    use atm_core::test_support::EnvGuard;
    use clap::Parser;
    use serial_test::serial;

    use super::{DaemonLocalIpcEndpoint, InternalQueueGetCommand};
    use crate::commands::Cli;
    use crate::observability::CliObservability;

    #[test]
    fn queue_get_cli_has_no_target_member_argument() {
        let result = Cli::try_parse_from(["atm", "_internal-queue-get", "--member", "other"]);
        assert!(result.is_err());
    }

    /// AC8: a real, bounded-timeout exit-0 proof for the queue-get CLI
    /// surface. This resolves a valid, isolated endpoint path with nothing
    /// listening (a closed socket, simulating daemon unavailability), so the
    /// real `preferred_local_client(...).execute(...)` connect-refusal path
    /// runs, not the earlier `resolve_daemon_local_ipc_endpoint()`
    /// short-circuit.
    #[tokio::test]
    #[serial(env)]
    async fn queue_get_exits_ok_within_the_bounded_timeout_when_the_daemon_is_unavailable() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some("aq25-ac8-queue-get-agent"))]);
        let temporary_root = tempfile::tempdir().expect("isolated runtime root");
        let endpoint = DaemonLocalIpcEndpoint::new(temporary_root.path().join("local-http.json"))
            .expect("isolated endpoint path");
        let command = InternalQueueGetCommand {
            team: Some("aq25-ac8-team".to_owned()),
            actor: None,
        };

        let started = std::time::Instant::now();
        command
            .run_with_endpoint(Ok(endpoint), &CliObservability::fallback())
            .await
            .expect("a daemon-down queue-get must fail open and exit 0");
        assert!(
            started.elapsed() < atm_http_runtime::SAME_HOST_REQUEST_DEADLINE,
            "the fail-open path must return well inside the bounded connect budget, not hang past it"
        );
    }
}
