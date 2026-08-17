//! Compatibility re-export for the shared ATM error-code vocabulary.
//!
//! `AtmErrorCode` is owned by the dependency-light `atm-error` crate so the
//! storage layer does not define or duplicate service-layer error semantics.

pub use atm_error::{AtmErrorCode, UnknownAtmErrorCode};
