//! Shared request-budget contract for same-host HTTP transport.
//!
//! `atm-http-runtime`'s Tokio/Axum server and its same-host clients enforce
//! bounded waits, and those bounds must stay in a fixed relationship or a
//! client can time out before the server sharing its host clock ever replies.
//! This module is the one source of truth for that relationship:
//!
//! - [`SERVER_REQUEST_BUDGET`] bounds how long the daemon spends handling one
//!   HTTP request end to end.
//! - [`RESPONSE_HANDOFF_GRACE`] is added on top of the server budget to
//!   derive a same-host client's absolute request deadline
//!   ([`SAME_HOST_REQUEST_DEADLINE`]), covering client-side work that starts
//!   before the server's own timer (endpoint record read, connect, request
//!   write) and the response transit after the server finishes.
//!
//! The 2.5/3.0/3.25 second ladder assumes no more than 0.5 seconds of
//! scheduler overhead per hop. A loaded host can breach a wall-clock
//! observation even when these product constants and typed outcomes remain
//! correct; that is backlog awareness, not a runtime retry or timeout fix.
//!
//! The client deadline is derived from [`SERVER_REQUEST_BUDGET`] so it stays
//! strictly above the server budget and cannot silently regress when either
//! side of the contract changes.

use std::time::Duration;

/// Upper bound on how long the daemon spends handling one HTTP request.
///
/// `atm-daemon-bootstrap` wires this into the Tokio/Axum runtime's
/// `RuntimeTimeouts` (server-side per-request timeout); `atm-http-runtime`'s
/// received-message hook and blocking-core bridge size their own sub-budgets
/// from the same `RequestDeadline`. Changing this value changes both how
/// long a same-host client is willing to wait (see
/// [`SAME_HOST_REQUEST_DEADLINE`]).
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

/// Reserve, in milliseconds, kept out of [`SERVER_REQUEST_BUDGET`] for the
/// hook runner's own work after a hook's timeout (design §9.7).
pub const HOOK_ACTION_RESERVE_MS: u32 = 500;

/// Largest `timeout_ms` a task hook may register: the server budget minus
/// [`HOOK_ACTION_RESERVE_MS`] (D-m). Enforced only by `check_timeout` in
/// `atm-http-runtime`'s `hook_registry_routes.rs`, called from `hook_register`;
/// [`crate::HookRegistration`] and the store do not re-check it.
pub const HOOK_TIMEOUT_MAX_MS: u32 = 2500;

/// Largest `timeout_ms` a cron `cron.tick` or `cron.stop` hook may register
/// (Rand 2026-10-18, design §9.8). Enforced only by `check_timeout` in
/// `atm-http-runtime`'s `hook_registry_routes.rs`, called from `cron_start`;
/// [`crate::NewCronSchedule`] and the store do not re-check it.
pub const CRON_TIMEOUT_MAX_MS: u32 = 30000;

// `as_millis` returns u128; the budget constants are all u32.
const _: () = assert!(
    (HOOK_TIMEOUT_MAX_MS + HOOK_ACTION_RESERVE_MS) as u128 == SERVER_REQUEST_BUDGET.as_millis()
);

// Compile-time enforcement of the client/server deadline contract. This
// fails the build if the client deadline is ever redefined below the server
// budget it waits on.
const _: () = assert!(SAME_HOST_REQUEST_DEADLINE.as_nanos() > SERVER_REQUEST_BUDGET.as_nanos());

#[cfg(test)]
mod tests {
    use super::{RESPONSE_HANDOFF_GRACE, SAME_HOST_REQUEST_DEADLINE, SERVER_REQUEST_BUDGET};

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
}
