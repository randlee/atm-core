use std::collections::BTreeMap;
use std::io::Write as _;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use atm_core::boundary::{
    BuiltInNudgeTemplateKind, PostSendHookEvent, TeamNudgeTemplateOverrideRow,
};
use atm_core::error::{AtmError, AtmErrorKind};
use atm_core::error_codes::AtmErrorCode;
use atm_core::graft::{
    GraftPostSendRequest, GraftPostSendResponse, graft_receiver_socket_path_from_home,
    read_graft_post_send_message, write_graft_post_send_message,
};
use atm_core::home;
use atm_daemon_bootstrap::with_default_nudge_template_override_store;
use clap::Args;
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;

use crate::observability::CliObservability;

const ATM_POST_SEND_ENV: &str = "ATM_POST_SEND";
const INTERNAL_NUDGE_SINK_ENV: &str = "ATM_INTERNAL_NUDGE_SINK";
const TMUX_DOUBLE_ENTER_DELAY: Duration = Duration::from_millis(275);
const TMUX_SEND_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const TMUX_PROGRAM_ENV: &str = "ATM_TEST_TMUX_BIN";

#[derive(Debug, Args)]
#[command(hide = true)]
pub struct InternalNudgeCommand;

impl InternalNudgeCommand {
    pub fn run(self, _observability: &CliObservability) -> Result<()> {
        let input = InternalNudgeInput::from_env()?;
        let kind = BuiltInNudgeTemplateKind::from_post_send_event(&input.event);
        let Some(template) = resolve_template_body(&input.event.recipient_team, kind)? else {
            return Ok(());
        };
        let rendered = render_template(&template, &input.render_values())?;
        match input.sink_target {
            NudgeSinkTarget::Tmux => TmuxNudgeSink.deliver(&input.event, &rendered)?,
            NudgeSinkTarget::Graft => GraftNudgeSink.deliver(&input.event)?,
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NudgeSinkTarget {
    Tmux,
    Graft,
}

impl NudgeSinkTarget {
    fn from_env() -> Result<Self> {
        match std::env::var(INTERNAL_NUDGE_SINK_ENV).as_deref() {
            Ok("tmux") => Ok(Self::Tmux),
            Ok("graft") => Ok(Self::Graft),
            Ok(other) => Err(AtmError::validation(format!(
                "unsupported built-in nudge sink target `{other}`"
            ))
            .with_recovery(
                "Set ATM_INTERNAL_NUDGE_SINK to `tmux` or `graft` before retrying the built-in post-send path.",
            )
            .into()),
            Err(_) => Err(AtmError::validation(
                "missing ATM_INTERNAL_NUDGE_SINK for built-in post-send nudge",
            )
            .with_recovery(
                "Populate ATM_INTERNAL_NUDGE_SINK before invoking `atm internal-nudge`.",
            )
            .into()),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct RawInternalNudgePayload {
    from: String,
    sender: String,
    recipient: String,
    team: String,
    message_id: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    message: String,
    requires_ack: bool,
    is_ack: bool,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    recipient_pane_id: Option<String>,
}

#[derive(Debug)]
struct InternalNudgeInput {
    from: String,
    event: PostSendHookEvent,
    sink_target: NudgeSinkTarget,
}

impl InternalNudgeInput {
    fn from_env() -> Result<Self> {
        let raw_payload = std::env::var(ATM_POST_SEND_ENV).map_err(|_| {
            AtmError::validation("missing ATM_POST_SEND payload for built-in post-send nudge")
                .with_recovery(
                    "Populate ATM_POST_SEND before invoking the built-in `atm internal-nudge` path.",
                )
        })?;
        let payload: RawInternalNudgePayload =
            serde_json::from_str(&raw_payload).map_err(|source| {
                AtmError::validation("failed to decode ATM_POST_SEND payload for built-in nudge")
                    .with_recovery(
                        "Repair the ATM_POST_SEND JSON payload before retrying the built-in post-send path.",
                    )
                    .with_source(source)
            })?;
        let description = if payload.description.trim().is_empty() {
            payload.message.clone()
        } else {
            payload.description.clone()
        };
        Ok(Self {
            from: payload.from,
            event: PostSendHookEvent {
                sender: payload.sender.parse()?,
                sender_team: payload.team.parse()?,
                recipient: payload.recipient.parse()?,
                recipient_team: payload.team.parse()?,
                message_id: payload.message_id.parse()?,
                description,
                requires_ack: payload.requires_ack,
                is_ack: payload.is_ack,
                task_id: payload
                    .task_id
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| value.parse())
                    .transpose()?,
                recipient_pane_id: payload
                    .recipient_pane_id
                    .filter(|value| !value.trim().is_empty())
                    .as_deref()
                    .map(atm_core::types::PaneId::from_cli)
                    .transpose()?,
            },
            sink_target: NudgeSinkTarget::from_env()?,
        })
    }

    fn render_values(&self) -> BTreeMap<&'static str, String> {
        BTreeMap::from([
            ("from", self.from.clone()),
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

fn load_override_body(
    team: &atm_core::types::TeamName,
    kind: BuiltInNudgeTemplateKind,
) -> Result<Option<String>> {
    with_default_nudge_template_override_store(|store| {
        Ok(store
            .load_template_override(team, kind)?
            .map(|row: TeamNudgeTemplateOverrideRow| row.template_body))
    })
    .map_err(Into::into)
}

fn resolve_template_body(
    team: &atm_core::types::TeamName,
    kind: BuiltInNudgeTemplateKind,
) -> Result<Option<String>> {
    match load_override_body(team, kind)? {
        Some(template) if template.is_empty() => Ok(None),
        Some(template) => Ok(Some(template)),
        None => Ok(Some(default_template(kind).to_string())),
    }
}

fn default_template(kind: BuiltInNudgeTemplateKind) -> &'static str {
    match kind {
        BuiltInNudgeTemplateKind::Delivery => {
            "<atm from=\"{{from}}\" message-id=\"{{message_id}}\">\n  <action>read atm --team {{team}}</action>\n  <description>{{description}}</description>\n  <action>execute the assigned task</action>\n  <when idle=\"immediate\" busy=\"after-current-task\"/>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        }
        BuiltInNudgeTemplateKind::DeliveryAck => {
            "<atm from=\"{{from}}\" message-id=\"{{message_id}}\">\n  <action>read atm --team {{team}}</action>\n  <action>ack the message</action>\n  <description>{{description}}</description>\n  <action>execute the assigned task</action>\n  <when idle=\"immediate\" busy=\"after-current-task\"/>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        }
        BuiltInNudgeTemplateKind::DeliveryTask => {
            "<atm from=\"{{from}}\" message-id=\"{{message_id}}\">\n  <action>read atm --team {{team}}</action>\n  <task id=\"{{task_id}}\">{{description}}</task>\n  <action>execute the assigned task</action>\n  <when idle=\"immediate\" busy=\"after-current-task\"/>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        }
        BuiltInNudgeTemplateKind::DeliveryTaskAck => {
            "<atm from=\"{{from}}\" message-id=\"{{message_id}}\">\n  <action>read atm --team {{team}}</action>\n  <action>ack the message</action>\n  <task id=\"{{task_id}}\">{{description}}</task>\n  <action>execute the assigned task</action>\n  <when idle=\"immediate\" busy=\"after-current-task\"/>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        }
        BuiltInNudgeTemplateKind::Acknowledge => {
            "<atm kind=\"ack\" from=\"{{from}}\" message-id=\"{{message_id}}\"/>"
        }
        BuiltInNudgeTemplateKind::AcknowledgeTask => {
            "<atm kind=\"ack\" from=\"{{from}}\" message-id=\"{{message_id}}\" task-id=\"{{task_id}}\"/>"
        }
    }
}

fn render_template(template: &str, values: &BTreeMap<&'static str, String>) -> Result<String> {
    if template.contains("{%") || template.contains("%}") {
        return Err(AtmError::validation(
            "built-in nudge templates do not support Jinja or conditional blocks",
        )
        .with_recovery(
            "Use only the documented placeholder tokens in the stored template body before retrying built-in nudge rendering.",
        )
        .into());
    }
    let mut output = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            return Err(AtmError::validation("unterminated built-in nudge placeholder")
                .with_recovery(
                    "Close every built-in nudge placeholder with `}}` before retrying template rendering.",
                )
                .into());
        };
        let key = after_start[..end].trim();
        let Some(value) = values.get(key) else {
            return Err(AtmError::validation(format!(
                "unsupported built-in nudge placeholder `{{{{{key}}}}}`"
            ))
            .with_recovery(
                "Use only {{from}}, {{team}}, {{message_id}}, {{description}}, and {{task_id}} in built-in nudge templates.",
            )
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
        let pane_id = event.recipient_pane_id.as_ref().ok_or_else(|| {
            AtmError::new_with_code(
                AtmErrorCode::PostSendPaneMissing,
                AtmErrorKind::Validation,
                format!(
                    "recipient {}@{} has tmux-backed post-send capability but no pane id",
                    event.recipient, event.recipient_team
                ),
            )
            .with_recovery(format!(
                "Repair the roster row with `atm teams update-member --team {} --member {} --pane-id <pane>`.",
                event.recipient_team, event.recipient
            ))
        })?;
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
                command.args(["send-keys", "-t", pane_id.as_str(), "Enter"]);
                command
            },
            "send first Enter to nudge pane",
        )?;
        thread::sleep(TMUX_DOUBLE_ENTER_DELAY);
        run_tmux_command(
            {
                let mut command = tmux_command();
                command.args(["send-keys", "-t", pane_id.as_str(), "Enter"]);
                command
            },
            "send second Enter to nudge pane",
        )?;
        Ok(())
    }
}

struct GraftNudgeSink;

impl GraftNudgeSink {
    fn deliver(&self, event: &PostSendHookEvent) -> Result<()> {
        let home_dir = home::atm_home()?;
        let endpoint_path = graft_receiver_socket_path_from_home(
            &home_dir,
            &event.recipient_team,
            &event.recipient,
        );
        let endpoint_name = atm_core::protocol::daemon_local_ipc_name_from_path(&endpoint_path)?;
        let mut stream = LocalSocketStream::connect(endpoint_name).map_err(|source| {
            AtmError::new_with_code(
                AtmErrorCode::PostSendGraftUnavailable,
                AtmErrorKind::DaemonUnavailable,
                format!(
                    "failed to connect to graft nudge receiver for {}@{}",
                    event.recipient, event.recipient_team
                ),
            )
            .with_recovery(
                "Start or repair the graft-backed receiver before retrying post-send delivery.",
            )
            .with_source(source)
        })?;
        let request = GraftPostSendRequest {
            event: event.clone(),
        };
        write_graft_post_send_message(
            &mut stream,
            &request,
            "failed to write graft post-send request",
            "graft post-send request exceeded the bounded payload cap",
        )?;
        let response: GraftPostSendResponse = read_graft_post_send_message(
            &mut stream,
            "failed to read graft post-send response",
            "graft post-send response exceeded the bounded payload cap",
        )?;
        stream.flush().map_err(|source| {
            AtmError::new_with_code(
                AtmErrorCode::PostSendGraftUnavailable,
                AtmErrorKind::DaemonUnavailable,
                "failed to flush graft post-send request",
            )
            .with_recovery(
                "Repair the graft-backed receiver socket before retrying post-send delivery.",
            )
            .with_source(source)
        })?;
        match response {
            GraftPostSendResponse::Delivered => Ok(()),
            GraftPostSendResponse::Error(error) => Err(error.into_atm_error().into()),
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
        AtmError::new_with_code(
            AtmErrorCode::PostSendTmuxSendFailed,
            AtmErrorKind::DaemonUnavailable,
            format!("failed to start tmux while trying to {action}: {source}"),
        )
        .with_recovery("Repair the local tmux installation before retrying post-send delivery.")
        .with_source(source)
    })?;
    let output = wait_for_tmux_output(child, action)?;
    ensure_tmux_success(output, action).map_err(Into::into)
}

fn wait_for_tmux_output(mut child: Child, action: &'static str) -> Result<Output> {
    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().map_err(|source| {
                    AtmError::new_with_code(
                        AtmErrorCode::PostSendTmuxSendFailed,
                        AtmErrorKind::DaemonUnavailable,
                        format!("failed to collect tmux output while trying to {action}: {source}"),
                    )
                    .with_recovery(
                        "Repair the local tmux installation before retrying post-send delivery.",
                    )
                    .with_source(source)
                    .into()
                });
            }
            Ok(None) if started_at.elapsed() < TMUX_SEND_TIMEOUT => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AtmError::new_with_code(
                    AtmErrorCode::PostSendTmuxSendFailed,
                    AtmErrorKind::Timeout,
                    format!(
                        "tmux {action} timed out after {}s",
                        TMUX_SEND_TIMEOUT.as_secs()
                    ),
                )
                .with_recovery(
                    "Repair the local tmux installation or pane state before retrying post-send delivery.",
                )
                .into());
            }
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AtmError::new_with_code(
                    AtmErrorCode::PostSendTmuxSendFailed,
                    AtmErrorKind::DaemonUnavailable,
                    format!("failed while waiting for tmux {action}: {source}"),
                )
                .with_recovery(
                    "Repair the local tmux installation before retrying post-send delivery.",
                )
                .with_source(source)
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
    Err(AtmError::new_with_code(
        AtmErrorCode::PostSendTmuxSendFailed,
        AtmErrorKind::DaemonUnavailable,
        detail,
    )
    .with_recovery("Repair the local tmux installation before retrying post-send delivery."))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use atm_core::boundary::{BuiltInNudgeTemplateKind, PostSendHookEvent};
    use atm_core::test_support::{EnvGuard, TEST_ARCH_CTM, TEST_LEAD, TEST_TEAM};
    use serial_test::serial;
    use tempfile::tempdir;

    use super::{
        ATM_POST_SEND_ENV, INTERNAL_NUDGE_SINK_ENV, InternalNudgeInput, NudgeSinkTarget,
        TMUX_DOUBLE_ENTER_DELAY, TMUX_PROGRAM_ENV, default_template, render_template,
        resolve_template_body,
    };

    fn base_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: TEST_LEAD.parse().expect("sender"),
            sender_team: TEST_TEAM.parse().expect("team"),
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
            BuiltInNudgeTemplateKind::from_post_send_event(&event),
            BuiltInNudgeTemplateKind::Delivery
        );
        event.requires_ack = true;
        assert_eq!(
            BuiltInNudgeTemplateKind::from_post_send_event(&event),
            BuiltInNudgeTemplateKind::DeliveryAck
        );
        event.requires_ack = false;
        event.task_id = Some("AD.21".parse().expect("task"));
        assert_eq!(
            BuiltInNudgeTemplateKind::from_post_send_event(&event),
            BuiltInNudgeTemplateKind::DeliveryTask
        );
        event.requires_ack = true;
        assert_eq!(
            BuiltInNudgeTemplateKind::from_post_send_event(&event),
            BuiltInNudgeTemplateKind::DeliveryTaskAck
        );
        event.is_ack = true;
        event.requires_ack = false;
        assert_eq!(
            BuiltInNudgeTemplateKind::from_post_send_event(&event),
            BuiltInNudgeTemplateKind::AcknowledgeTask
        );
        event.task_id = None;
        assert_eq!(
            BuiltInNudgeTemplateKind::from_post_send_event(&event),
            BuiltInNudgeTemplateKind::Acknowledge
        );
    }

    #[test]
    fn render_template_replaces_only_supported_placeholders() {
        let rendered = render_template(
            "<atm from=\"{{from}}\" task-id=\"{{task_id}}\">{{description}}</atm>",
            &InternalNudgeInput {
                from: format!("{TEST_LEAD}@{TEST_TEAM}"),
                event: base_event(),
                sink_target: NudgeSinkTarget::Tmux,
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
                from: format!("{TEST_LEAD}@{TEST_TEAM}"),
                event: base_event(),
                sink_target: NudgeSinkTarget::Tmux,
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
    fn internal_nudge_input_reads_post_send_env() {
        let payload = serde_json::json!({
            "from": format!("{TEST_LEAD}@{TEST_TEAM}"),
            "sender": TEST_LEAD,
            "recipient": TEST_ARCH_CTM,
            "team": TEST_TEAM,
            "message_id": "01KX1TEST00000000000000000",
            "description": "review failing smoke lane",
            "message": "review failing smoke lane",
            "requires_ack": true,
            "is_ack": false,
            "task_id": "AD.21",
            "recipient_pane_id": "%9"
        });
        let payload_value = payload.to_string();
        let _env = EnvGuard::set_many([
            (ATM_POST_SEND_ENV, Some(payload_value.as_str())),
            (INTERNAL_NUDGE_SINK_ENV, Some("tmux")),
        ]);

        let input = InternalNudgeInput::from_env().expect("input");

        assert_eq!(input.from, format!("{TEST_LEAD}@{TEST_TEAM}"));
        assert_eq!(input.sink_target, NudgeSinkTarget::Tmux);
        assert_eq!(input.event.task_id.expect("task").as_str(), "AD.21");
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

    #[test]
    #[serial(env)]
    fn empty_override_body_skips_built_in_nudge_delivery() {
        let tempdir = tempdir().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        fs::create_dir_all(&home_dir).expect("home");
        let team = TEST_TEAM.parse().expect("team");
        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(home_dir.to_str().expect("utf8"))),
            ("HOME", Some(home_dir.to_str().expect("utf8"))),
        ]);

        atm_daemon_bootstrap::with_default_nudge_template_override_store(|override_store| {
            override_store.save_template_override(&team, BuiltInNudgeTemplateKind::Delivery, "")?;
            Ok(())
        })
        .expect("save override");

        let template =
            resolve_template_body(&team, BuiltInNudgeTemplateKind::Delivery).expect("resolve");
        assert!(template.is_none());
    }

    #[test]
    #[serial(env)]
    fn override_row_only_applies_to_selected_template_kind() {
        let tempdir = tempdir().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        fs::create_dir_all(&home_dir).expect("home");
        let team = TEST_TEAM.parse().expect("team");
        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(home_dir.to_str().expect("utf8"))),
            ("HOME", Some(home_dir.to_str().expect("utf8"))),
        ]);

        atm_daemon_bootstrap::with_default_nudge_template_override_store(|override_store| {
            override_store.save_template_override(
                &team,
                BuiltInNudgeTemplateKind::DeliveryAck,
                "<atm kind=\"override\"/>",
            )?;
            Ok(())
        })
        .expect("save override");

        let overridden = resolve_template_body(&team, BuiltInNudgeTemplateKind::DeliveryAck)
            .expect("resolve ack override");
        let fallback = resolve_template_body(&team, BuiltInNudgeTemplateKind::Delivery)
            .expect("resolve delivery fallback");

        assert_eq!(overridden.as_deref(), Some("<atm kind=\"override\"/>"));
        assert_eq!(
            fallback.as_deref(),
            Some(default_template(BuiltInNudgeTemplateKind::Delivery))
        );
    }
}
