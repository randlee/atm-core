use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use atm_core::boundary::{
    BuiltInNudgeSinkTarget, InternalNudgeEnvelope, PostSendHookEvent, ResolvedBuiltInNudgeTemplate,
    TMUX_DOUBLE_ENTER_DELAY, TMUX_NUDGE_CONFIRM_KEY,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::graft::{
    GraftPostSendRequest, GraftPostSendResponse, deliver_graft_post_send,
    graft_receiver_record_path_from_home,
};
use atm_core::home;
#[cfg(test)]
use atm_core::send::qualified_nudge_sender_identity;
use atm_core::send::render_resolved_built_in_nudge;
use clap::Args;

use crate::observability::CliObservability;

/// Loopback connect budget for delivering a post-send nudge to a graft receiver.
const GRAFT_POST_SEND_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
/// Bounded request/response budget once connected to a graft receiver.
const GRAFT_POST_SEND_IO_DEADLINE: Duration = Duration::from_secs(3);

#[cfg(test)]
use std::collections::BTreeMap;

const INTERNAL_NUDGE_ENV: &str = "ATM_INTERNAL_NUDGE";
const TMUX_SEND_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const TMUX_PROGRAM_ENV: &str = "ATM_TEST_TMUX_BIN";

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
            BuiltInNudgeSinkTarget::Tmux => TmuxNudgeSink.deliver(&input.event, &template)?,
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
        let raw_payload = std::env::var(INTERNAL_NUDGE_ENV).map_err(|_| {
            AtmError::validation("missing ATM_INTERNAL_NUDGE payload for built-in post-send nudge")
        })?;
        let payload: InternalNudgeEnvelope =
            serde_json::from_str(&raw_payload).map_err(|_source| {
                AtmError::validation(
                    "failed to decode ATM_INTERNAL_NUDGE payload for built-in nudge",
                )
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

struct TmuxNudgeSink;

impl TmuxNudgeSink {
    fn deliver(&self, event: &PostSendHookEvent, rendered: &str) -> Result<()> {
        let pane_id = event
            .recipient_pane_id
            .as_ref()
            .ok_or_else(|| AtmError::for_code(AtmErrorCode::PostSendPaneMissing))?;
        run_tmux_command(
            {
                let mut command = tmux_command();
                command.args(["send-keys", "-t", pane_id.as_str(), "-l", rendered]);
                command
            },
            "send literal nudge",
        )?;
        run_tmux_command(
            {
                let mut command = tmux_command();
                command.args(["send-keys", "-t", pane_id.as_str(), TMUX_NUDGE_CONFIRM_KEY]);
                command
            },
            "send first Enter to nudge pane",
        )?;
        thread::sleep(TMUX_DOUBLE_ENTER_DELAY);
        run_tmux_command(
            {
                let mut command = tmux_command();
                command.args(["send-keys", "-t", pane_id.as_str(), TMUX_NUDGE_CONFIRM_KEY]);
                command
            },
            "send second Enter to nudge pane",
        )?;
        Ok(())
    }
}

struct GraftNudgeSink;

impl GraftNudgeSink {
    async fn deliver(&self, event: &PostSendHookEvent, rendered_nudge: &str) -> Result<()> {
        let home_dir = home::atm_home()?;
        let record_path = graft_receiver_record_path_from_home(
            &home_dir,
            &event.recipient_team,
            &event.recipient,
        );
        let request = GraftPostSendRequest {
            event: event.clone(),
            rendered_nudge: rendered_nudge.to_string(),
            // This legacy diagnostic command has no admitted message body.
            message_body: String::new(),
        };
        let response = deliver_graft_post_send(
            &record_path,
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

fn tmux_command() -> Command {
    #[cfg(test)]
    if let Some(program) = std::env::var_os(TMUX_PROGRAM_ENV).filter(|value| !value.is_empty()) {
        return Command::new(program);
    }
    Command::new("tmux")
}

fn run_tmux_command(mut command: Command, action: &'static str) -> Result<()> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command.spawn().map_err(|source| {
        AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed).with_cause(source)
    })?;
    let output = wait_for_tmux_output(child, action)?;
    ensure_tmux_success(output, action).map_err(Into::into)
}

fn wait_for_tmux_output(mut child: Child, _action: &'static str) -> Result<Output> {
    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().map_err(|source| {
                    AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed)
                        .with_cause(source)
                        .into()
                });
            }
            Ok(None) if started_at.elapsed() < TMUX_SEND_TIMEOUT => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed).into());
            }
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed)
                    .with_cause(source)
                    .into());
            }
        }
    }
}

fn ensure_tmux_success(output: Output, action: &'static str) -> Result<(), AtmError> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = if stderr.is_empty() {
        format!("tmux exited unsuccessfully while trying to {action}")
    } else {
        format!("tmux exited unsuccessfully while trying to {action}: {stderr}")
    };
    Err(AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed).with_cause(detail))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use atm_core::boundary::{
        BuiltInNudgeSinkTarget, BuiltInNudgeTemplateKind, InternalNudgeEnvelope, PostSendHookEvent,
        ResolvedBuiltInNudgeTemplate, TMUX_DOUBLE_ENTER_DELAY,
        built_in_nudge_template_kind_from_post_send_event,
    };
    use atm_core::send::default_template;
    use atm_core::test_support::{EnvGuard, TEST_ARCH_CTM, TEST_LEAD, TEST_TEAM};
    use serial_test::serial;
    use tempfile::tempdir;

    use super::{
        INTERNAL_NUDGE_ENV, InternalNudgeCommand, InternalNudgeInput, TMUX_PROGRAM_ENV,
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
            recipient_pane_id: Some(atm_core::types::PaneId::from_cli("%9").expect("pane")),
        }
    }

    #[test]
    fn built_in_templates_keep_ack_payloads_compact() {
        assert_eq!(
            default_template(BuiltInNudgeTemplateKind::Acknowledge),
            "<atm kind=\"ack\" from=\"{{from}}\" message-id=\"{{message_id}}\"/>"
        );
        assert_eq!(
            default_template(BuiltInNudgeTemplateKind::AcknowledgeTask),
            "<atm kind=\"ack\" from=\"{{from}}\" message-id=\"{{message_id}}\" task-id=\"{{task_id}}\"/>"
        );
    }

    #[test]
    fn built_in_template_kind_selection_covers_six_paths() {
        let mut event = base_event();
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event),
            BuiltInNudgeTemplateKind::Delivery
        );
        event.requires_ack = true;
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event),
            BuiltInNudgeTemplateKind::DeliveryAck
        );
        event.requires_ack = false;
        event.task_id = Some("AD.21".parse().expect("task"));
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event),
            BuiltInNudgeTemplateKind::DeliveryTask
        );
        event.requires_ack = true;
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event),
            BuiltInNudgeTemplateKind::DeliveryTaskAck
        );
        event.is_ack = true;
        event.requires_ack = false;
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event),
            BuiltInNudgeTemplateKind::AcknowledgeTask
        );
        event.task_id = None;
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&event),
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
                kind: BuiltInNudgeTemplateKind::DeliveryTaskAck,
                body: Some("<atm from=\"{{from}}\" message-id=\"{{message_id}}\"/>".to_string()),
            },
        })
        .expect("serialize envelope");
        let payload_value = payload.to_string();
        let _env = EnvGuard::set_many([(INTERNAL_NUDGE_ENV, Some(payload_value.as_str()))]);

        let input = InternalNudgeInput::from_env().expect("input");

        assert_eq!(input.sink_target, BuiltInNudgeSinkTarget::Tmux);
        assert_eq!(input.event.task_id.expect("task").as_str(), "AD.21");
        assert_eq!(
            input.template.kind,
            BuiltInNudgeTemplateKind::DeliveryTaskAck
        );
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
    #[serial(env)]
    fn tmux_sink_uses_double_enter_sequence() {
        let tempdir = tempdir().expect("tempdir");
        let tmux_log = tempdir.path().join("tmux.log");
        #[cfg(windows)]
        let tmux_path = tempdir.path().join("tmux.cmd");
        #[cfg(not(windows))]
        let tmux_path = tempdir.path().join("tmux");
        #[cfg(windows)]
        fs::write(
            &tmux_path,
            "@echo off\r\n>> \"%ATM_TEST_TMUX_LOG%\" echo %*\r\nexit /b 0\r\n",
        )
        .expect("write tmux shim");
        #[cfg(not(windows))]
        fs::write(
            &tmux_path,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$ATM_TEST_TMUX_LOG\"\nexit 0\n",
        )
        .expect("write tmux shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&tmux_path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&tmux_path, perms).expect("chmod");
        }

        let tmux_log_value = tmux_log.display().to_string();
        let tmux_bin_value = tmux_path.display().to_string();
        let _env = EnvGuard::set_many([
            (TMUX_PROGRAM_ENV, Some(tmux_bin_value.as_str())),
            ("ATM_TEST_TMUX_LOG", Some(tmux_log_value.as_str())),
        ]);
        super::TmuxNudgeSink
            .deliver(&base_event(), "<atm/>")
            .expect("deliver");
        let logged = fs::read_to_string(&tmux_log).expect("tmux log");
        assert_eq!(logged.matches("Enter").count(), 2);
        assert!(TMUX_DOUBLE_ENTER_DELAY >= Duration::from_millis(250));
    }
}
