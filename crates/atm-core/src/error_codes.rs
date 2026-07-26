//! Canonical ATM error-code vocabulary for service and CLI consumers.
//!
//! The implementation lives in the dependency-light `atm-error` crate so
//! lower-layer storage contracts can use the same type without depending on
//! this higher-level service crate.

pub use atm_error::{AtmErrorCode, UnknownAtmErrorCode};
