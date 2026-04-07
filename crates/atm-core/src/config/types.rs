#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AtmConfig {
    pub identity: Option<String>,
    pub default_team: Option<String>,
    pub(crate) obsolete_identity_present: bool,
}
