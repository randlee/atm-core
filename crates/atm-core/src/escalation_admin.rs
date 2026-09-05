//! Administrative operations for daemon and team escalation recipients.

use atm_storage::{EscalationScope, TaskStore};

use crate::address::AgentAddress;
use crate::error::AtmError;
use crate::types::IsoTimestamp;

/// Parse the optional team selector used by the escalation CLI.
pub fn scope(team: Option<&str>) -> Result<EscalationScope, AtmError> {
    team.map(|value| value.parse().map(EscalationScope::Team))
        .unwrap_or(Ok(EscalationScope::Daemon))
}

/// Validate and canonicalize an ADR-040 recipient.
pub fn validate_address(address: &str) -> Result<String, AtmError> {
    address
        .parse::<AgentAddress>()
        .map(|parsed| parsed.to_string())
        .map_err(|error| {
            AtmError::new(
                crate::error_codes::AtmErrorCode::MessageValidationFailed,
                format!(
                    "invalid escalation recipient '{address}': {}",
                    error.message()
                ),
            )
        })
}

pub fn add(
    store: &(dyn TaskStore + Send + Sync),
    target: &EscalationScope,
    address: &str,
    at: IsoTimestamp,
) -> Result<bool, AtmError> {
    let address = validate_address(address)?;
    store.add_escalation_recipient(target, &address, at)
}

pub fn remove(
    store: &(dyn TaskStore + Send + Sync),
    target: &EscalationScope,
    address: &str,
) -> Result<bool, AtmError> {
    let address = validate_address(address)?;
    store.remove_escalation_recipient(target, &address)
}

pub fn list(
    store: &(dyn TaskStore + Send + Sync),
    target: &EscalationScope,
) -> Result<Vec<String>, AtmError> {
    store.list_escalation_recipients(target)
}

#[must_use]
pub fn scope_label(target: &EscalationScope) -> String {
    match target {
        EscalationScope::Daemon => "daemon".to_owned(),
        EscalationScope::Team(team) => format!("team:{team}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{add, list, remove, scope, validate_address};
    use atm_storage::{DummyTaskStore, EscalationScope};

    #[test]
    fn scope_defaults_to_daemon() {
        assert_eq!(scope(None).expect("scope"), EscalationScope::Daemon);
    }

    #[test]
    fn address_validation_accepts_adr040_forms() {
        for address in ["agent", "agent@team", "agent@team.host"] {
            validate_address(address).expect("address");
        }
    }

    #[test]
    fn address_validation_rejects_malformed_input() {
        let error = validate_address("not an address").expect_err("invalid address");
        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::MessageValidationFailed
        );
    }

    #[test]
    fn recipient_mutations_store_canonical_addresses() {
        let store = DummyTaskStore::default();
        let target = EscalationScope::Daemon;
        let timestamp = crate::types::IsoTimestamp::now();
        assert!(add(&store, &target, " ops@atm-dev ", timestamp).expect("add recipient"));
        assert_eq!(
            list(&store, &target).expect("list recipients"),
            vec!["ops@atm-dev"]
        );
        assert!(remove(&store, &target, " ops@atm-dev ").expect("remove recipient"));
        assert!(list(&store, &target).expect("list recipients").is_empty());
    }
}
