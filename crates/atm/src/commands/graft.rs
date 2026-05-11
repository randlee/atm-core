use anyhow::Result;
use atm_core::graft::{
    GraftBatchLimit, GraftNudgeDrainRequest, GraftNudgeDrainResponse, GraftNudgeFetchRequest,
    GraftNudgeFetchResponse, GraftSessionId,
};
use clap::{Args, Subcommand};

use crate::composition::CliComposition;
use crate::observability::CliObservability;

#[derive(Debug, Args)]
/// Inspect or drain daemon-owned graft nudge state.
pub struct GraftCommand {
    #[command(subcommand)]
    mode: GraftMode,
}

#[derive(Debug, Subcommand)]
enum GraftMode {
    Fetch(GraftQueueCommand),
    Drain(GraftQueueCommand),
}

#[derive(Debug, Clone, Args)]
struct GraftQueueCommand {
    #[arg(long = "session-id")]
    session_id: String,

    #[arg(long, default_value_t = 64)]
    limit: usize,

    #[arg(long)]
    json: bool,
}

impl GraftCommand {
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let composition = CliComposition::bootstrap(observability)?;
        match self.mode {
            GraftMode::Fetch(command) => {
                let response = command.fetch(&composition)?;
                print_response(response, command.json)?;
            }
            GraftMode::Drain(command) => {
                let response = command.drain(&composition)?;
                print_response(response, command.json)?;
            }
        }
        Ok(())
    }
}

impl GraftQueueCommand {
    fn session_id(&self) -> Result<GraftSessionId> {
        Ok(GraftSessionId::new(self.session_id.clone())?)
    }

    fn limit(&self) -> Result<GraftBatchLimit> {
        Ok(GraftBatchLimit::new(self.limit)?)
    }

    fn fetch(&self, composition: &CliComposition) -> Result<GraftNudgeFetchResponse> {
        Ok(composition.fetch_graft_nudges(GraftNudgeFetchRequest {
            session_id: self.session_id()?,
            limit: self.limit()?,
        })?)
    }

    fn drain(&self, composition: &CliComposition) -> Result<GraftNudgeDrainResponse> {
        Ok(composition.drain_graft_nudges(GraftNudgeDrainRequest {
            session_id: self.session_id()?,
            limit: self.limit()?,
        })?)
    }
}

fn print_response(
    response: impl serde::Serialize + GraftQueueResponseView,
    json: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
    }

    for nudge in response.nudges() {
        println!("{}: {}", nudge.from, nudge.message);
    }
    println!(
        "remaining={} dropped_count={}",
        response.remaining(),
        response.dropped_count()
    );
    Ok(())
}

trait GraftQueueResponseView {
    fn nudges(&self) -> &[atm_core::graft::GraftNudge];
    fn remaining(&self) -> usize;
    fn dropped_count(&self) -> usize;
}

impl GraftQueueResponseView for GraftNudgeFetchResponse {
    fn nudges(&self) -> &[atm_core::graft::GraftNudge] {
        &self.nudges
    }

    fn remaining(&self) -> usize {
        self.remaining
    }

    fn dropped_count(&self) -> usize {
        self.dropped_count
    }
}

impl GraftQueueResponseView for GraftNudgeDrainResponse {
    fn nudges(&self) -> &[atm_core::graft::GraftNudge] {
        &self.nudges
    }

    fn remaining(&self) -> usize {
        self.remaining
    }

    fn dropped_count(&self) -> usize {
        self.dropped_count
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use atm_core::graft::{
        GraftNudge, GraftNudgeDrainResponse, GraftNudgeFetchResponse, GraftSessionId,
    };
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
    use atm_core::transport::testing::FakeClientTransport;

    use super::{GraftCommand, GraftMode, GraftQueueCommand};
    use crate::composition::CliComposition;
    use crate::observability::CliObservability;

    #[test]
    fn fetch_uses_graft_fetch_request_envelope() {
        let transport = Arc::new(FakeClientTransport::new(|request| match request {
            RequestEnvelope::GraftFetch(request) => {
                Ok(ResponseEnvelope::GraftFetch(GraftNudgeFetchResponse {
                    session_id: request.session_id,
                    nudges: Vec::new(),
                    remaining: 0,
                    dropped_count: 0,
                }))
            }
            other => panic!("unexpected request: {other:?}"),
        }));
        let observability = CliObservability::fallback();
        let composition = CliComposition::from_transport(transport, &observability);

        let command = GraftQueueCommand {
            session_id: "session-1".to_string(),
            limit: 8,
            json: true,
        };

        let response = command.fetch(&composition).expect("fetch response");
        assert_eq!(
            response.session_id,
            GraftSessionId::new("session-1").expect("session")
        );
    }

    #[test]
    fn drain_uses_graft_drain_request_envelope() {
        let transport = Arc::new(FakeClientTransport::new(|request| match request {
            RequestEnvelope::GraftDrain(request) => {
                Ok(ResponseEnvelope::GraftDrain(GraftNudgeDrainResponse {
                    session_id: request.session_id,
                    nudges: vec![GraftNudge {
                        message_id: atm_core::schema::LegacyMessageId::new(),
                        from: "sender".parse().expect("sender"),
                        message: "hello".to_string(),
                        received_at: atm_core::types::IsoTimestamp::now(),
                        task_id: None,
                    }],
                    remaining: 0,
                    dropped_count: 0,
                }))
            }
            other => panic!("unexpected request: {other:?}"),
        }));
        let observability = CliObservability::fallback();
        let composition = CliComposition::from_transport(transport, &observability);

        let command = GraftQueueCommand {
            session_id: "session-1".to_string(),
            limit: 8,
            json: false,
        };

        let response = command.drain(&composition).expect("drain response");
        assert_eq!(response.nudges.len(), 1);
        assert_eq!(response.nudges[0].message, "hello");
    }

    #[test]
    fn command_runs_fetch_mode_against_fake_transport() {
        let transport = Arc::new(FakeClientTransport::new(|request| match request {
            RequestEnvelope::GraftFetch(request) => {
                Ok(ResponseEnvelope::GraftFetch(GraftNudgeFetchResponse {
                    session_id: request.session_id,
                    nudges: Vec::new(),
                    remaining: 0,
                    dropped_count: 0,
                }))
            }
            other => panic!("unexpected request: {other:?}"),
        }));
        let observability = CliObservability::fallback();
        let composition = CliComposition::from_transport(transport, &observability);
        let command = GraftCommand {
            mode: GraftMode::Fetch(GraftQueueCommand {
                session_id: "session-1".to_string(),
                limit: 4,
                json: true,
            }),
        };

        match command.mode {
            GraftMode::Fetch(fetch) => {
                let response = fetch.fetch(&composition).expect("fetch");
                assert_eq!(response.remaining, 0);
            }
            GraftMode::Drain(_) => panic!("expected fetch mode"),
        }
    }
}
