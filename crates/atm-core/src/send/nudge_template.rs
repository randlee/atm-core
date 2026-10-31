use std::collections::BTreeMap;

use crate::boundary::{
    BuiltInNudgeTemplateKind, PostSendHookEvent, ResolvedBuiltInNudgeTemplate, TaskClosedOutcome,
    TaskTransition, TeamNudgeTemplateOverrideRow,
};
use crate::boundary::{CronLifecycleEvent, CronName};
use crate::error::AtmError;
use crate::types::TaskId;

pub fn resolve_template(
    override_row: Option<TeamNudgeTemplateOverrideRow>,
    kind: BuiltInNudgeTemplateKind,
) -> ResolvedBuiltInNudgeTemplate {
    let body = match override_row {
        Some(row) => row.template_body().map(ToOwned::to_owned),
        None => Some(default_template(kind).to_string()),
    };
    ResolvedBuiltInNudgeTemplate { kind, body }
}

pub fn qualified_sender_identity(event: &PostSendHookEvent) -> String {
    event.source_address().to_string()
}

pub fn render_resolved_built_in_nudge(
    event: &PostSendHookEvent,
    template: &ResolvedBuiltInNudgeTemplate,
) -> Result<Option<String>, AtmError> {
    let Some(template_body) = template.body.as_deref() else {
        return Ok(None);
    };
    render_template(template_body, &render_values(event, &[])).map(Some)
}

/// Render one cron lifecycle nudge from the cron-only template family.
///
/// Cron lifecycle notices deliberately do not reuse post-send events: their
/// values come from committed cron intent, and the only user-configurable
/// variables are `name`, `event`, and a trusted `detail_attribute` fragment.
pub fn render_cron_lifecycle_nudge(
    template: &ResolvedBuiltInNudgeTemplate,
    name: &CronName,
    event: &CronLifecycleEvent,
) -> Result<Option<String>, AtmError> {
    if template.kind != BuiltInNudgeTemplateKind::Cron {
        return Err(AtmError::validation(
            "cron lifecycle rendering requires the cron template kind",
        ));
    }
    let Some(template_body) = template.body.as_deref() else {
        return Ok(None);
    };
    let (event, detail_attribute) = match event {
        CronLifecycleEvent::Stopped { message } => (
            "stopped",
            format!("message=\"{}\"", escape_xml_attribute(message.as_str())),
        ),
        CronLifecycleEvent::Error { failure } => (
            "error",
            format!(
                "error=\"{}\"",
                escape_xml_attribute(failure.detail.as_str())
            ),
        ),
    };
    render_template(
        template_body,
        &BTreeMap::from([
            ("name", escape_xml_attribute(name.as_str())),
            ("event", event.to_owned()),
            ("detail_attribute", detail_attribute),
        ]),
    )
    .map(Some)
}

/// Renders a task nudge that lists every open task of the recipient, so one
/// nudge covers all of a member's tasks rather than one nudge per task.
pub(crate) fn render_built_in_nudge(
    event: &PostSendHookEvent,
    template_body: &str,
    open_tasks: &[TaskId],
) -> Result<String, AtmError> {
    render_template(template_body, &render_values(event, open_tasks))
}

pub(crate) fn validate_built_in_nudge_template_body(template_body: &str) -> Result<(), AtmError> {
    render_template(
        template_body,
        &BTreeMap::from([
            ("from", String::new()),
            ("team", String::new()),
            ("message_id", String::new()),
            ("description", String::new()),
            ("task_id", String::new()),
            ("open_tasks", String::new()),
            ("position", String::new()),
            ("attempt", String::new()),
            ("assignee", String::new()),
            ("outcome", String::new()),
            ("by", String::new()),
        ]),
    )
    .map(|_| ())
}

/// Validate a cron-only override at write time, before it reaches storage.
pub(crate) fn validate_cron_nudge_template_body(template_body: &str) -> Result<(), AtmError> {
    render_template(
        template_body,
        &BTreeMap::from([
            ("name", String::new()),
            ("event", String::new()),
            ("detail_attribute", String::new()),
        ]),
    )
    .map(|_| ())
}

fn escape_xml_attribute(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            character if character.is_control() => escaped.push(' '),
            character => escaped.push(character),
        }
    }
    escaped
}

fn render_values(
    event: &PostSendHookEvent,
    open_tasks: &[TaskId],
) -> BTreeMap<&'static str, String> {
    let (position, attempt, outcome) = match event.task_transition {
        Some(TaskTransition::Queued { position }) => {
            (position.to_string(), String::new(), String::new())
        }
        Some(TaskTransition::Reminder { attempt }) => {
            (String::new(), attempt.to_string(), String::new())
        }
        Some(TaskTransition::Complete { outcome }) => {
            (String::new(), String::new(), outcome.as_str().to_owned())
        }
        Some(TaskTransition::Closed { outcome }) => (
            String::new(),
            String::new(),
            match outcome {
                TaskClosedOutcome::Cancelled => "cancelled",
                TaskClosedOutcome::Reassigned => "reassigned",
            }
            .to_owned(),
        ),
        _ => (String::new(), String::new(), String::new()),
    };
    // Peer ingress supplies sender_host only for an authenticated cross-host
    // sender; local sends leave it absent.
    let sender = if event.sender_team == event.recipient_team && event.sender_host.is_none() {
        event.sender.to_string()
    } else {
        qualified_sender_identity(event)
    };
    BTreeMap::from([
        ("from", sender.clone()),
        ("team", event.recipient_team.to_string()),
        ("message_id", event.message_id.to_string()),
        ("description", event.description.clone()),
        (
            "task_id",
            event
                .task_id
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        ),
        (
            "open_tasks",
            open_tasks
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        ("position", position),
        ("attempt", attempt),
        ("assignee", sender.clone()),
        ("outcome", outcome),
        ("by", sender),
    ])
}

pub fn default_template(kind: BuiltInNudgeTemplateKind) -> &'static str {
    match kind {
        BuiltInNudgeTemplateKind::Delivery => {
            "<atm from=\"{{from}}\" message-id=\"{{message_id}}\">\n  <action>atm read --message-id {{message_id}}</action>\n  <description>{{description}}</description>\n  <action>execute the assigned task</action>\n  <when idle=\"immediate\" busy=\"after-current-task\"/>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        }
        BuiltInNudgeTemplateKind::DeliveryAck => {
            "<atm from=\"{{from}}\" message-id=\"{{message_id}}\">\n  <action>atm read --message-id {{message_id}}</action>\n  <action>ack the message</action>\n  <description>{{description}}</description>\n  <action>execute the assigned task</action>\n  <when idle=\"immediate\" busy=\"after-current-task\"/>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        }
        BuiltInNudgeTemplateKind::Queue => {
            "<atm from=\"{{from}}\" message-id=\"{{message_id}}\">\n  <action>atm read --message-id {{message_id}}</action>\n  <description>{{description}}</description>\n  <action>execute the assigned task</action>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        }
        BuiltInNudgeTemplateKind::QueueAck => {
            "<atm from=\"{{from}}\" message-id=\"{{message_id}}\">\n  <action>atm read --message-id {{message_id}}</action>\n  <action>ack the message</action>\n  <description>{{description}}</description>\n  <action>execute the assigned task</action>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        }
        BuiltInNudgeTemplateKind::Acknowledge => {
            "<atm kind=\"ack\" from=\"{{from}}\" message-id=\"{{message_id}}\"/>"
        }
        BuiltInNudgeTemplateKind::TaskQueued => {
            "<atm task=\"{{task_id}}\" queued=\"{{position}}\" message=\"{{message_id}}\" from=\"{{from}}\"/>"
        }
        BuiltInNudgeTemplateKind::TaskReady => {
            "<atm task=\"{{task_id}}\" open=\"{{open_tasks}}\" ready message=\"{{message_id}}\" from=\"{{from}}\">\n  <action>atm read --message-id {{message_id}}</action>\n  <action>atm task start {{task_id}}</action>\n  <action>execute the assigned task</action>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        }
        BuiltInNudgeTemplateKind::TaskReminder => {
            "<atm task=\"{{task_id}}\" open=\"{{open_tasks}}\" reminder=\"{{attempt}}\" message=\"{{message_id}}\" from=\"{{from}}\">\n  <action>atm read --message-id {{message_id}}</action>\n  <action>atm task start {{task_id}}</action>\n  <action>execute the assigned task</action>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        }
        BuiltInNudgeTemplateKind::TaskStarted => {
            "<atm task=\"{{task_id}}\" started agent=\"{{assignee}}\" message=\"{{message_id}}\"/>"
        }
        BuiltInNudgeTemplateKind::TaskComplete => {
            "<atm task=\"{{task_id}}\" complete agent=\"{{assignee}}\" outcome=\"{{outcome}}\" message=\"{{message_id}}\"/>"
        }
        BuiltInNudgeTemplateKind::TaskClosed => {
            "<atm task=\"{{task_id}}\" closed by=\"{{from}}\" outcome=\"{{outcome}}\" message=\"{{message_id}}\"/>"
        }
        BuiltInNudgeTemplateKind::Cron => {
            "<atm cron name=\"{{name}}\" event=\"{{event}}\" {{detail_attribute}}>"
        }
    }
}

fn render_template(
    template: &str,
    values: &BTreeMap<&'static str, String>,
) -> Result<String, AtmError> {
    if template.contains("{%") || template.contains("%}") {
        return Err(AtmError::validation(
            "built-in nudge templates do not support Jinja or conditional blocks",
        ));
    }

    let mut output = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            return Err(AtmError::validation(
                "unterminated built-in nudge placeholder",
            ));
        };
        let key = after_start[..end].trim();
        let Some(value) = values.get(key) else {
            return Err(AtmError::validation(format!(
                "unsupported built-in nudge placeholder `{{{{{key}}}}}`"
            )));
        };
        output.push_str(value);
        rest = &after_start[end + 2..];
    }
    output.push_str(rest);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{
        default_template, qualified_sender_identity, render_built_in_nudge,
        render_cron_lifecycle_nudge, resolve_template, validate_built_in_nudge_template_body,
        validate_cron_nudge_template_body,
    };
    use crate::boundary::CronDiagnosticDetail;
    use crate::boundary::{
        BuiltInNudgeTemplateKind, PostSendHookEvent, ResolvedBuiltInNudgeTemplate,
        TaskCloseOutcome, TaskClosedOutcome, TaskTransition, TeamNudgeTemplateOverrideMode,
        TeamNudgeTemplateOverrideRow,
    };
    use crate::boundary::{CronLifecycleEvent, CronName};
    use crate::test_support::{TEST_ARCH_CTM, TEST_LEAD, TEST_TEAM};
    use crate::types::TaskId;
    use crate::types::{AgentName, ChatId, IsoTimestamp, PaneId, TeamName};

    fn base_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: AgentName::from_validated(TEST_LEAD),
            sender_chat_id: None,
            sender_team: TeamName::from_validated(TEST_TEAM),
            sender_host: None,
            recipient: AgentName::from_validated(TEST_ARCH_CTM),
            recipient_team: TeamName::from_validated(TEST_TEAM),
            message_id: "01KX1TEST00000000000000000".parse().expect("message id"),
            description: "review failing smoke lane".to_string(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            task_transition: None,
            recipient_pane_id: Some(PaneId::from_cli("%9").expect("pane")),
        }
    }

    #[test]
    fn resolve_template_uses_explicit_override_body() {
        let row = TeamNudgeTemplateOverrideRow {
            team_name: TeamName::from_validated(TEST_TEAM),
            kind: BuiltInNudgeTemplateKind::DeliveryAck,
            mode: TeamNudgeTemplateOverrideMode::Override {
                template_body: "<atm kind=\"override\"/>".to_string(),
            },
            updated_at: IsoTimestamp::now(),
        };
        assert_eq!(
            resolve_template(Some(row), BuiltInNudgeTemplateKind::DeliveryAck),
            ResolvedBuiltInNudgeTemplate {
                kind: BuiltInNudgeTemplateKind::DeliveryAck,
                body: Some("<atm kind=\"override\"/>".to_string()),
            }
        );
    }

    #[test]
    fn qualified_sender_identity_uses_sender_and_team() {
        assert_eq!(
            qualified_sender_identity(&base_event()),
            format!("{TEST_LEAD}@{TEST_TEAM}")
        );
    }

    #[test]
    fn qualified_sender_identity_includes_authenticated_sender_host() {
        let mut event = base_event();
        event.sender_host = Some("rand-m4.local".parse().expect("sender host"));

        assert_eq!(
            qualified_sender_identity(&event),
            format!("{TEST_LEAD}@{TEST_TEAM}.rand-m4.local")
        );
        assert!(
            render_built_in_nudge(
                &event,
                default_template(BuiltInNudgeTemplateKind::Delivery),
                &[]
            )
            .expect("render nudge")
            .contains(&format!("from=\"{TEST_LEAD}@{TEST_TEAM}.rand-m4.local\""))
        );
    }

    #[test]
    fn qualified_sender_identity_preserves_chat_id() {
        let mut event = base_event();
        event.sender_chat_id = Some("chat-42".parse::<ChatId>().expect("chat id"));

        assert_eq!(
            qualified_sender_identity(&event),
            format!("{TEST_LEAD}:chat-42@{TEST_TEAM}")
        );
    }

    #[test]
    fn render_built_in_nudge_populates_placeholders() {
        let mut event = base_event();
        event.task_id = Some("task-9".parse().expect("task id"));
        event.task_transition = Some(TaskTransition::Ready);
        let rendered = render_built_in_nudge(
            &event,
            default_template(BuiltInNudgeTemplateKind::TaskReady),
            &[],
        )
        .expect("rendered template");
        assert!(rendered.contains(&format!("from=\"{TEST_LEAD}\"")));
        assert!(rendered.contains("01KX1TEST00000000000000000"));
    }

    #[test]
    fn task_notice_sender_identity_is_relative_to_recipient() {
        let cases = [
            (TEST_TEAM, None, TEST_LEAD.to_owned()),
            ("other-team", None, format!("{TEST_LEAD}@other-team")),
            (
                TEST_TEAM,
                Some("peer.example"),
                format!("{TEST_LEAD}@{TEST_TEAM}.peer.example"),
            ),
            (
                "other-team",
                Some("peer.example"),
                format!("{TEST_LEAD}@other-team.peer.example"),
            ),
        ];
        for (sender_team, host, expected) in cases {
            let mut event = base_event();
            event.sender_team = TeamName::from_validated(sender_team);
            event.sender_host = host.map(|value| value.parse().expect("host"));
            event.task_id = Some("task-9".parse().expect("task"));
            for (kind, transition) in [
                (
                    BuiltInNudgeTemplateKind::TaskStarted,
                    TaskTransition::Started,
                ),
                (
                    BuiltInNudgeTemplateKind::TaskComplete,
                    TaskTransition::Complete {
                        outcome: TaskCloseOutcome::Completed,
                    },
                ),
            ] {
                event.task_transition = Some(transition);
                let rendered =
                    render_built_in_nudge(&event, default_template(kind), &[]).expect("render");
                assert!(
                    rendered.contains(&format!("agent=\"{expected}\"")),
                    "{kind:?}: {rendered}"
                );
            }
            let rendered = render_built_in_nudge(&event, "from=\"{{from}}\" by=\"{{by}}\"", &[])
                .expect("render");
            assert_eq!(rendered, format!("from=\"{expected}\" by=\"{expected}\""));
            let closed = render_built_in_nudge(
                &event,
                default_template(BuiltInNudgeTemplateKind::TaskClosed),
                &[],
            )
            .expect("render");
            assert!(closed.contains(&format!("by=\"{expected}\"")), "{closed}");
        }
    }

    #[test]
    fn notice_sender_chat_is_retained_in_qualified_addresses() {
        let mut event = base_event();
        event.sender_chat_id = Some("chat-42".parse().expect("chat"));
        let body = "{{from}}|{{assignee}}|{{by}}";
        assert_eq!(
            render_built_in_nudge(&event, body, &[]).expect("local"),
            format!("{TEST_LEAD}|{TEST_LEAD}|{TEST_LEAD}")
        );
        event.sender_team = TeamName::from_validated("other-team");
        assert_eq!(
            render_built_in_nudge(&event, body, &[]).expect("cross-team"),
            format!(
                "{TEST_LEAD}:chat-42@other-team|{TEST_LEAD}:chat-42@other-team|{TEST_LEAD}:chat-42@other-team"
            )
        );
    }

    #[test]
    fn task_reminder_body_lists_open_tasks() {
        let mut event = base_event();
        event.task_id = Some("task-1".parse().expect("task id"));
        event.task_transition = Some(TaskTransition::Reminder { attempt: 1 });
        let open: Vec<TaskId> = ["task-1", "task-2", "task-3"]
            .iter()
            .map(|id| id.parse().expect("task id"))
            .collect();
        for kind in [
            BuiltInNudgeTemplateKind::TaskReminder,
            BuiltInNudgeTemplateKind::TaskReady,
        ] {
            let rendered = render_built_in_nudge(&event, default_template(kind), &open)
                .expect("default task body renders");
            assert!(
                rendered.contains("open=\"task-1 task-2 task-3\""),
                "{kind:?} body must list every open task: {rendered}"
            );
        }
    }

    #[test]
    fn open_tasks_placeholder_is_valid_in_team_override() {
        let override_body = "<atm open=\"{{open_tasks}}\"/>";
        validate_built_in_nudge_template_body(override_body).expect("override validates");
        let mut event = base_event();
        event.task_transition = Some(TaskTransition::Reminder { attempt: 1 });
        let open: Vec<TaskId> = vec!["task-1".parse().expect("task id")];
        let rendered =
            render_built_in_nudge(&event, override_body, &open).expect("override renders");
        assert_eq!(rendered, "<atm open=\"task-1\"/>");
    }

    #[test]
    fn every_default_body_renders_with_its_placeholders() {
        let cases = [
            (BuiltInNudgeTemplateKind::Delivery, None, true, true),
            (BuiltInNudgeTemplateKind::DeliveryAck, None, true, true),
            (BuiltInNudgeTemplateKind::Queue, None, false, true),
            (BuiltInNudgeTemplateKind::QueueAck, None, false, true),
            (BuiltInNudgeTemplateKind::Acknowledge, None, false, false),
            (
                BuiltInNudgeTemplateKind::TaskQueued,
                Some(TaskTransition::Queued { position: 2 }),
                false,
                false,
            ),
            (
                BuiltInNudgeTemplateKind::TaskReady,
                Some(TaskTransition::Ready),
                false,
                true,
            ),
            (
                BuiltInNudgeTemplateKind::TaskReminder,
                Some(TaskTransition::Reminder { attempt: 3 }),
                false,
                true,
            ),
            (
                BuiltInNudgeTemplateKind::TaskStarted,
                Some(TaskTransition::Started),
                false,
                false,
            ),
            (
                BuiltInNudgeTemplateKind::TaskComplete,
                Some(TaskTransition::Complete {
                    outcome: TaskCloseOutcome::Completed,
                }),
                false,
                false,
            ),
            (
                BuiltInNudgeTemplateKind::TaskClosed,
                Some(TaskTransition::Closed {
                    outcome: TaskClosedOutcome::Cancelled,
                }),
                false,
                false,
            ),
        ];
        for (kind, transition, has_idle_clause, has_read_action) in cases {
            let mut event = base_event();
            event.task_id = Some("task-9".parse().expect("task id"));
            event.task_transition = transition;
            let body = default_template(kind);
            assert!(!body.contains("read atm "), "legacy read action in {kind}");
            assert_eq!(
                body.contains("<when idle=\"immediate\""),
                has_idle_clause,
                "idle delivery contract for {kind}"
            );
            assert_eq!(
                body.contains("<action>atm read --message-id {{message_id}}</action>"),
                has_read_action,
                "read delivery contract for {kind}"
            );
            render_built_in_nudge(&event, body, &[]).expect("default template renders");
        }
    }

    #[test]
    fn cron_template_is_isolated_escaped_and_disableable() {
        let name = CronName::parse("nightly\"<&").expect("name");
        let event = CronLifecycleEvent::Stopped {
            message: CronDiagnosticDetail::parse("done <&\"'\nnext".to_owned()).expect("detail"),
        };
        let template = ResolvedBuiltInNudgeTemplate {
            kind: BuiltInNudgeTemplateKind::Cron,
            body: Some("<cron n=\"{{name}}\" e=\"{{event}}\" {{detail_attribute}}/>".to_owned()),
        };
        let rendered = render_cron_lifecycle_nudge(&template, &name, &event)
            .expect("render")
            .expect("enabled");
        assert_eq!(
            rendered,
            "<cron n=\"nightly&quot;&lt;&amp;\" e=\"stopped\" message=\"done &lt;&amp;&quot;&apos; next\"/>"
        );
        assert!(
            render_cron_lifecycle_nudge(
                &ResolvedBuiltInNudgeTemplate {
                    kind: BuiltInNudgeTemplateKind::Cron,
                    body: None
                },
                &name,
                &event,
            )
            .expect("disabled")
            .is_none()
        );
        validate_cron_nudge_template_body("<cron {{name}} {{event}} {{detail_attribute}}/>")
            .expect("cron-only placeholders");
        assert!(validate_cron_nudge_template_body("{{from}}").is_err());
    }

    #[test]
    fn informational_kinds_carry_no_action_element() {
        for kind in [
            BuiltInNudgeTemplateKind::TaskQueued,
            BuiltInNudgeTemplateKind::TaskStarted,
            BuiltInNudgeTemplateKind::TaskComplete,
            BuiltInNudgeTemplateKind::TaskClosed,
        ] {
            assert!(!default_template(kind).contains("<action>"));
        }
    }

    #[test]
    fn ready_and_reminder_bodies_name_atm_task_start() {
        for kind in [
            BuiltInNudgeTemplateKind::TaskReady,
            BuiltInNudgeTemplateKind::TaskReminder,
        ] {
            assert!(default_template(kind).contains("<action>atm task start {{task_id}}</action>"));
        }
    }

    #[test]
    fn unknown_placeholder_is_a_validation_error() {
        let error = render_built_in_nudge(&base_event(), "{{unknown}}", &[])
            .expect_err("unknown placeholder");
        assert!(
            error
                .message()
                .contains("unsupported built-in nudge placeholder")
        );
    }
}
