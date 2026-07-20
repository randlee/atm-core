use std::fmt;
use std::str::FromStr;

pub use atm_storage::validate_path_segment;
use serde::{Deserialize, Serialize};

use crate::error::AtmError;
use crate::types::{AgentName, TeamName};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAddress {
    pub agent: AgentName,
    pub team: Option<TeamName>,
}

impl FromStr for AgentAddress {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::address_parse("agent name must not be empty"));
        }

        match trimmed.split_once('@') {
            Some((agent, team)) => Ok(Self {
                agent: agent.parse()?,
                team: Some(team.parse()?),
            }),
            None => Ok(Self {
                agent: trimmed.parse()?,
                team: None,
            }),
        }
    }
}

impl fmt::Display for AgentAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.team {
            Some(team) => write!(f, "{}@{}", self.agent, team),
            None => f.write_str(&self.agent),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::AgentAddress;
    use crate::test_support::{TEST_SENDER, TEST_SENDER_ADDRESS, TEST_TEAM};
    use crate::types::{AgentName, TeamName};

    #[test]
    fn parses_bare_agent_address() {
        let parsed = AgentAddress::from_str(TEST_SENDER).expect("address");
        assert_eq!(parsed.agent, AgentName::from_validated(TEST_SENDER));
        assert_eq!(parsed.team, None);
    }

    #[test]
    fn parses_agent_with_team() {
        let parsed = AgentAddress::from_str(TEST_SENDER_ADDRESS).expect("address");
        assert_eq!(parsed.agent, AgentName::from_validated(TEST_SENDER));
        assert_eq!(parsed.team, Some(TeamName::from_validated(TEST_TEAM)));
    }

    #[test]
    fn rejects_empty_agent_name() {
        assert!(AgentAddress::from_str("").is_err());
        assert!(AgentAddress::from_str(&format!("@{TEST_TEAM}")).is_err());
    }

    #[test]
    fn rejects_invalid_team_segment() {
        assert!(AgentAddress::from_str(&format!("{TEST_SENDER}@")).is_err());
        assert!(AgentAddress::from_str(&format!("{TEST_SENDER}@atm@dev")).is_err());
    }

    #[test]
    fn rejects_path_traversal_and_separator_segments() {
        assert!(AgentAddress::from_str("../evil").is_err());
        assert!(AgentAddress::from_str("../../passwd").is_err());
        assert!(AgentAddress::from_str("team/subdir").is_err());
        assert!(AgentAddress::from_str(r"team\\subdir").is_err());
        assert!(AgentAddress::from_str(".hidden").is_err());
        assert!(AgentAddress::from_str("bad:name").is_err());
        assert!(AgentAddress::from_str("bad name").is_err());
        assert!(AgentAddress::from_str("a..b@team").is_err());
        assert!(AgentAddress::from_str("a...b@team").is_err());
    }

    #[test]
    fn accepts_valid_segment_characters() {
        let parsed = AgentAddress::from_str("valid-team_name1").expect("address");
        assert_eq!(parsed.agent, AgentName::from_validated("valid-team_name1"));
        assert_eq!(parsed.team, None);

        let parsed = AgentAddress::from_str(TEST_SENDER_ADDRESS).expect("address");
        assert_eq!(parsed.agent, AgentName::from_validated(TEST_SENDER));
        assert_eq!(parsed.team, Some(TeamName::from_validated(TEST_TEAM)));
    }

    #[test]
    fn display_round_trips_bare_and_qualified_addresses() {
        assert_eq!(
            AgentAddress::from_str(TEST_SENDER)
                .expect("address")
                .to_string(),
            TEST_SENDER
        );
        assert_eq!(
            AgentAddress::from_str(TEST_SENDER_ADDRESS)
                .expect("address")
                .to_string(),
            TEST_SENDER_ADDRESS
        );
    }
}
