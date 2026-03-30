use std::str::FromStr;

use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAddress {
    pub agent: String,
    pub team: Option<String>,
}

impl FromStr for AgentAddress {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(Error::address_parse("agent name must not be empty"));
        }

        match trimmed.split_once('@') {
            Some((agent, team)) => {
                if agent.is_empty() {
                    return Err(Error::address_parse("agent name must not be empty"));
                }
                if team.is_empty() {
                    return Err(Error::address_parse("team name must not be empty"));
                }
                if team.contains('@') {
                    return Err(Error::address_parse(
                        "address must contain at most one @ separator",
                    ));
                }

                Ok(Self {
                    agent: agent.to_string(),
                    team: Some(team.to_string()),
                })
            }
            None => Ok(Self {
                agent: trimmed.to_string(),
                team: None,
            }),
        }
    }
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
}
