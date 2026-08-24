# Sprint AQ1.8 — Graft File-Record Retirement + AI3133 Closure

Status: draft · Branch: `feature/aq-1-8-graft-file-retirement` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: Cipher-311d · recommended_model: fast

Fourth graft connection-model sprint (see AQ1.5). With no readers left
(AQ1.7), the file-record mechanism is deleted outright — eliminating the
AI3133 TOCTOU class rather than patching it.

## Deliverables

1. **Deletions** in `crates/atm-core/src/graft.rs`:
   `write_receiver_record`, `read_receiver_record`,
   `graft_receiver_record_path_from_home` / `_from_root`, the file half of
   `republish_if_missing` (the timer keeps only the AQ1.6 lease refresh),
   and the record-file cleanup in `Drop`. The `ReceiverOwnershipGuard`
   flock REMAINS (same-host mutual exclusion is still its job; only the
   record file dies). Note: this function may carry a small interim
   atomic-write hardening commit from a develop fix branch — it is deleted
   regardless; the deletion is a strict superset.
2. **Test migration**: unit tests in `graft.rs` and
   `atm-graft/src/runtime.rs` that exercised file write/read/republish are
   rewritten against the registry lease model;
   `crates/atm-core/tests/graft_receiver_ownership.rs` and
   `crates/atm-architecture/tests/graft_receiver_ownership_boundary.rs`
   updated to assert the new ownership model (flock + daemon lease +
   generation), reviewed by `boundary-guard` as explicit diffs.
3. **AI3133 closure**: `.triage/phase-AI/findings/AI3133-...ttl` updated
   with a supersession Resolution referencing ADR-056 and this sprint's
   merge commit (ancestry-verified per the standing resolution rule):
   defect #1 (no exclusivity) — fixed by AI.36's flock, retained; defect
   #3 (unconditional Drop delete) — fixed by generation-checked semantics,
   now enforced in the store; defect #2 (truncating write) — eliminated
   with the file.
4. **Stale-file cleanup**: one-time removal of orphaned
   `.atm/graft/<team>/<agent>.json` (+ `.lock` siblings are kept — flock
   still uses them) on receiver bind, so upgraded fleets don't accumulate
   dead records.

## Acceptance criteria

1. Grep gates: zero occurrences of `write_receiver_record`,
   `read_receiver_record`, or `graft_receiver_record_path` anywhere in the
   workspace (including tests); `ReceiverOwnershipGuard` still present with
   its tests.
2. Rewritten ownership/boundary tests pass; `boundary-guard` sign-off on
   the boundary-test diff recorded in the PR.
3. AI3133 record shows the supersession Resolution with
   `merge-base --is-ancestor` evidence.
4. Upgrade test: a fixture tree with a pre-existing record file binds
   cleanly, registers, and removes the orphan.
5. `cargo test` workspace green on both CI lanes.

## Non-closure / out of scope

- hermes-atm wheel bump/smoke (AQ1.9).
- Any multi-receiver-per-agent design (explicitly out; ADR-056 records
  single-active-lease as the model).

## Dependencies

- must_follow: AQ1.7 (nothing may still read the file). Merge-forward
  trigger: AQ1.7 dev push.
- parallel_safe: AQ1.9 (disjoint: Rust deletion vs Python wheel
  bump/smoke; no shared files).
