use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AtmConfig {
    /// Deprecated compatibility-only field for legacy `.atm.toml` parsing.
    ///
    /// ATM no longer uses config identity as a runtime fallback. Callers must
    /// use `ATM_IDENTITY` or an explicit sender override instead.
    pub identity: Option<String>,
    pub default_team: Option<String>,
    pub team_members: Vec<String>,
    pub aliases: BTreeMap<String, String>,
    pub post_send_hook: Option<Vec<String>>,
    pub post_send_hook_members: Vec<String>,
    pub config_root: PathBuf,
    pub(crate) obsolete_identity_present: bool,
}
