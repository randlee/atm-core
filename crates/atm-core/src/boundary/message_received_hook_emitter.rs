use super::{BuiltInPostSendDispatch, PostSendEmissionPath, sealed};
use crate::error::AtmError;

/// BOUNDARY-MessageReceivedHookEmitter — see docs/atm-core/boundaries.md.
///
/// A recipient-side notification is emitted only after the immutable message
/// record has been committed to the recipient's SQLite store.
pub trait MessageReceivedHookEmitter: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when one direct receiver-side emission attempt fails
    /// after durable message persistence has already succeeded.
    fn emit_message_received(
        &self,
        dispatch: &BuiltInPostSendDispatch,
    ) -> Result<PostSendEmissionPath, AtmError>;
}
