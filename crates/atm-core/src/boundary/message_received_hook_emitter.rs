use std::future::Future;
use std::pin::Pin;

use super::{BuiltInPostSendDispatch, PostSendEmissionPath, sealed};
use crate::api::RequestDeadline;
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
    fn emit_received_message(
        &self,
        dispatch: &BuiltInPostSendDispatch,
        deadline: RequestDeadline,
    ) -> Result<PostSendEmissionPath, AtmError>;
}

/// Replacement-runtime boundary for one cancellable receiver-side emission.
///
/// The replacement Tokio runtime awaits this future against the request's
/// inherited deadline. Dropping the future is the cancellation signal: an
/// implementation must not hide an unbounded thread, process, or queue behind
/// it. The legacy synchronous emitter remains reference-only until Phase AM
/// deletes the legacy daemon.
pub trait AsyncMessageReceivedHookEmitter: sealed::Sealed + Send + Sync {
    /// Starts one receiver-side emission after durable persistence.
    ///
    /// The returned future must cooperatively finish or clean up when dropped
    /// because the caller's request deadline expires.
    fn emit_received_message(
        &self,
        dispatch: BuiltInPostSendDispatch,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>>;
}
