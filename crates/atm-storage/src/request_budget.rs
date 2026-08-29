//! Shared request-budget contract for same-host HTTP transport and storage.
//!
//! `atm-http-runtime`'s Tokio/Axum server and its SQLite storage backend
//! (`atm-storage-rusqlite`) each enforce their own bounded wait, and those
//! bounds must stay in a fixed relationship or a same-host client can time
//! out before the server sharing its host clock ever replies. This module is
//! the one source of truth for that relationship:
//!
//! - [`SERVER_REQUEST_BUDGET`] bounds how long the daemon spends handling one
//!   HTTP request end to end, including any SQLite writer-lock wait.
//! - [`RESPONSE_HANDOFF_GRACE`] is added on top of the server budget to
//!   derive a same-host client's absolute request deadline
//!   ([`SAME_HOST_REQUEST_DEADLINE`]), covering client-side work that starts
//!   before the server's own timer (endpoint record read, connect, request
//!   write) and the response transit after the server finishes.
//! - [`SQLITE_BUSY_TIMEOUT`] bounds SQLite's writer-lock wait so lock
//!   contention alone can never by itself consume the entire
//!   [`SERVER_REQUEST_BUDGET`], leaving [`BUSY_TIMEOUT_MARGIN`] for the rest
//!   of request processing (transaction body, received-message hook,
//!   response encoding).
//!
//! Every constant here is derived from [`SERVER_REQUEST_BUDGET`] so the
//! invariants (`SAME_HOST_REQUEST_DEADLINE > SERVER_REQUEST_BUDGET` and
//! `SQLITE_BUSY_TIMEOUT < SERVER_REQUEST_BUDGET`) hold by construction and
//! cannot silently regress when one side of the contract changes.

use std::time::Duration;

/// Upper bound on how long the daemon spends handling one HTTP request,
/// including any SQLite writer-lock wait.
///
/// `atm-daemon-bootstrap` wires this into the Tokio/Axum runtime's
/// `RuntimeTimeouts` (server-side per-request timeout); `atm-http-runtime`'s
/// received-message hook and blocking-core bridge size their own sub-budgets
/// from the same `RequestDeadline`. Changing this value changes both how
/// long a same-host client is willing to wait (see
/// [`SAME_HOST_REQUEST_DEADLINE`]) and how long a SQLite writer-lock wait
/// may block (see [`SQLITE_BUSY_TIMEOUT`]).
pub const SERVER_REQUEST_BUDGET: Duration = Duration::from_secs(3);

/// Grace added on top of [`SERVER_REQUEST_BUDGET`] for a same-host client's
/// absolute request deadline.
///
/// A same-host client's clock starts before the server's: it must read the
/// local endpoint record, connect, and write the request before the server
/// begins timing the request, and it still needs to receive and decode the
/// response after the server finishes. This grace is kept small and of the
/// same order of magnitude as the existing same-host connect/handoff grace
/// constants in `atm_core::graft` (`RECEIVER_HOOK_CONNECT_DEADLINE`,
/// `RECEIVER_HOOK_RESULT_HANDOFF_GRACE`) so [`SAME_HOST_REQUEST_DEADLINE`]
/// does not mask a genuinely stuck server; 250ms is generous for a loopback
/// TCP or Unix-socket round trip plus response decode.
pub const RESPONSE_HANDOFF_GRACE: Duration = Duration::from_millis(250);

/// Absolute request deadline used by same-host ATM clients (CLI, graft).
///
/// Always strictly greater than [`SERVER_REQUEST_BUDGET`] so a same-host
/// client cannot time out before the server-side budget it is waiting on
/// has elapsed. Previously the client and server budgets were both a fixed
/// 3s, so whenever the server used its full budget the client's clock
/// (which starts earlier) always elapsed first, and the client learned
/// nothing about a durable server-side success. See the module docs for
/// the full contract.
pub const SAME_HOST_REQUEST_DEADLINE: Duration =
    SERVER_REQUEST_BUDGET.saturating_add(RESPONSE_HANDOFF_GRACE);

/// Margin reserved out of [`SERVER_REQUEST_BUDGET`] for request processing
/// that happens after a SQLite writer-lock wait: transaction body
/// execution, the received-message hook, and response encoding.
pub const BUSY_TIMEOUT_MARGIN: Duration = Duration::from_millis(500);

/// Upper bound for SQLite's `busy_timeout`, derived so a writer-lock wait
/// alone can never consume the entire [`SERVER_REQUEST_BUDGET`].
///
/// Previously this was a fixed 5s value that exceeded the 3s server request
/// budget outright, so a lock wait longer than the request budget could
/// never succeed within it. Deriving it from the same shared constant keeps
/// the two bounds consistent by construction.
pub const SQLITE_BUSY_TIMEOUT: Duration = SERVER_REQUEST_BUDGET.saturating_sub(BUSY_TIMEOUT_MARGIN);

// Compile-time enforcement of the two contract invariants above: a same-host
// client must always outlive the server budget it waits on, and a SQLite
// writer-lock wait must never by itself consume the entire server budget.
// These fail the build (not just a test run) if either derived constant
// above is ever redefined in a way that breaks the contract.
const _: () = assert!(SAME_HOST_REQUEST_DEADLINE.as_nanos() > SERVER_REQUEST_BUDGET.as_nanos());
const _: () = assert!(SQLITE_BUSY_TIMEOUT.as_nanos() < SERVER_REQUEST_BUDGET.as_nanos());

#[cfg(test)]
mod tests {
    use super::{
        BUSY_TIMEOUT_MARGIN, RESPONSE_HANDOFF_GRACE, SAME_HOST_REQUEST_DEADLINE,
        SERVER_REQUEST_BUDGET, SQLITE_BUSY_TIMEOUT,
    };

    /// The same-host client deadline must never be reachable before the
    /// server's own request budget elapses, or a same-host client always
    /// times out first and the "durable success + advisory warning" design
    /// can never reach it. This is a `const` assertion made a test so a
    /// regression fails loudly in CI rather than only in an integration
    /// harness.
    #[test]
    fn same_host_deadline_exceeds_server_budget() {
        assert!(
            SAME_HOST_REQUEST_DEADLINE > SERVER_REQUEST_BUDGET,
            "same-host client budget ({SAME_HOST_REQUEST_DEADLINE:?}) must exceed \
             the server request budget ({SERVER_REQUEST_BUDGET:?})"
        );
        assert_eq!(
            SAME_HOST_REQUEST_DEADLINE,
            SERVER_REQUEST_BUDGET + RESPONSE_HANDOFF_GRACE
        );
    }

    /// A SQLite writer-lock wait must never by itself consume the entire
    /// server request budget, or a lock wait can never succeed within it.
    #[test]
    fn busy_timeout_stays_within_server_budget() {
        assert!(
            SQLITE_BUSY_TIMEOUT < SERVER_REQUEST_BUDGET,
            "sqlite busy_timeout ({SQLITE_BUSY_TIMEOUT:?}) must stay below \
             the server request budget ({SERVER_REQUEST_BUDGET:?})"
        );
        assert_eq!(
            SQLITE_BUSY_TIMEOUT,
            SERVER_REQUEST_BUDGET - BUSY_TIMEOUT_MARGIN
        );
    }
}
