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
    fn error_codes_round_trip_through_json() {
        let code = AtmErrorCode::MessageValidationFailed;
        let encoded = serde_json::to_string(&code).expect("serialize error code");
        let decoded: AtmErrorCode = serde_json::from_str(&encoded).expect("deserialize error code");

        assert_eq!(decoded, code);
    }
}
