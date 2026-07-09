use std::collections::BTreeMap;

use crate::boundary::{BuiltInNudgeTemplateKind, PostSendHookEvent, TeamNudgeTemplateOverrideRow};
use crate::error::AtmError;

pub fn resolve_template_body(
    override_row: Option<TeamNudgeTemplateOverrideRow>,
    kind: BuiltInNudgeTemplateKind,
) -> Option<String> {
    match override_row {
        Some(row) => row.template_body().map(ToOwned::to_owned),
        None => Some(default_template(kind).to_string()),
    }
}

pub fn render_built_in_nudge(
    event: &PostSendHookEvent,
    from: &str,
    template_body: &str,
) -> Result<String, AtmError> {
    render_template(template_body, &render_values(event, from))
}

pub fn default_template(kind: BuiltInNudgeTemplateKind) -> &'static str {
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

fn render_values(event: &PostSendHookEvent, from: &str) -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("from", from.to_string()),
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
    ])
}

fn render_template(
    template: &str,
    values: &BTreeMap<&'static str, String>,
) -> Result<String, AtmError> {
    if template.contains("{%") || template.contains("%}") {
        return Err(AtmError::validation(
            "built-in nudge templates do not support Jinja or conditional blocks",
        )
        .with_recovery(
            "Use only the documented placeholder tokens in the stored template body before retrying built-in nudge rendering.",
        ));
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
                ));
        };
        let key = after_start[..end].trim();
        let Some(value) = values.get(key) else {
            return Err(AtmError::validation(format!(
                "unsupported built-in nudge placeholder `{{{{{key}}}}}`"
            ))
            .with_recovery(
                "Use only {{from}}, {{team}}, {{message_id}}, {{description}}, and {{task_id}} in built-in nudge templates.",
            ));
        };
        output.push_str(value);
        rest = &after_start[end + 2..];
    }
    output.push_str(rest);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{default_template, render_built_in_nudge, resolve_template_body};
    use crate::boundary::{
        BuiltInNudgeTemplateKind, PostSendHookEvent, TeamNudgeTemplateOverrideMode,
        TeamNudgeTemplateOverrideRow,
    };
    use crate::test_support::{TEST_ARCH_CTM, TEST_LEAD, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, PaneId, TeamName};

    fn base_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: AgentName::from_validated(TEST_LEAD),
            sender_team: TeamName::from_validated(TEST_TEAM),
            recipient: AgentName::from_validated(TEST_ARCH_CTM),
            recipient_team: TeamName::from_validated(TEST_TEAM),
            message_id: "01KX1TEST00000000000000000".parse().expect("message id"),
            description: "review failing smoke lane".to_string(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            recipient_pane_id: Some(PaneId::from_cli("%9").expect("pane")),
        }
    }

    #[test]
    fn resolve_template_body_uses_default_when_no_override_exists() {
        assert_eq!(
            resolve_template_body(None, BuiltInNudgeTemplateKind::Delivery),
            Some(default_template(BuiltInNudgeTemplateKind::Delivery).to_string())
        );
    }

    #[test]
    fn resolve_template_body_treats_disabled_override_as_disabled() {
        let row = TeamNudgeTemplateOverrideRow {
            team_name: TeamName::from_validated(TEST_TEAM),
            kind: BuiltInNudgeTemplateKind::Delivery,
            mode: TeamNudgeTemplateOverrideMode::Disabled,
            updated_at: IsoTimestamp::now(),
        };
        assert_eq!(
            resolve_template_body(Some(row), BuiltInNudgeTemplateKind::Delivery),
            None
        );
    }

    #[test]
    fn render_built_in_nudge_populates_placeholders() {
        let rendered = render_built_in_nudge(
            &base_event(),
            &format!("{TEST_LEAD}@{TEST_TEAM}"),
            default_template(BuiltInNudgeTemplateKind::DeliveryTaskAck),
        )
        .expect("rendered template");
        assert!(rendered.contains(&format!("{TEST_LEAD}@{TEST_TEAM}")));
        assert!(rendered.contains("01KX1TEST00000000000000000"));
    }
}
