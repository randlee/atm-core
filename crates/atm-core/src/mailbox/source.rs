use crate::address::AgentAddress;
use crate::config;
use crate::error::AtmError;
use crate::types::{AgentName, TeamName};

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct SourcedMessage {
    pub envelope: crate::schema::InboxMessage,
    pub source_path: std::path::PathBuf,
    pub source_index: crate::types::SourceIndex,
}

#[derive(Debug)]
pub(crate) struct ResolvedTarget {
    pub agent: AgentName,
    pub team: TeamName,
    pub explicit: bool,
}

pub(crate) fn resolve_target(
    target_address: Option<&AgentAddress>,
    actor: &AgentName,
    caller_team: &TeamName,
    _config: Option<&config::AtmConfig>,
) -> Result<ResolvedTarget, AtmError> {
    let Some(target_address) = target_address else {
        return Ok(ResolvedTarget {
            agent: actor.clone(),
            team: caller_team.clone(),
            explicit: false,
        });
    };

    let team = target_address
        .team()
        .cloned()
        .unwrap_or_else(|| caller_team.clone());
    Ok(ResolvedTarget {
        agent: target_address.agent().clone(),
        team,
        explicit: true,
    })
}

#[cfg(test)]
mod tests {
    use super::resolve_target;
    use crate::config::AtmConfig;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, TeamName};

    #[test]
    fn resolve_target_forwards_a_valid_member_token_without_config_alias_resolution() {
        let config = AtmConfig {
            default_team: Some(TEST_TEAM.parse().expect("team")),
            ..Default::default()
        };

        let target = resolve_target(
            Some(&"tl".parse().expect("address")),
            &TEST_SENDER.parse().expect("agent"),
            &TEST_TEAM.parse().expect("team"),
            Some(&config),
        )
        .expect("target");

        assert_eq!(target.agent, AgentName::from_validated("tl"));
        assert_eq!(target.team, TeamName::from_validated(TEST_TEAM));
        assert!(target.explicit);
    }
}
