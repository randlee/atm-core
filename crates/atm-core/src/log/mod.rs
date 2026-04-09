pub mod filters;

// Deferred retained-log service surface.
//
// Phase L bound ATM to the shared `sc-observability` integration directly.
// A dedicated `atm-core::log` service layer remains intentionally deferred
// until a later phase needs crate-owned query orchestration beyond the current
// adapter boundary.
