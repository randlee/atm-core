# AC Freshness Flags: Smoke Checklist Row Risk Assessment

## Purpose

Phase AC made substantial changes to the daemon composition layer, ack module, storage
abstraction, and notification model. Some AB smoke rows were written against pre-AC
expected outputs. This table identifies which rows carry elevated re-verification risk
and why.

Recommendation: before freezing AB.1 and before commencing any cross-host lane, verify
the flagged rows' expected outputs against a current develop build on at least one host.

---

## Risk Table

| Row ID | Command / Action | AC Risk Level | Reason |
|---|---|---|---|
| AB-SMOKE-001 | `atm doctor --json` on both hosts | Moderate | `crates/atm/src/commands/doctor.rs` changed substantially. Doctor now reflects the AC storage layer and `StorageNotifier` health. JSON field names or structure may differ from AB plan expectations. Verify output schema before freezing expected output in checklist. |
| AB-SMOKE-002 | `atm list`, `atm clear`, `atm send`, `atm read --all --json` same-host | Moderate | `composition.rs` (~150 lines changed) and `members.rs` / `teams.rs` updated for AC storage model. List and send commands route through the new storage layer. Expected output formats should be re-confirmed against a current build. |
| AB-SMOKE-003 | Win→macOS one-way send (cross-host) | Blocked | Blocked by missing receiver listener (see `executability-gap.md`). Risk level N/A until listener sprint lands. |
| AB-SMOKE-004 | macOS→Win one-way send (cross-host) | Blocked | Same block as AB-SMOKE-003. |
| AB-SMOKE-005 | Receiver reads message (cross-host) | Blocked | Blocked by missing receiver listener. |
| AB-SMOKE-006 | Receiver confirms receipt (cross-host) | Blocked | Blocked by missing receiver listener. |
| AB-SMOKE-007 | Cross-host ack round-trip | High (when unblocked) | `crates/atm-core/src/ack/mod.rs` changed ~82 lines in AC. Ack now routes through `StorageNotifier` trait. The ack round-trip expected behavior (timing, confirmation frame, sender-side receipt) must be re-verified against the new ack path before this row can be considered correctly specified. |
| AB-SMOKE-008 | Degraded notification after durable send | High (when unblocked) | `StorageNotifier` is a new AC trait. Degraded-state notifications previously triggered through a direct code path; they now route through `StorageNotifier`. The observable degraded signal (output format, timing, JSON field names in doctor output) may have changed. |
| AB-SMOKE-009 | Retry-visible interruption / recovery | High (when unblocked) | Daemon restart behavior depends on composition wiring in `composition.rs` (150 lines changed) and the new runtime layer (`atm-runtime` crate added). Retry visibility and recovery semantics should be re-confirmed against the AC composition graph before this row's expected output is treated as authoritative. |
| AB-SMOKE-010 | Copied-state Lane B revalidation | Moderate (when unblocked) | Gated on Lane A pass. Inherits all Lane A risks. No additional AC-specific risk beyond what applies to the rows it reruns. |

---

## Recommended Action Before Freezing AB.1

1. Build release binaries from `c451afe4` (or later develop HEAD) on one host.
2. Run a disposable clean-room bring-up (see `ab1-execution-readiness.md`).
3. Execute `atm doctor --json` and capture the full JSON output. Compare field names
   and structure against what AB-SMOKE-001 specifies as expected output.
4. Execute `atm list`, `atm clear`, `atm send`, and `atm read --all --json` and capture
   outputs. Compare against AB-SMOKE-002 expected output.
5. Update the frozen `cross-host-smoke-checklist.md` to reflect any output differences
   before the checklist is signed off.

Rows AB-SMOKE-007, AB-SMOKE-008, and AB-SMOKE-009 should be similarly re-verified
when the listener sprint lands and cross-host execution becomes possible.
