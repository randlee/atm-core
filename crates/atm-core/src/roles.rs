// Keep the core role surface stable while sharing canonical identities with
// the lower-level storage contract. Storage adapters must not depend upward on
// atm-core just to use these values.
pub use atm_storage::roles::{ROLE_QUALITY_MANAGER, ROLE_TEAM_LEAD, TEAM_ATM_DEV};
