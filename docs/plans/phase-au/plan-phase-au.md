# Phase AU — Boundary Debt Retirement

status: proposed
branch: plan/boundary-regression
target: develop
integration_branch: integrate/phase-au
tracking: GH issue #1028; triage waiver AO2-SCBOUNDARY-DEBT-001
master_analysis: [boundary-regression-plan.md](../boundary-regression-plan.md)

## Why now

PR #966 merged phase-ao2 to `develop` on 2026-08-26. Because that merge carries the
QA-RUSTQA-AO2-001 lint-wiring fix, `just validate` on `develop` now runs the sc-boundary
lint and reports the 22 pre-existing findings waived at the AO2 gate — **CI on `develop`
is red today** on those findings. Phase AU exists to retire that debt and turn CI green
again, permanently.

Note (plan-QA 2026-08-26): the merge-commit CI run (job 33002585446, commit 9923ef6c)
also shows two failures **separate from** the sc-boundary debt: (1) macOS Test —
`cargo binstall` bootstrap-tooling failure (infra); (2) Windows Test — a genuine test
panic in `storage_and_nudge_router.rs`
(`local_ack_routes_to_the_received_peer_and_peer_receipt_does_not_reacknowledge`).
These are tracked outside Phase AU's scope but are carved out below so the interim-CI
check remains applicable. The Windows panic is in the ack-routing flow AU.3 touches —
AU.3's entry tests must pin behavior with that failure understood first.
Triage update (2026-08-26): the Windows panic is classified **flaky/timing, one-off**
(hard-coded 1 s test budget expired on a degraded runner; test died in setup before any
ack assertion; same tree passed the four preceding Windows runs) — not a product bug.
Details and side findings recorded in sprint-AU3 work item 1.

## Constraint (non-negotiable)

**No boundary loosening.** No rule leaves `just lint all`/`validate`; no baseline or
ignore file is introduced; no trait-wide or workspace allowlist. The only suppression
used anywhere is the lint's own per-type
`#[sc_lint(boundary.allow("cycle.recursive_value_container"))]` opt-in on one genuine
recursive value container (master plan §1.6) — its designed purpose. Lint-side changes
make the rules *more precise* (narrowing verified false-positive classes with pinning
tests), never weaker.

## Authoritative decomposition

The full 22-finding analysis — per-finding exact changes, arch-ctm's round-1 critical
review, and the finding-to-section index — is the master plan,
[boundary-regression-plan.md](../boundary-regression-plan.md). This phase doc owns
sequencing, sprint scoping, and gates; the master plan owns the technical content. The
three sprints are ordered easiest → hardest so the easiest findings close out first,
and are assigned for **fully parallel execution**:

| Sprint | Difficulty | Assignee | Master-plan sections | Findings owned | Crates touched |
|---|---|---|---|---|---|
| AU.1 code fixes | easy | Cipher | §1 + §2.1 + §2.2 | 10 (index #2-5,6,7,17,19,22 + optional #12) | atm (incl. composition.rs), atm-core (search/observability), atm-graft, atm-storage, atm-template-sc-compose |
| AU.2 lint calibration | mid | arch-ctm | §4.1 + §4.2 + §4.3 | 11 (index #8-11,13-16,18,20,21) | sc-lint-boundary only |
| AU.3 ack/send write module | hard | fenix (team-lead) | §3.1 | 1 (index #1) | atm-core (ack/send/new write), atm-architecture (tripwire test) |

**Parallelism proof**: the three file sets are disjoint — AU.1 never touches
`crates/sc-lint-boundary` or `ack`/`send`; AU.2 touches nothing outside the lint crate;
AU.3 touches nothing in AU.1's fix list. `parallel_safe` across all three pairs; no
`must_follow` edges between sprints. Each sprint's gate is verified by **finding
identity** (its owned findings absent from the sc-boundary JSON payload, no new
findings introduced), never by total count — so no sprint waits on another to measure
success. The one soft ordering is merge order: merge PRs easiest-first
(AU.1 → AU.2 → AU.3) as they individually pass QA, so `develop`-visible debt shrinks
fastest and each later merge-forward is small.

Sprint docs:
- [sprint-AU1-boundary-code-fixes.md](./sprint-AU1-boundary-code-fixes.md)
- [sprint-AU2-lint-calibration.md](./sprint-AU2-lint-calibration.md)
- [sprint-AU3-ack-send-write-module.md](./sprint-AU3-ack-send-write-module.md)

## Test-coverage baseline (audit 2026-08-26, develop @ 9923ef6cb)

A full coverage audit of every touched code path found the changes split cleanly:

**Adequately covered today** (existing tests are the regression net): §1.1 search
conversion (both call sites), §1.3 test-support writer, §1.4 dead-code deletion, §1.5
injector trait path (`hard_accept_failure_rebinds_and_resumes_authenticated_delivery`
already dispatches through `&dyn HostNudgeInjector`), §1.6 LogFieldValue/Map recursive
round-trips, §2.2 composer helpers (33-test behavioral suite), and every §4.2-flagged
method's own unit test.

**Gaps — new tests are a sprint deliverable, written BEFORE or WITH the change:**
1. *AU.1*: teams.rs — only 3 of 7 `reload_runtime_view` call sites have success-path
   `.run()` coverage; `AddMember`/`UpdateMember`/`RemoveMember`/`Restore(Applied)` never
   reach the call. Add success-path tests for those four before the hoist.
2. *AU.1*: `HelperThreadBudget`/`HelperThreadPermit` — zero isolated tests exist; the
   four review-r1-mandated tests (failed-acquire no-op, concurrent-cap under race,
   single-slot release on drop, budget-drop with live permits) are all new. Largest
   gap relative to risk.
3. *AU.2*: sc-lint-boundary has no graph-identity pinning at all (only a trivial
   `NodeId::new("")` panic test) — the §4.1 identity tests and all §4.2/§4.3
   classifier-boundary tests are new by definition.
4. *AU.3*: the admission control flow has exactly one end-to-end test
   (`local_ack_routes_to_the_received_peer_and_peer_receipt_does_not_reacknowledge`,
   atm-http-runtime, async variant only); the sync `admit_acknowledgement_write` has no
   dedicated atm-core test. Direct unit tests for both variants are an AU.3 entry
   requirement. Also: `atm-architecture/tests/boundary_enforcement.rs:362-406`
   (`acknowledgement_cannot_restore_a_second_write_pipeline`) literally greps
   `send/mod.rs`/`ack/mod.rs` source for the current cross-module call strings — it
   must be deliberately updated (kept as a tripwire against a second write pipeline,
   re-pointed at the new module paths), never deleted or weakened.

## Sequencing and gates

- Branch model: standard phase pattern — `integrate/phase-au` off `develop`; sprint
  branches `feature/pau-s1-boundary-code-fixes`, `feature/pau-s2-lint-calibration`,
  `feature/pau-s3-ack-send-write-module` off `integrate/phase-au`; sprint PRs target
  `integrate/phase-au`; one final PR `integrate/phase-au → develop`.
- All three sprints dispatch **simultaneously** (parallel_safe, disjoint file sets, see
  table above). Per repo policy, each sprint that merges to `integrate/phase-au` is
  merged forward into the still-open sprint branches before their PRs.
- AU.3's single internal precondition is its design-confirmation step (sprint doc step
  0): the sibling write-module direction is arch-ctm's own round-1 recommendation and is
  the accepted baseline; step 0 confirms the module member set before code moves. This
  does not block AU.1/AU.2.
- Phase exit gate: sc-boundary reports **0 findings**; `just validate` and `just test`
  fully green with sc-boundary armed; AU.3's official benchmark campaign meets the
  baselines.json floors; waiver AO2-SCBOUNDARY-DEBT-001 closed; GH #1028 closed.
- Interim-CI note: `develop` stays red until AU.1+AU.2 merge back. If an unrelated PR
  must merge to develop mid-phase, its CI failure set must be verified to be exactly
  the known sc-boundary findings (count and identity) plus, at most, the two
  known-separate failures carved out in "Why now" (macOS binstall infra failure;
  Windows `storage_and_nudge_router.rs` ack-routing panic) — until those two are
  resolved, at which point the carve-out shrinks accordingly. Anything else is new.

## Benchmark protection (AU.3)

The ack/send write pipeline is the throughput hot path. AU.3 is constrained to
mechanical relocation (no signature changes, no new indirection on the hot path) and
gated on an official benchmark campaign on the isolated m5-atmbench account (never
rand-m5) meeting the standing baselines.json floors before its PR merges. The standing
rule that no fix may regress achieved benchmark numbers applies.

## Review history

- 2026-08-26 — arch-ctm critical review round 1 of the master analysis: verdict NEEDS
  PLAN REVISION; all 8 corrections folded (see master plan "Review history"). The §3.1
  direction (sibling write module) and the §4.2/§4.3 narrowed classifiers in the sprint
  docs are the post-review versions.
- 2026-08-26 — test-coverage audit (background agent, develop @ 9923ef6cb) folded in as
  the coverage-baseline section above; gaps became explicit sprint deliverables.
- 2026-08-26 — quality-mgr plan-QA round 1 @ 9a9ebb0a5: verdict **PASS** (findings
  non-blocking). (a) decomposition completeness PASS (all 22 traced, #12 single-owner in
  both conditional branches); (b) parallelism PASS, cosmetic crate mislabel fixed
  (composition.rs is the atm crate); (c) test deliverables PASS; (d) benchmark gate PASS
  with note — target matrix + marginal-miss policy added to sprint AU.3; (e) important:
  "Why now" overstated CI purity — develop's merge-commit CI also has a macOS binstall
  infra failure and a Windows ack-routing test panic; premise corrected and interim-CI
  carve-out added. All findings folded this commit.
