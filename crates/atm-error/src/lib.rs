//! Stable ATM error-code vocabulary shared across crate layers.
//!
//! This crate is intentionally dependency-light so lower-layer contracts can
//! classify failures without depending on the higher-level `atm-core` crate.

mod error_codes;

pub use error_codes::{AtmErrorCode, UnknownAtmErrorCode};

#[cfg(test)]
mod tests {
    use super::AtmErrorCode;

    #[test]
    fn remote_delivery_unconfirmed_keeps_its_stable_wire_code() {
        assert_eq!(
            AtmErrorCode::RemoteDeliveryUnconfirmed.as_str(),
            "REMOTE_DELIVERY_UNCONFIRMED"
        );
        assert_eq!(
            "REMOTE_DELIVERY_UNCONFIRMED".parse::<AtmErrorCode>(),
            Ok(AtmErrorCode::RemoteDeliveryUnconfirmed)
        );
    }

    #[test]
    fn peer_wire_policy_error_codes_keep_their_stable_wire_spellings() {
        for (code, wire) in [
            (
                AtmErrorCode::PeerWireModeInvalid,
                "ATM_PEER_WIRE_MODE_INVALID",
            ),
            (
                AtmErrorCode::PeerWireModeSourceForbidden,
                "ATM_PEER_WIRE_MODE_SOURCE_FORBIDDEN",
            ),
            (
                AtmErrorCode::PeerWirePlaintextAuthenticationRequired,
                "ATM_PEER_WIRE_PLAINTEXT_AUTHENTICATION_REQUIRED",
            ),
        ] {
            assert_eq!(code.as_str(), wire);
            assert_eq!(wire.parse::<AtmErrorCode>(), Ok(code));
        }
    }

    #[test]
    fn error_codes_round_trip_through_json() {
        let code = AtmErrorCode::MessageValidationFailed;
        let encoded = serde_json::to_string(&code).expect("serialize error code");
        let decoded: AtmErrorCode = serde_json::from_str(&encoded).expect("deserialize error code");

        assert_eq!(decoded, code);
    }

    #[test]
    fn template_send_error_codes_keep_their_stable_wire_spelling() {
        for (code, wire) in [
            (AtmErrorCode::TemplateLoadFailed, "TEMPLATE_LOAD_FAILED"),
            (
                AtmErrorCode::TemplateHashApiFailed,
                "TEMPLATE_HASH_API_FAILED",
            ),
            (
                AtmErrorCode::TemplateInspectionParseFailed,
                "TEMPLATE_INSPECTION_PARSE_FAILED",
            ),
            (
                AtmErrorCode::TemplateRequiredVariableMissing,
                "TEMPLATE_REQUIRED_VARIABLE_MISSING",
            ),
            (
                AtmErrorCode::TemplateRenderVerificationFailed,
                "TEMPLATE_RENDER_VERIFICATION_FAILED",
            ),
            (
                AtmErrorCode::TemplateIncludeUnresolved,
                "TEMPLATE_INCLUDE_UNRESOLVED",
            ),
            (
                AtmErrorCode::TemplateClassificationInvalid,
                "TEMPLATE_CLASSIFICATION_INVALID",
            ),
            (
                AtmErrorCode::TemplateWorkflowInvalid,
                "TEMPLATE_WORKFLOW_INVALID",
            ),
            (
                AtmErrorCode::TemplateWorkflowValueInvalid,
                "TEMPLATE_WORKFLOW_VALUE_INVALID",
            ),
            (AtmErrorCode::TemplateTagReserved, "TEMPLATE_TAG_RESERVED"),
            (
                AtmErrorCode::WorkflowQueryInvalid,
                "ATM_WORKFLOW_QUERY_INVALID",
            ),
            (
                AtmErrorCode::WorkflowTelemetryConfigInvalid,
                "ATM_WORKFLOW_TELEMETRY_CONFIG_INVALID",
            ),
            (
                AtmErrorCode::WorkflowTelemetryDropped,
                "ATM_WORKFLOW_TELEMETRY_DROPPED",
            ),
        ] {
            assert_eq!(code.as_str(), wire);
            assert_eq!(wire.parse::<AtmErrorCode>(), Ok(code));
        }
    }
}
