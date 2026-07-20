use super::AtmConfig;
use crate::error::AtmError;
use crate::types::AgentName;

pub fn resolve_agent(value: &str, config: Option<&AtmConfig>) -> String {
    config
        .and_then(|config| config.aliases.get(value))
        .cloned()
        .unwrap_or_else(|| value.to_string())
}

pub fn resolve_agent_name(value: &str, config: Option<&AtmConfig>) -> Result<AgentName, AtmError> {
    resolve_agent(value, config).parse()
}

#[allow(
    dead_code,
    reason = "Phase AD obsolete: caller identity alias projection is no longer used after caller-context ownership moved to the CLI boundary."
)]
pub fn preferred_alias(canonical: &str, config: Option<&AtmConfig>) -> Option<String> {
    config.and_then(|config| {
        config
            .aliases
            .iter()
            .find_map(|(alias, resolved)| (resolved == canonical).then(|| alias.clone()))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{preferred_alias, resolve_agent, resolve_agent_name};
    use crate::config::AtmConfig;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::test_support::TEST_SENDER;

    #[test]
    fn resolve_agent_returns_canonical_name_when_alias_exists() {
        let mut aliases = BTreeMap::new();
        aliases.insert("tl".to_string(), ROLE_TEAM_LEAD.to_string());
        let config = AtmConfig {
            aliases,
            ..Default::default()
        };

        assert_eq!(resolve_agent("tl", Some(&config)), ROLE_TEAM_LEAD);
        assert_eq!(resolve_agent(ROLE_TEAM_LEAD, Some(&config)), ROLE_TEAM_LEAD);
    }

    #[test]
    fn resolve_agent_name_rejects_invalid_alias_target() {
        let mut aliases = BTreeMap::new();
        aliases.insert("tl".to_string(), "../bad-agent".to_string());
        let config = AtmConfig {
            aliases,
            ..Default::default()
        };

        let error = resolve_agent_name("tl", Some(&config)).expect_err("invalid alias target");
        assert!(error.code() == crate::error_codes::AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn preferred_alias_returns_first_alias_for_canonical_name() {
        let mut aliases = BTreeMap::new();
        aliases.insert("lead".to_string(), ROLE_TEAM_LEAD.to_string());
        aliases.insert("tl".to_string(), ROLE_TEAM_LEAD.to_string());
        let config = AtmConfig {
            aliases,
            ..Default::default()
        };

        assert_eq!(
            preferred_alias(ROLE_TEAM_LEAD, Some(&config)).as_deref(),
            Some("lead")
        );
        assert_eq!(preferred_alias(TEST_SENDER, Some(&config)), None);
    }
}
