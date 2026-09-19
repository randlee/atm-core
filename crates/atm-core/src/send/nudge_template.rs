use std::collections::BTreeMap;

use crate::boundary::{
    BuiltInNudgeTemplateKind, PostSendHookEvent, ResolvedBuiltInNudgeTemplate, TaskClosedOutcome,
    TaskTransition, TeamNudgeTemplateOverrideRow,
};
use crate::error::AtmError;

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
    render_template(template_body, &render_values(event)).map(Some)
}

pub fn render_template_body(
    event: &PostSendHookEvent,
    template_body: &str,
) -> Result<String, AtmError> {
    render_template(template_body, &render_values(event))
}

pub fn render_built_in_nudge(
    event: &PostSendHookEvent,
    template_body: &str,
) -> Result<String, AtmError> {
    render_template_body(event, template_body)
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
            ("position", String::new()),
            ("attempt", String::new()),
            ("assignee", String::new()),
            ("outcome", String::new()),
            ("by", String::new()),
        ]),
    )
    .map(|_| ())
}

fn render_values(event: &PostSendHookEvent) -> BTreeMap<&'static str, String> {
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
    let assignee = if matches!(
        event.task_transition,
        Some(TaskTransition::Started | TaskTransition::Complete { .. })
    ) {
        event.recipient.to_string()
    } else {
        event.sender.to_string()
    };
    BTreeMap::from([
        ("from", qualified_sender_identity(event)),
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
        ("position", position),
        ("attempt", attempt),
        ("assignee", assignee),
        ("outcome", outcome),
        ("by", event.sender.to_string()),
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
            "<atm task=\"{{task_id}}\" ready message=\"{{message_id}}\" from=\"{{from}}\">\n  <action>atm read --message-id {{message_id}}</action>\n  <action>atm task start {{task_id}}</action>\n  <action>execute the assigned task</action>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        }
        BuiltInNudgeTemplateKind::TaskReminder => {
            "<atm task=\"{{task_id}}\" reminder=\"{{attempt}}\" message=\"{{message_id}}\" from=\"{{from}}\">\n  <action>atm read --message-id {{message_id}}</action>\n  <action>atm task start {{task_id}}</action>\n  <action>execute the assigned task</action>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
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
        default_template, qualified_sender_identity, render_built_in_nudge, resolve_template,
    };
    use crate::boundary::{
        BuiltInNudgeTemplateKind, PostSendHookEvent, ResolvedBuiltInNudgeTemplate,
        TaskCloseOutcome, TaskClosedOutcome, TaskTransition, TeamNudgeTemplateOverrideMode,
        TeamNudgeTemplateOverrideRow,
    };
    use crate::test_support::{TEST_ARCH_CTM, TEST_LEAD, TEST_TEAM};
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
            render_built_in_nudge(&event, default_template(BuiltInNudgeTemplateKind::Delivery))
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
        )
        .expect("rendered template");
        assert!(rendered.contains(&format!("{TEST_LEAD}@{TEST_TEAM}")));
        assert!(rendered.contains("01KX1TEST00000000000000000"));
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
            render_built_in_nudge(&event, body).expect("default template renders");
        }
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
        let error =
            render_built_in_nudge(&base_event(), "{{unknown}}").expect_err("unknown placeholder");
        assert!(
            error
                .message()
                .contains("unsupported built-in nudge placeholder")
        );
    }
}
