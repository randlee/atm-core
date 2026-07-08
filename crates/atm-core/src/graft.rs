//! Thin graft-facing daemon client contracts shared by embedded host agents.

use crate::ack::{AckOutcome, AckRequest};
use crate::error::AtmError;
use crate::read::{ReadOutcome, ReadQuery};
use crate::send::{SendOutcome, SendRequest};

/// Open unary client surface for embedded ATM consumers.
///
/// This trait is intentionally not sealed. `atm-graft` must be able to
/// implement the concrete same-host client in a separate crate without taking
/// a Rust dependency on `atm-daemon`.
pub trait AtmGraftClient: Send + Sync {
    /// Execute one send-shaped ATM compose request.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the underlying daemon-backed send path cannot
    /// complete successfully.
    fn send_message(&self, request: SendRequest) -> Result<SendOutcome, AtmError>;

    /// Execute one ATM read request through the same daemon-backed semantic
    /// path used by the retained CLI.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the read request cannot be delivered or the
    /// daemon returns a typed failure.
    fn read_message(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError>;

    /// Execute one send-shaped ATM acknowledgement request.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the acknowledgement request cannot be
    /// completed successfully.
    fn acknowledge_message(&self, request: AckRequest) -> Result<AckOutcome, AtmError>;
}

#[cfg(test)]
mod tests {
    use super::AtmGraftClient;
    use crate::ack::{AckOutcome, AckRequest};
    use crate::error::AtmError;
    use crate::read::{ReadOutcome, ReadQuery};
    use crate::send::{SendOutcome, SendRequest};

    #[derive(Debug)]
    struct MockGraftClient;

    impl AtmGraftClient for MockGraftClient {
        fn send_message(&self, _request: SendRequest) -> Result<SendOutcome, AtmError> {
            panic!("send_message should not be called in trait object test")
        }

        fn read_message(&self, _query: ReadQuery) -> Result<ReadOutcome, AtmError> {
            panic!("read_message should not be called in trait object test")
        }

        fn acknowledge_message(&self, _request: AckRequest) -> Result<AckOutcome, AtmError> {
            panic!("acknowledge_message should not be called in trait object test")
        }
    }

    #[test]
    fn atm_graft_client_trait_is_object_safe() {
        let client: &dyn AtmGraftClient = &MockGraftClient;
        let _ = client;
    }
}
