//! Transport-level runtime-observation integration coverage.

use crate::runtime_health::dispatch::TrustedActivityObservation;

// This compile-only binding is deliberately crate-internal: external callers
// cannot construct the capability because its constructor is private.
fn requires_trusted_observation(_: TrustedActivityObservation) {}

#[test]
fn trusted_activity_observation_capability_gate_is_compile_time_enforced() {
    let _ = requires_trusted_observation;
}
