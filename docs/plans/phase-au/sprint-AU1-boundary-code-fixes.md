# Sprint AU.1 — Boundary Code Fixes (easy wave)

status: complete
assignee: cipher
difficulty: easy
branch: feature/pau-s1-boundary-code-fixes (off integrate/phase-au)
pr_target: integrate/phase-au
parallel_safe: AU.2, AU.3 (disjoint file sets; no must_follow edges)
master_plan: [boundary-regression-plan.md](../boundary-regression-plan.md) §1, §2.1, §2.2

## Scope

Clear 10 sc-boundary findings with mechanical code fixes that change no behavior and
loosen no boundary. The exact per-finding change, call sites, and review-r1 caveats are
authoritative in the master plan — this doc lists the work items and the sprint-specific
test deliverables.

Work items (master-plan section → finding index #):
1. §1.1 `SearchInput` → `From` impl (#3) — includes the review-r1 public-API audit
   before deleting `into_request`.
2. §1.2 `teams.rs` `reload_runtime_view` hoist to free fn, 7 call sites (#4).
3. §1.3 test-only `SharedLogWriter` holds `Arc<Mutex<Vec<u8>>>` (#5).
4. §1.4 delete dead `impl From<MessageEnvelope> for RawMessageEnvelope` (#7).
5. §1.5 inline `inject_nudge` wrapper into the trait method (#19).
6. §1.6 `#[sc_lint(boundary.allow("cycle.recursive_value_container"))]` on
   `LogFieldValue` + `LogFieldMap`; add `sc-lint-attributes` dep to atm-core (#2).
7. §2.1 `HelperThreadPermit` holds `Arc<AtomicUsize>`; same-counter-after-CAS
   condition per review r1 (#6).
8. §2.2 hoist ALL `ScComposeTemplateComposer` associated helpers (exhaustive `Self::`
   sweep incl. `source_text`, `hash_api_error`) to free fns (#17 + #22).
9. §2.3 (optional) `SendCommand` error-builder hoist (#12) — do it if convenient;
   otherwise #12 is owned by AU.2's classifier fix. State which in the completion
   report.
10. **#20 residual (added 2026-08-26, routed from AU.2)** — AU.2's accepted §4.3
    classifier clears #20's call-callee edges, but one edge survives:
    `atm-runtime/src/legacy_storage_adapters.rs:162` passes `Self::mailbox_row` as a
    *function value* to `Iterator::map` (a `references_expr` edge with no call_callee;
    broad non-call exclusion was reviewed and REJECTED). Fix in app code: hoist
    `mailbox_row` to a free fn (arch-ctm's recommendation, same pattern as item 8).
    File verified NOT a Phase-AM deletion target (master plan §4.3 note). This adds
    the atm-runtime crate to this sprint's file set — still disjoint from AU.2
    (lint crate only) and AU.3 (ack/send/write + tripwire).

## Test deliverables (from the 2026-08-26 coverage audit — write BEFORE or WITH the fix)

- **teams.rs (item 2)**: success-path `.run()` tests for the four uncovered call sites
  — `AddMemberCommand`, `UpdateMemberCommand`, `RemoveMemberCommand`,
  `RestoreCommand` (applied branch, not dry-run) — so all 7 call sites reach
  `reload_runtime_view()` under test. (Today only the three nudge-template commands do.)
- **HelperThreadBudget/Permit (item 7)**: the four review-r1-mandated tests, none of
  which exist today: (a) failed acquire leaves `inflight` unchanged; (b) N concurrent
  `try_acquire` racing never exceed `max_inflight`; (c) permit drop releases exactly one
  slot; (d) dropping `HelperThreadBudget` while permits are live does not invalidate
  outstanding permits.
- Item 5 needs no new test: trait-path dispatch is already covered by
  `hard_accept_failure_rebinds_and_resumes_authenticated_delivery` (verified in audit);
  confirm it still passes.

## Acceptance criteria

- The owned findings (index #2,3,4,5,6,7,17,19,20,22 and #12 if item 9 done) are absent
  from the sc-boundary JSON payload (`.just/lint_sc_boundary.py` underlying command,
  full `findings[]` — not the 3-line preview), verified by finding identity.
- **No new sc-boundary findings introduced**; findings owned by AU.2/AU.3 untouched.
- `cargo build`, `just test`, `just lint` (all non-sc-boundary rules) pass.
- New tests above present and passing; no unrelated files modified.
- Public-API audit result for `into_request` recorded in the PR description.

## Validation

`just test`; `just lint`; sc-boundary full-payload diff against the 22-finding baseline
showing exactly the owned findings removed.

## QA history

- **QA-AU1-R1** (2026-08-26, quality-mgr msg 01M1009YE73X77FWNKA831CFCE): **PASS**,
  0 blocking / 1 important / 2 minor. Deliverables 18/18; all 10 owned sc-boundary
  findings fixed + 8 mandated tests confirmed by independent reviewers and
  quality-mgr's direct sc-lint-boundary run (baseline 22 → 13, zero added, exactly 9
  removed, identity match to owned scope). First reviewer round was terminated by a
  session-wide API usage limit and re-dispatched in full; partial notes were treated
  as non-evidence. Deferred debt: RBQA-F001 (duplicate `reload_runtime_view` impls,
  pre-existing on develop), RSH-001, RBP-F001 (both pre-existing). Report: PR #1034
  comment 5431447685. Merged to integrate/phase-au @ cf232d8b7.
