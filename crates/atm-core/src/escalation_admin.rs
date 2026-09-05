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

/// Validate an ADR-040 recipient while retaining the configured spelling.
pub fn validate_address(address: &str) -> Result<(), AtmError> {
    address
        .parse::<AgentAddress>()
        .map(|_| ())
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
    validate_address(address)?;
    store.add_escalation_recipient(target, address, at)
}

pub fn remove(
    store: &(dyn TaskStore + Send + Sync),
    target: &EscalationScope,
    address: &str,
) -> Result<bool, AtmError> {
    validate_address(address)?;
    store.remove_escalation_recipient(target, address)
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
    use super::{scope, validate_address};
    use atm_storage::EscalationScope;

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
}
