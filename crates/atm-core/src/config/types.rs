use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AtmConfig {
    pub identity: Option<String>,
    pub default_team: Option<String>,
}
