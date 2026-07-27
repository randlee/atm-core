use std::fmt;
use std::str::FromStr;

pub use atm_storage::validate_path_segment;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::AtmError;
use crate::types::{AgentIdentity, AgentName, ChatId, HostName, TeamName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAddress {
    agent: AgentName,
    chat_id: Option<ChatId>,
    team: Option<TeamName>,
    host: Option<HostName>,
}

#[derive(Serialize, Deserialize)]
struct AgentAddressWire {
    agent: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chat_id: Option<ChatId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    team: Option<TeamName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host: Option<HostName>,
}

impl AgentAddress {
    /// Creates an address while enforcing that a host is always team-qualified.
    pub fn new(
        agent: AgentName,
        chat_id: Option<ChatId>,
        team: Option<TeamName>,
        host: Option<HostName>,
    ) -> Result<Self, AtmError> {
        if host.is_some() && team.is_none() {
            return Err(AtmError::address_parse(
                "a host-qualified address must also specify a team",
            ));
        }
        Ok(Self {
            agent,
            chat_id,
            team,
            host,
        })
    }

    #[must_use]
    pub fn agent(&self) -> &AgentName {
        &self.agent
    }

    #[must_use]
    pub fn chat_id(&self) -> Option<&ChatId> {
        self.chat_id.as_ref()
    }

    #[must_use]
    pub fn team(&self) -> Option<&TeamName> {
        self.team.as_ref()
    }

    #[must_use]
    pub fn host(&self) -> Option<&HostName> {
        self.host.as_ref()
    }
}

impl Serialize for AgentAddress {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        AgentAddressWire {
            agent: self.agent.clone(),
            chat_id: self.chat_id.clone(),
            team: self.team.clone(),
            host: self.host.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AgentAddress {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = AgentAddressWire::deserialize(deserializer)?;
        Self::new(wire.agent, wire.chat_id, wire.team, wire.host).map_err(serde::de::Error::custom)
    }
}

/// Selects which participant position a chat-qualified filter applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantDirection {
    From,
    To,
    Either,
}

/// Canonical participant filter. An absent chat id selects the base identity,
/// distinct from every chat-qualified identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageParticipantFilter {
    pub agent: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<ChatId>,
    pub direction: ParticipantDirection,
}

impl FromStr for AgentAddress {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::address_parse("agent name must not be empty"));
        }

        match trimmed.split_once('@') {
            Some((identity, destination)) => {
                if destination.contains('@') {
                    return Err(AtmError::address_parse("address may contain only one '@'"));
                }
                let identity: AgentIdentity = identity.parse()?;
                let (team, host) = match destination.split_once('.') {
                    Some((team, host)) => (team.parse()?, Some(host.parse()?)),
                    None => (destination.parse()?, None),
                };
                Self::new(identity.agent, identity.chat_id, Some(team), host)
            }
            None => {
                let identity: AgentIdentity = trimmed.parse()?;
                Self::new(identity.agent, identity.chat_id, None, None)
            }
        }
    }
}

impl fmt::Display for AgentAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let identity = AgentIdentity::new(self.agent.clone(), self.chat_id.clone());
        match (&self.team, &self.host) {
            (Some(team), Some(host)) => write!(f, "{identity}@{team}.{host}"),
            (Some(team), None) => write!(f, "{identity}@{team}"),
            (None, None) => write!(f, "{identity}"),
            // `AgentAddress::new` prevents this state through every safe
            // construction boundary. Keep the formatter total anyway: a
            // malformed value must not turn `format!("{}", address)` into a
            // panic if it is introduced by legacy state or an internal bug.
            (None, Some(host)) => write!(
                f,
                "<invalid-agent-address: {identity} has host {host} without team>"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::AgentAddress;
    use crate::test_support::{TEST_SENDER, TEST_SENDER_ADDRESS, TEST_TEAM};
    use crate::types::{AgentName, ChatId, HostName, TeamName};

    #[test]
    fn parses_bare_agent_address() {
        let parsed = AgentAddress::from_str(TEST_SENDER).expect("address");
        assert_eq!(parsed.agent, AgentName::from_validated(TEST_SENDER));
        assert_eq!(parsed.chat_id, None);
        assert_eq!(parsed.team, None);
        assert_eq!(parsed.host, None);
    }

    #[test]
    fn parses_agent_with_team() {
        let parsed = AgentAddress::from_str(TEST_SENDER_ADDRESS).expect("address");
        assert_eq!(parsed.agent, AgentName::from_validated(TEST_SENDER));
        assert_eq!(parsed.chat_id, None);
        assert_eq!(parsed.team, Some(TeamName::from_validated(TEST_TEAM)));
        assert_eq!(parsed.host, None);
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
    fn rejects_host_without_team_at_every_construction_boundary() {
        let agent = AgentName::from_validated(TEST_SENDER);
        let host = "peer.example.test".parse::<HostName>().expect("host");
        assert!(AgentAddress::new(agent, None, None, Some(host)).is_err());
        assert!(
            serde_json::from_str::<AgentAddress>(
                r#"{"agent":"sender","host":"peer.example.test"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn display_reports_invalid_internal_state_without_panicking() {
        // This state cannot be constructed through the public API, but the
        // formatter must remain total if legacy or corrupted state reaches it.
        let address = AgentAddress {
            agent: AgentName::from_validated(TEST_SENDER),
            chat_id: None,
            team: None,
            host: Some("peer.example.test".parse().expect("host")),
        };

        let rendered = std::panic::catch_unwind(|| address.to_string())
            .expect("invalid address formatting must not panic");
        assert_eq!(
            rendered,
            format!(
                "<invalid-agent-address: {TEST_SENDER} has host peer.example.test without team>"
            )
        );
    }

    #[test]
    fn rejects_path_traversal_and_separator_segments() {
        assert!(AgentAddress::from_str("../evil").is_err());
        assert!(AgentAddress::from_str("../../passwd").is_err());
        assert!(AgentAddress::from_str("team/subdir").is_err());
        assert!(AgentAddress::from_str(r"team\\subdir").is_err());
        assert!(AgentAddress::from_str(".hidden").is_err());
        assert!(AgentAddress::from_str("bad:name:again").is_err());
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
    fn parses_and_renders_chat_qualified_addresses() {
        let address = format!("omega-prime:1234@{TEST_TEAM}.localhost");
        let parsed = AgentAddress::from_str(&address).expect("chat-qualified address");
        assert_eq!(parsed.agent, AgentName::from_validated("omega-prime"));
        assert_eq!(
            parsed.chat_id,
            Some("1234".parse::<ChatId>().expect("chat id"))
        );
        assert_eq!(parsed.team, Some(TeamName::from_validated(TEST_TEAM)));
        assert_eq!(
            parsed.host,
            Some("localhost".parse::<HostName>().expect("host"))
        );
        assert_eq!(parsed.to_string(), address);
    }

    #[test]
    fn parses_and_preserves_ipv4_host_qualified_address() {
        let address = format!("{TEST_SENDER}@{TEST_TEAM}.192.168.128.82");
        let parsed = AgentAddress::from_str(&address).expect("IPv4 host-qualified address");

        assert_eq!(parsed.team, Some(TeamName::from_validated(TEST_TEAM)));
        assert_eq!(
            parsed.host,
            Some("192.168.128.82".parse::<HostName>().expect("IPv4 host"))
        );
        assert_eq!(parsed.to_string(), address);
    }

    #[test]
    fn rejects_empty_host_segment_without_team_fallback() {
        let error = AgentAddress::from_str(&format!("{TEST_SENDER}@{TEST_TEAM}."))
            .expect_err("empty host must be a typed address parse failure");
        assert_eq!(
            error.code(),
            crate::error_codes::AtmErrorCode::AddressParseFailed
        );
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
