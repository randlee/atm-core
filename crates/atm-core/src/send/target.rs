use std::net::IpAddr;
use std::path::Path;

use crate::address::AgentAddress;
use crate::config;
use crate::error::AtmError;
use crate::types::{AgentName, TeamName};

use super::{SendMessageSource, file_policy, input};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedRecipient {
    pub(crate) agent: AgentName,
    pub(crate) team: TeamName,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct PeerLoopbackHost(String);

impl PeerLoopbackHost {
    pub fn parse(value: &str) -> Result<Self, AtmError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::address_parse("loopback host must not be empty").with_recovery(
                "Use `loopback@localhost` or `loopback@<literal-ip>` before retrying the loopback send.",
            ));
        }
        if trimmed.eq_ignore_ascii_case("localhost") || trimmed.parse::<IpAddr>().is_ok() {
            return Ok(Self(trimmed.to_string()));
        }
        Err(AtmError::address_parse(format!(
            "unsupported loopback host `{trimmed}`; expected `localhost` or a literal IP address"
        ))
        .with_recovery(
            "Use `loopback@localhost` or `loopback@<literal-ip>` before retrying the loopback send.",
        ))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn parse_cli_target(raw_target: &str) -> Option<Result<Self, AtmError>> {
        let (agent, host) = raw_target.trim().split_once('@')?;
        if !agent.eq_ignore_ascii_case("loopback") {
            return None;
        }
        Some(Self::parse(host))
    }
}

pub(crate) fn validate_non_self_recipient(
    sender: &AgentName,
    sender_team: &TeamName,
    recipient: &ResolvedRecipient,
) -> Result<(), AtmError> {
    if sender
        .as_str()
        .eq_ignore_ascii_case(recipient.agent.as_str())
        && sender_team
            .as_str()
            .eq_ignore_ascii_case(recipient.team.as_str())
    {
        return Err(AtmError::self_addressed_send_invalid(format!(
            "self-addressed messages are invalid ATM input: '{sender}@{sender_team}' may not send to itself"
        )));
    }
    Ok(())
}

pub(crate) fn resolve_recipient(
    target_address: &AgentAddress,
    caller_team: &TeamName,
    config: Option<&config::AtmConfig>,
) -> Result<ResolvedRecipient, AtmError> {
    let team = target_address
        .team
        .as_deref()
        .and_then(|team| team.parse().ok())
        .or_else(|| Some(caller_team.clone()))
        .ok_or_else(AtmError::team_unavailable)?;

    Ok(ResolvedRecipient {
        agent: config::aliases::resolve_agent_name(&target_address.agent, config)?,
        team,
    })
}

pub(crate) fn resolve_message_body(
    source: &SendMessageSource,
    current_dir: &Path,
    home_dir: &Path,
    team_name: &TeamName,
) -> Result<String, AtmError> {
    match source {
        SendMessageSource::Inline(message) => input::validate_message_text(message.clone()),
        SendMessageSource::File { path, message } => {
            input::validate_message_text(file_policy::process_file_reference(
                path,
                message.as_deref(),
                team_name,
                current_dir,
                home_dir,
            )?)
        }
    }
}

pub fn qualified_sender_identity(sender: &AgentName, sender_team: Option<&TeamName>) -> String {
    sender_team
        .map(|team| {
            AgentAddress {
                agent: sender.clone(),
                team: Some(team.clone()),
            }
            .to_string()
        })
        .unwrap_or_else(|| sender.to_string())
}

pub fn qualified_sender_origin(
    sender: &AgentName,
    sender_team: Option<&TeamName>,
    remote_host: Option<&str>,
) -> String {
    let identity = qualified_sender_identity(sender, sender_team);
    match remote_host.filter(|host| !host.trim().is_empty()) {
        Some(host) => format!("{identity}.{host}"),
        None => identity,
    }
}

#[cfg(test)]
mod tests {
    use super::{ResolvedRecipient, validate_non_self_recipient};
    use crate::error_codes::AtmErrorCode;
    use crate::types::{AgentName, TeamName};

    #[test]
    fn validate_non_self_recipient_rejects_case_variant_self_target() {
        let error = validate_non_self_recipient(
            &AgentName::from_validated("Sender-A"),
            &TeamName::from_validated("Test-Team"),
            &ResolvedRecipient {
                agent: AgentName::from_validated("sender-a"),
                team: TeamName::from_validated("test-team"),
            },
        )
        .expect_err("case-variant self target must be rejected");

        assert!(error.is_validation(), "{error:?}");
        assert_eq!(error.code, AtmErrorCode::SelfAddressedSendInvalid);
    }
}
