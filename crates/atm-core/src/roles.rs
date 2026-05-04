use std::sync::LazyLock;

use crate::types::AgentName;

/// Reserved product role name used by workflows that target the team lead.
// rule-008: allow-next-line -- reserved production role literal; all other
// code must reference this constant instead of repeating the string.
pub const ROLE_TEAM_LEAD: &str = "team-lead";

/// Typed lead-role identity shared by runtime code that must target the
/// reserved team-lead mailbox without re-validating the same literal.
pub static TEAM_LEAD_AGENT: LazyLock<AgentName> =
    LazyLock::new(|| AgentName::from_validated(ROLE_TEAM_LEAD));
