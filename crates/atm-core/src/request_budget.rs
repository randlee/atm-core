//! Re-export of the shared same-host client/server request-budget contract.
//!
//! `atm-storage-rusqlite` (the SQLite storage backend) and `atm-http-runtime`
//! (the Tokio/Axum daemon runtime) must not depend on each other, and
//! `atm-storage-rusqlite` must not depend on `atm-core`, so `atm-storage` is
//! the one crate reachable by both and owns the actual constants. This
//! module re-exports them for `atm-core` and its callers (the same-host HTTP
//! client in `atm-http-runtime`, `atm-daemon-bootstrap`) that sit above
//! `atm-storage` in the dependency graph without a new crate edge. See
//! [`atm_storage::request_budget`] for the full client/server budget
//! contract and why the two must never be equal.

pub use atm_storage::request_budget::{
    BUSY_TIMEOUT_MARGIN, RESPONSE_HANDOFF_GRACE, SAME_HOST_REQUEST_DEADLINE, SERVER_REQUEST_BUDGET,
    SQLITE_BUSY_TIMEOUT,
};
