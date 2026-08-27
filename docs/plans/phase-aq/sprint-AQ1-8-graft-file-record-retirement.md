---
status: complete
branch: feature/aq-1-8-graft-file-record-retirement
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/aq-1-8-graft-file-record-retirement
---

# Sprint AQ1.8 — Graft File-Record Retirement + Receiver-Singleton Finding Closure

Status: complete · Branch: `feature/aq-1-8-graft-file-record-retirement` off
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
   flock and AQ1.6's `graft_receiver_lock_path_from_root` REMAIN
   (same-host mutual exclusion is still their job; only the record file
   and its path builders die). Note: `write_receiver_record` (the
   truncating-write function, AI3133 defect #2) may carry a small interim
   atomic-write hardening commit from a develop fix branch by the time
   this sprint lands — it is deleted regardless of any such interim
   commit; the deletion is a strict superset of any hardening applied to
   it.
2. **Test migration**: unit tests in `graft.rs`, `atm-graft/src/lib.rs`
   (the bare-workspace activation test now on the AQ1.6 `bind` signature)
   and `atm-graft/src/runtime.rs` that exercised file write/read/republish
   are rewritten against the registry lease model;
   `crates/atm-core/tests/graft_receiver_ownership.rs` and
   `crates/atm-architecture/tests/graft_receiver_ownership_boundary.rs`
   updated to assert the new ownership model (flock + daemon lease +
   generation), reviewed by `boundary-guard` as explicit diffs.
   **Concrete replacement for `graft_receiver_ownership_boundary.rs`'s
   exact-count assertions (closes M5)**: today's test greps
   `crates/atm-core/src/graft.rs` for three literal `write_receiver_record(`
   call sites and one `fs::remove_file(&self.record_path)` site, all of
   which are deleted by deliverable 1, so the existing assertions cannot
   survive unmodified. The rewritten test instead asserts: (a)
   `crates/atm-core/src/graft.rs` production source contains zero
   occurrences of `write_receiver_record`, `read_receiver_record`,
   `graft_receiver_record_path_from_home`, and
   `graft_receiver_record_path_from_root` (structural belt-and-suspenders
   on top of AC #1's workspace grep gate); (b) `ReceiverOwnershipGuard::acquire(`
   and `impl Drop for ReceiverOwnershipGuard` each still appear exactly
   once (the flock primitive is retained, singular); (c)
   `crates/atm-graft/src/runtime.rs` contains exactly one
   `impl Drop for RegisteredGraftReceiver` (AQ1.6 deliverable 4) and the
   daemon `.unregister(` call appears only inside it; (d)
   `crates/atm-graft-python/src/lib.rs` still contains none of the deleted
   file-record identifiers and no direct `.register(`/`.unregister(`/`.refresh(`
   calls against the daemon client — Python bindings never manage
   registration lifecycle directly, only through the Rust runtime loop.
3. **Finding closure**: `.triage/phase-AI/findings/AI3133-HERMES-GRAFT-RECEIVER-SINGLETON-UNSAFE.ttl` (exactly this file — 11 distinct findings share the AI3133 prefix; none of the others are touched) updated
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
   workspace (including tests and `crates/atm-graft/src/lib.rs`
   explicitly — the AQ1.6 `bind`-signature migration is a precondition,
   verified here); `ReceiverOwnershipGuard` and
   `graft_receiver_lock_path_from_root` still present with their tests.
2. Rewritten ownership/boundary tests (deliverable 2's concrete
   replacement assertions) pass; `boundary-guard` sign-off on the
   boundary-test diff recorded in the PR.
3. `.triage/phase-AI/findings/AI3133-HERMES-GRAFT-RECEIVER-SINGLETON-UNSAFE.ttl` shows the supersession Resolution covering its three occurrences (bind-overwrite, record-truncate-write, drop-unconditional-delete) with
   `merge-base --is-ancestor` evidence.
4. Upgrade test: a fixture tree with a pre-existing record file binds
   cleanly, registers, and removes the orphan.

## Required validation

- `cargo test` workspace green on both CI lanes.

## Non-closure / out of scope

- **Accepted residual risk (disclosed, recorded in ADR-056; Rand signs off
  via plan approval)**: after the file is retired, a receiver that binds
  while the daemon is down has NO backing record until its first
  successful registration tick after the daemon returns. A delivery
  attempt in that window gets the AQ1.7 receiver-not-registered error.
  Worst case once the daemon is reachable is bounded by
  `GRAFT_LEASE_REFRESH_INTERVAL` (1s); while the daemon is down, delivery
  was impossible anyway (the daemon performs delivery). This narrow
  cold-start window replaces today's behavior where bind writes the file
  regardless of daemon state — disclosed here, not silently introduced.
- **Capability at-rest encoding and DB permission story (closes I12)**:
  `capability` is stored as the same base64url string
  `LocalCapability::to_base64url()`/`parse_base64url()` already produce
  for the JSON record's `capability_base64url` field and the
  `GraftPostSendWireRequest` wire type — no new encoding is introduced.
  The JSON record's per-secret `0o600` file mode
  (`apply_owner_only_record_mode`, `crates/atm-core/src/graft.rs:663-667`)
  is deleted with the file; the capability's confidentiality going
  forward rests on the shared SQLite DB's existing file-level protection,
  which today has no dedicated `0o600`-style hardening in this codebase
  (verified: no `set_permissions`/`mode(0o600)` call touches the shared DB
  file in `crates/atm-storage-rusqlite/src/shared_db.rs`) — the same
  protection roster rows and message bodies already rely on. This is
  disclosed as parity with today's DB-wide posture, not a regression
  introduced by this sprint and not a new guarantee either; dedicated
  SQLite-file permission hardening (if wanted) is a separate,
  phase-agnostic daemon-storage task, not scoped here.
- hermes-atm wheel bump/smoke (AQ1.9).
- Any multi-receiver-per-agent design (explicitly out; ADR-056 records
  single-active-lease as the model).

## Dependencies

- must_follow: AQ1.7 (nothing may still read the file). Merge-forward
  trigger: AQ1.7 dev push.
- parallel_safe: AQ2.6, AQ2.7 (Herdr — disjoint files; 2026-08-26 reorder);
  AQ1.9 (disjoint: Rust deletion vs Python wheel
  bump/smoke; no shared files).
