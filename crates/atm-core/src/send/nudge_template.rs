use std::collections::BTreeMap;

use crate::boundary::{
    BuiltInNudgeTemplateKind, PostSendHookEvent, ResolvedBuiltInNudgeTemplate,
    TeamNudgeTemplateOverrideRow,
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
    let source = event.source_address().to_string();
    let Some(host) = event.authenticated_source_host.as_ref() else {
        return source;
    };

    format!("{source}.{}", display_host(host.as_str()))
}

/// Keeps canonical peer hostnames in storage/API while making `.local` mDNS
/// names compact in the human nudge address.
fn display_host(host: &str) -> &str {
    host.strip_suffix(".local").unwrap_or(host)
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

fn render_values(event: &PostSendHookEvent) -> BTreeMap<&'static str, String> {
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
    ])
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
        TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow,
    };
    use crate::test_support::{TEST_ARCH_CTM, TEST_LEAD, TEST_TEAM};
    use crate::types::{AgentName, ChatId, IsoTimestamp, PaneId, TeamName};

    fn base_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: AgentName::from_validated(TEST_LEAD),
            sender_chat_id: None,
            sender_team: TeamName::from_validated(TEST_TEAM),
            authenticated_source_host: None,
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
    fn qualified_sender_identity_preserves_chat_id() {
        let mut event = base_event();
        event.sender_chat_id = Some("chat-42".parse::<ChatId>().expect("chat id"));

        assert_eq!(
            qualified_sender_identity(&event),
            format!("{TEST_LEAD}:chat-42@{TEST_TEAM}")
        );
    }

    #[test]
    fn qualified_sender_identity_includes_compact_authenticated_peer_host() {
        let mut event = base_event();
        event.authenticated_source_host = Some("rand-m5.local".parse().expect("peer host"));

        assert_eq!(
            qualified_sender_identity(&event),
            format!("{TEST_LEAD}@{TEST_TEAM}.rand-m5")
        );
    }

    #[test]
    fn render_built_in_nudge_populates_placeholders() {
        let rendered = render_built_in_nudge(
            &base_event(),
            default_template(BuiltInNudgeTemplateKind::DeliveryTaskAck),
        )
        .expect("rendered template");
        assert!(rendered.contains(&format!("{TEST_LEAD}@{TEST_TEAM}")));
        assert!(rendered.contains("01KX1TEST00000000000000000"));
    }
}
