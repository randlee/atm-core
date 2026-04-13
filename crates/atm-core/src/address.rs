use std::str::FromStr;

use crate::error::AtmError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAddress {
    pub agent: String,
    pub team: Option<String>,
}

impl FromStr for AgentAddress {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::address_parse("agent name must not be empty"));
        }

        match trimmed.split_once('@') {
            Some((agent, team)) => {
                validate_path_segment(agent, "agent")?;
                validate_path_segment(team, "team")?;

                Ok(Self {
                    agent: agent.to_string(),
                    team: Some(team.to_string()),
                })
            }
            None => {
                validate_path_segment(trimmed, "agent")?;
                Ok(Self {
                    agent: trimmed.to_string(),
                    team: None,
                })
            }
        }
    }
}

fn validate_path_segment(value: &str, kind: &str) -> Result<(), AtmError> {
    if value.is_empty() {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not be empty"
        )));
    }

    if value.starts_with('.') {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not start with '.'"
        )));
    }

    if value.contains("..") {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not contain '..'"
        )));
    }

    if value.contains(['/', '\\']) {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not contain path separators"
        )));
    }

    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(AtmError::address_parse(format!(
            "{kind} name contains invalid characters"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::AgentAddress;

    #[test]
    fn parses_bare_agent_address() {
        let parsed = AgentAddress::from_str("arch-ctm").expect("address");
        assert_eq!(parsed.agent, "arch-ctm");
        assert_eq!(parsed.team, None);
    }

    #[test]
    fn parses_agent_with_team() {
        let parsed = AgentAddress::from_str("arch-ctm@atm-dev").expect("address");
        assert_eq!(parsed.agent, "arch-ctm");
        assert_eq!(parsed.team.as_deref(), Some("atm-dev"));
    }

    #[test]
    fn rejects_empty_agent_name() {
        assert!(AgentAddress::from_str("").is_err());
        assert!(AgentAddress::from_str("@atm-dev").is_err());
    }

    #[test]
    fn rejects_invalid_team_segment() {
        assert!(AgentAddress::from_str("arch-ctm@").is_err());
        assert!(AgentAddress::from_str("arch-ctm@atm@dev").is_err());
    }

    #[test]
    fn rejects_path_traversal_and_separator_segments() {
        assert!(AgentAddress::from_str("../evil").is_err());
        assert!(AgentAddress::from_str("../../passwd").is_err());
        assert!(AgentAddress::from_str("team/subdir").is_err());
        assert!(AgentAddress::from_str(r"team\\subdir").is_err());
        assert!(AgentAddress::from_str(".hidden").is_err());
        assert!(AgentAddress::from_str("a..b@team").is_err());
        assert!(AgentAddress::from_str("a...b@team").is_err());
    }

    #[test]
    fn accepts_valid_segment_characters() {
        let parsed = AgentAddress::from_str("valid-team_name.1").expect("address");
        assert_eq!(parsed.agent, "valid-team_name.1");
        assert_eq!(parsed.team, None);

        let parsed = AgentAddress::from_str("arch-ctm@atm-dev").expect("address");
        assert_eq!(parsed.agent, "arch-ctm");
        assert_eq!(parsed.team.as_deref(), Some("atm-dev"));
    }
}
