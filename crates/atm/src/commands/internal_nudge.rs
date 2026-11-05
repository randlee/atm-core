use std::time::Duration;

use anyhow::Result;
use atm_core::api::{ApiRequest, DaemonApiClient};
use atm_core::boundary::{
    BuiltInNudgeSinkTarget, InternalNudgeEnvelope, PostSendHookEvent, ResolvedBuiltInNudgeTemplate,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::graft::{
    GraftPostSendRequest, GraftPostSendResponse, deliver_graft_post_send,
    graft_receiver_not_registered_error,
};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
#[cfg(test)]
use atm_core::send::qualified_nudge_sender_identity;
use atm_core::send::render_resolved_built_in_nudge;
use atm_daemon_client::{resolve_daemon_local_ipc_endpoint, unexpected_response};
use atm_http_runtime::SAME_HOST_REQUEST_DEADLINE;
use clap::Args;

use crate::observability::CliObservability;

mod tmux;
use tmux::TmuxNudgeSink;

/// Loopback connect budget for delivering a post-send nudge to a graft receiver.
const GRAFT_POST_SEND_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
/// Bounded request/response budget once connected to a graft receiver.
const GRAFT_POST_SEND_IO_DEADLINE: Duration = Duration::from_secs(3);

#[cfg(test)]
use std::collections::BTreeMap;

const INTERNAL_NUDGE_ENV: &str = "ATM_INTERNAL_NUDGE";

#[derive(Debug, Args)]
#[command(hide = true)]
pub struct InternalNudgeCommand;

impl InternalNudgeCommand {
    pub async fn run(self, _observability: &CliObservability) -> Result<()> {
        let input = InternalNudgeInput::from_env()?;
        let Some(template) = render_resolved_built_in_nudge(&input.event, &input.template)? else {
            return Ok(());
        };
        match input.sink_target {
            BuiltInNudgeSinkTarget::Tmux => {
                tokio::task::spawn_blocking(move || TmuxNudgeSink.deliver(&input.event, &template))
                    .await??;
            }
            BuiltInNudgeSinkTarget::Graft => {
                GraftNudgeSink.deliver(&input.event, &template).await?
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct InternalNudgeInput {
    event: PostSendHookEvent,
    sink_target: BuiltInNudgeSinkTarget,
    template: ResolvedBuiltInNudgeTemplate,
}

impl InternalNudgeInput {
    fn from_env() -> Result<Self> {
        let raw_payload = std::env::var(INTERNAL_NUDGE_ENV).map_err(|source| {
            AtmError::validation("missing ATM_INTERNAL_NUDGE payload for built-in post-send nudge")
                .with_cause(source)
        })?;
        let payload: InternalNudgeEnvelope =
            serde_json::from_str(&raw_payload).map_err(|source| {
                AtmError::validation(
                    "failed to decode ATM_INTERNAL_NUDGE payload for built-in nudge",
                )
                .with_cause(source)
            })?;
        Ok(Self {
            event: payload.event,
            sink_target: payload.sink_target,
            template: payload.template,
        })
    }

    #[cfg(test)]
    fn render_values(&self) -> BTreeMap<&'static str, String> {
        BTreeMap::from([
            ("from", qualified_nudge_sender_identity(&self.event)),
            ("team", self.event.recipient_team.to_string()),
            ("message_id", self.event.message_id.to_string()),
            ("description", self.event.description.clone()),
            (
                "task_id",
                self.event
                    .task_id
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            ),
        ])
    }
}

#[cfg(test)]
fn render_template(template: &str, values: &BTreeMap<&'static str, String>) -> Result<String> {
    if template.contains("{%") || template.contains("%}") {
        return Err(AtmError::validation(
            "built-in nudge templates do not support Jinja or conditional blocks",
        )
        .into());
    }
    let mut output = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            return Err(AtmError::validation("unterminated built-in nudge placeholder").into());
        };
        let key = after_start[..end].trim();
        let Some(value) = values.get(key) else {
            return Err(AtmError::validation(format!(
                "unsupported built-in nudge placeholder `{{{{{key}}}}}`"
            ))
            .into());
        };
        output.push_str(value);
        rest = &after_start[end + 2..];
    }
    output.push_str(rest);
    Ok(output)
}

struct GraftNudgeSink;

impl GraftNudgeSink {
    async fn deliver(&self, event: &PostSendHookEvent, rendered_nudge: &str) -> Result<()> {
        let lease = lookup_receiver(&event.recipient_team, &event.recipient).await?;
        let request = GraftPostSendRequest {
            event: event.clone(),
            kind: atm_core::boundary::NudgeKind::Steer,
            rendered_nudge: rendered_nudge.to_string(),
        };
        let response = deliver_graft_post_send(
            lease.endpoint,
            &lease.capability,
            &request,
            GRAFT_POST_SEND_CONNECT_DEADLINE,
            GRAFT_POST_SEND_IO_DEADLINE,
        )
        .map_err(|error| {
            if error.code() == AtmErrorCode::PostSendGraftUnavailable {
                error
            } else {
                AtmError::for_code(AtmErrorCode::PostSendGraftUnavailable).with_cause(error)
            }
        })?;
        match response {
            GraftPostSendResponse::Delivered => Ok(()),
            GraftPostSendResponse::Error(error) => Err(error.into()),
        }
    }
}

async fn lookup_receiver(
    team: &atm_core::types::TeamName,
    agent: &atm_core::types::AgentName,
) -> Result<atm_storage::GraftReceiverLease, AtmError> {
    let endpoint = resolve_daemon_local_ipc_endpoint()?;
    let transport =
        atm_http_runtime::preferred_local_client(endpoint.as_ref(), SAME_HOST_REQUEST_DEADLINE)?;
    lookup_receiver_via(transport.as_ref(), team, agent).await
}

/// Executes the `GraftReceiverLookup` round trip against any daemon client
/// transport, decoupled from local endpoint resolution so it can be
/// exercised against a fake or router-backed transport in tests without a
/// running daemon (AC7).
async fn lookup_receiver_via(
    transport: &dyn DaemonApiClient,
    team: &atm_core::types::TeamName,
    agent: &atm_core::types::AgentName,
) -> Result<atm_storage::GraftReceiverLease, AtmError> {
    let response = transport
        .execute(ApiRequest::new(RequestEnvelope::GraftReceiverLookup {
            team: team.clone(),
            agent: agent.clone(),
        }))
        .await?
        .into_inner();
    match response {
        ResponseEnvelope::GraftReceiverLookup(Some(lease)) => Ok(lease),
        ResponseEnvelope::GraftReceiverLookup(None) => {
            Err(graft_receiver_not_registered_error(team, agent))
        }
        ResponseEnvelope::Error(error) => Err(error),
        other => Err(unexpected_response("graft receiver lookup", other)),
    }
}

#[cfg(test)]
mod tests {

    use atm_core::boundary::{
        BuiltInNudgeSinkTarget, BuiltInNudgeTemplateKind, InternalNudgeEnvelope, NudgeKind,
        PostSendHookEvent, ResolvedBuiltInNudgeTemplate, TMUX_DOUBLE_ENTER_DELAY,
        built_in_nudge_template_kind_from_post_send_event,
    };
    use atm_core::send::default_template;
    use atm_core::test_support::{EnvGuard, TEST_ARCH_CTM, TEST_LEAD, TEST_TEAM};
    use serial_test::serial;

    use atm_core::graft::graft_receiver_not_registered_error;
    use atm_core::protocol::{OwnerGeneration, RequestEnvelope, ResponseEnvelope};
    use atm_core::transport::testing::FakeClientTransport;

    use super::{
        INTERNAL_NUDGE_ENV, InternalNudgeCommand, InternalNudgeInput, lookup_receiver_via,
        render_template,
    };

    fn base_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: TEST_LEAD.parse().expect("sender"),
            sender_chat_id: None,
            sender_team: TEST_TEAM.parse().expect("team"),
            sender_host: None,
            recipient: TEST_ARCH_CTM.parse().expect("recipient"),
            recipient_team: TEST_TEAM.parse().expect("team"),
            message_id: "01KX1TEST00000000000000000".parse().expect("message id"),
            description: "review failing smoke lane".to_string(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            task_transition: None,
            recipient_pane_id: Some(atm_core::types::PaneId::from_cli("%9").expect("pane")),
        }
    }

    #[test]
    fn built_in_templates_keep_ack_payloads_compact() {
        assert_eq!(
            default_template(BuiltInNudgeTemplateKind::Acknowledge),
            "<atm kind=\"ack\" from=\"{{from}}\" message-id=\"{{message_id}}\"/>"
        );
    }

    #[test]
    fn built_in_template_kind_selection_covers_non_task_paths() {
        let mut event = base_event();
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event, NudgeKind::Steer),
            BuiltInNudgeTemplateKind::Delivery
        );
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event, NudgeKind::Queue),
            BuiltInNudgeTemplateKind::Queue
        );
        event.requires_ack = true;
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event, NudgeKind::Steer),
            BuiltInNudgeTemplateKind::DeliveryAck
        );
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event, NudgeKind::Queue),
            BuiltInNudgeTemplateKind::QueueAck
        );
        event.requires_ack = false;
        event.task_id = Some("AD.21".parse().expect("task"));
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event, NudgeKind::Queue),
            BuiltInNudgeTemplateKind::Queue
        );
        event.is_ack = true;
        event.requires_ack = false;
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event, NudgeKind::Steer),
            BuiltInNudgeTemplateKind::Acknowledge
        );
        event.task_id = None;
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event, NudgeKind::Steer),
            BuiltInNudgeTemplateKind::Acknowledge
        );
    }

    #[test]
    fn render_template_replaces_only_supported_placeholders() {
        let rendered = render_template(
            "<atm from=\"{{from}}\" task-id=\"{{task_id}}\">{{description}}</atm>",
            &InternalNudgeInput {
                event: base_event(),
                sink_target: BuiltInNudgeSinkTarget::Tmux,
                template: ResolvedBuiltInNudgeTemplate {
                    kind: BuiltInNudgeTemplateKind::Delivery,
                    body: Some(default_template(BuiltInNudgeTemplateKind::Delivery).to_string()),
                },
            }
            .render_values(),
        )
        .expect("render");
        assert!(rendered.contains(&format!("{TEST_LEAD}@{TEST_TEAM}")));
        assert!(rendered.contains("review failing smoke lane"));
        assert!(rendered.contains("task-id=\"\""));
    }

    #[test]
    fn render_template_rejects_unknown_placeholder() {
        let error = render_template(
            "<atm>{{unknown}}</atm>",
            &InternalNudgeInput {
                event: base_event(),
                sink_target: BuiltInNudgeSinkTarget::Tmux,
                template: ResolvedBuiltInNudgeTemplate {
                    kind: BuiltInNudgeTemplateKind::Delivery,
                    body: Some(default_template(BuiltInNudgeTemplateKind::Delivery).to_string()),
                },
            }
            .render_values(),
        )
        .expect_err("unknown placeholder");
        assert!(
            error
                .to_string()
                .contains("unsupported built-in nudge placeholder")
        );
    }

    #[test]
    #[serial(env)]
    fn internal_nudge_input_reads_resolved_envelope() {
        let payload = serde_json::to_string(&InternalNudgeEnvelope {
            event: PostSendHookEvent {
                requires_ack: true,
                task_id: Some("AD.21".parse().expect("task")),
                ..base_event()
            },
            sink_target: BuiltInNudgeSinkTarget::Tmux,
            template: ResolvedBuiltInNudgeTemplate {
                kind: BuiltInNudgeTemplateKind::TaskReady,
                body: Some("<atm from=\"{{from}}\" message-id=\"{{message_id}}\"/>".to_string()),
            },
        })
        .expect("serialize envelope");
        let payload_value = payload.to_string();
        let _env = EnvGuard::set_many([(INTERNAL_NUDGE_ENV, Some(payload_value.as_str()))]);

        let input = InternalNudgeInput::from_env().expect("input");

        assert_eq!(input.sink_target, BuiltInNudgeSinkTarget::Tmux);
        assert_eq!(input.event.task_id.expect("task").as_str(), "AD.21");
        assert_eq!(input.template.kind, BuiltInNudgeTemplateKind::TaskReady);
        assert_eq!(
            input.template.body.as_deref(),
            Some("<atm from=\"{{from}}\" message-id=\"{{message_id}}\"/>")
        );
    }

    #[test]
    #[serial(env)]
    fn internal_nudge_input_accepts_explicit_disabled_template_state() {
        let payload = serde_json::to_string(&InternalNudgeEnvelope {
            event: base_event(),
            sink_target: BuiltInNudgeSinkTarget::Tmux,
            template: ResolvedBuiltInNudgeTemplate {
                kind: BuiltInNudgeTemplateKind::Delivery,
                body: None,
            },
        })
        .expect("serialize envelope");
        let _env = EnvGuard::set_many([(INTERNAL_NUDGE_ENV, Some(payload.as_str()))]);

        let input = InternalNudgeInput::from_env().expect("input");

        assert_eq!(input.template.body, None);
    }

    #[tokio::test]
    #[serial(env)]
    async fn internal_nudge_run_skips_delivery_when_template_is_explicitly_disabled() {
        let payload = serde_json::to_string(&InternalNudgeEnvelope {
            event: base_event(),
            sink_target: BuiltInNudgeSinkTarget::Tmux,
            template: ResolvedBuiltInNudgeTemplate {
                kind: BuiltInNudgeTemplateKind::Delivery,
                body: None,
            },
        })
        .expect("serialize envelope");
        let _env = EnvGuard::set_many([(INTERNAL_NUDGE_ENV, Some(payload.as_str()))]);

        InternalNudgeCommand
            .run(&crate::observability::CliObservability::fallback())
            .await
            .expect("disabled template should short-circuit");
    }

    #[test]
    fn tmux_sink_uses_double_enter_sequence() {
        let mut commands = Vec::new();
        let mut delays = Vec::new();
        super::TmuxNudgeSink
            .deliver_with(
                &base_event(),
                "<atm/>",
                |command, _action| {
                    commands.push(
                        command
                            .get_args()
                            .map(|arg| arg.to_string_lossy().into_owned())
                            .collect::<Vec<_>>(),
                    );
                    Ok(())
                },
                |duration| delays.push(duration),
            )
            .expect("deliver");
        assert_eq!(
            commands,
            vec![
                vec!["send-keys", "-t", "%9", "-l", "<atm/>"],
                vec!["send-keys", "-t", "%9", "Enter"],
                vec!["send-keys", "-t", "%9", "Enter"],
            ]
        );
        assert_eq!(delays, vec![TMUX_DOUBLE_ENTER_DELAY]);
    }

    #[test]
    fn tmux_sink_stops_after_a_command_failure() {
        let mut calls = 0;
        let mut delays = Vec::new();
        let result = super::TmuxNudgeSink.deliver_with(
            &base_event(),
            "<atm/>",
            |_command, _action| {
                calls += 1;
                Err(anyhow::anyhow!("tmux failed"))
            },
            |duration| delays.push(duration),
        );
        assert!(result.is_err());
        assert_eq!(calls, 1);
        assert!(delays.is_empty());
    }

    #[test]
    fn tmux_sink_rejects_a_missing_pane_without_running_commands() {
        let mut event = base_event();
        event.recipient_pane_id = None;
        let mut calls = 0;
        let result = super::TmuxNudgeSink.deliver_with(
            &event,
            "<atm/>",
            |_command, _action| {
                calls += 1;
                Ok(())
            },
            |_duration| panic!("missing pane must not wait"),
        );
        assert!(result.is_err());
        assert_eq!(calls, 0);
    }

    // Client-side translation-contract unit test (supplements AC7, does not
    // stand in for it): `lookup_receiver_via` is the exact request/response
    // contract `GraftNudgeSink::deliver` uses. This exercises its Some/None
    // -> Ok(lease)/Err(not-registered) mapping against a `FakeClientTransport`
    // for fast, deterministic coverage of the translation logic itself. AC7's
    // real-route coverage — proving this same contract holds against the
    // actual `atm-http-runtime` HTTP route, not a hand-rolled fake — lives in
    // `atm-http-runtime`'s
    // `storage_and_nudge_router::tests::graft_receiver_lookup_round_trips_hit_and_absent_lease_through_the_real_http_route`
    // (PR #1048, QA-2).
    #[tokio::test]
    async fn graft_receiver_lookup_round_trips_hit_and_absent_lease() {
        let team: atm_core::types::TeamName = TEST_TEAM.parse().expect("team");
        let agent: atm_core::types::AgentName = TEST_ARCH_CTM.parse().expect("agent");
        let owner_generation =
            OwnerGeneration::new("01J00000000000000000000040").expect("owner generation");
        let lease = atm_storage::GraftReceiverLease {
            endpoint: "127.0.0.1:43120".parse().expect("endpoint"),
            capability: atm_core::local_http::LocalCapability::generate().expect("capability"),
            owner_generation,
            registered_at: chrono::Utc::now(),
            last_seen_at: chrono::Utc::now(),
            unreachable_since: None,
        };

        let hit_lease = lease.clone();
        let hit_transport = FakeClientTransport::new(move |request| match request {
            RequestEnvelope::GraftReceiverLookup { .. } => Ok(
                ResponseEnvelope::GraftReceiverLookup(Some(hit_lease.clone())),
            ),
            other => panic!("unexpected request in graft receiver lookup test: {other:?}"),
        });
        let resolved = lookup_receiver_via(&hit_transport, &team, &agent)
            .await
            .expect("hit lookup must resolve the lease");
        assert_eq!(resolved, lease);

        let miss_transport = FakeClientTransport::new(|request| match request {
            RequestEnvelope::GraftReceiverLookup { .. } => {
                Ok(ResponseEnvelope::GraftReceiverLookup(None))
            }
            other => panic!("unexpected request in graft receiver lookup test: {other:?}"),
        });
        let error = lookup_receiver_via(&miss_transport, &team, &agent)
            .await
            .expect_err("an absent lease must surface the not-registered error");
        assert_eq!(
            error.code(),
            graft_receiver_not_registered_error(&team, &agent).code()
        );
        assert!(error.message().contains(TEST_ARCH_CTM));
    }
}
