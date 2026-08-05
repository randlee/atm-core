//! Compile-failing tombstone for the removed peer resend coordinator.
//!
//! Cross-host delivery is an immediate `messages[]` request followed by one
//! confirmation for that request. Recovery must not reintroduce a sender-side
//! coordinator, queue, due callback, or admission gate.

/// Removed. The crate-wide `#![deny(deprecated)]` policy makes every attempted
/// use of this type a compiler error.
#[deprecated(
    note = "PeerResendScheduler was removed; use direct peer batch delivery and confirmation"
)]
#[allow(dead_code)]
pub(crate) struct PeerResendScheduler {
    _private: (),
}
