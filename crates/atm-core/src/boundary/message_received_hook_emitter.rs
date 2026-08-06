use super::{BuiltInPostSendDispatch, PostSendEmissionPath, sealed};
use crate::error::AtmError;

/// BOUNDARY-MessageReceivedHookEmitter — see docs/atm-core/boundaries.md.
///
/// A recipient-side notification is emitted only after the immutable message
/// record has been committed. Its error is advisory: the receive path records
/// it as a warning and does not redefine the durable receive result.
pub trait MessageReceivedHookEmitter: sealed::Sealed + Send + Sync {
    /// Attempts one direct receiver-side emission after persistence.
    ///
    /// # Errors
    ///
    /// Returns `AtmError` when the receiver-side emission fails after durable
    /// message persistence has already succeeded.
    fn emit_post_send(
        &self,
        dispatch: &BuiltInPostSendDispatch,
    ) -> Result<PostSendEmissionPath, AtmError>;
}
