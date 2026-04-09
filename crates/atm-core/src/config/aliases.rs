use super::AtmConfig;

pub fn resolve_agent(value: &str, config: Option<&AtmConfig>) -> String {
    config
        .and_then(|config| config.aliases.get(value))
        .cloned()
        .unwrap_or_else(|| value.to_string())
}

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

    use super::{preferred_alias, resolve_agent};
    use crate::config::AtmConfig;

    #[test]
    fn resolve_agent_returns_canonical_name_when_alias_exists() {
        let mut aliases = BTreeMap::new();
        aliases.insert("tl".to_string(), "team-lead".to_string());
        let config = AtmConfig {
            aliases,
            ..Default::default()
        };

        assert_eq!(resolve_agent("tl", Some(&config)), "team-lead");
        assert_eq!(resolve_agent("team-lead", Some(&config)), "team-lead");
    }

    #[test]
    fn preferred_alias_returns_first_alias_for_canonical_name() {
        let mut aliases = BTreeMap::new();
        aliases.insert("lead".to_string(), "team-lead".to_string());
        aliases.insert("tl".to_string(), "team-lead".to_string());
        let config = AtmConfig {
            aliases,
            ..Default::default()
        };

        assert_eq!(
            preferred_alias("team-lead", Some(&config)).as_deref(),
            Some("lead")
        );
        assert_eq!(preferred_alias("arch-ctm", Some(&config)), None);
    }
}
