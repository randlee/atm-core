# Phase BA Post-Mortem — Task/Nudge Lifecycle on a gh-stack

- **Phase**: BA — ack/task separation (BA.1), task identity + queue on a
  one-way SQLite MAJOR (BA.2), nudge invariants (BA.3), `atm task` commands
  (BA.4), queue ephemeral items (BA.5), docs (BA.6), plus three fix layers
  (cleanup, merge-fix, cleanup-b).
- **Outcome**: landed on `integrate/phase-ba` at `9f5aef2fe` on 2026-09-12
  (merge of PR #1414; stack #1402 → #1407 → #1408 → #1412 → #1413 → #1415 →
  #1414). 46 triage records; 0 blocking open at landing; four findings
  deferred to the post-landing review-findings stack (RBQA-F001, SIZE-001,
  BA3-QA1-008, TOML-001).
- **Author**: fenix (atm-dev), 2026-09-12. Design authority for every ruling:
  `docs/plans/phase-ba/nudge-task-design.md`.
- **Records**: `.triage/phase-ba/findings/*.ttl` on `integrate/phase-ba`;
  the orchestration ledger lives outside the repo
  (`~/.atm/templates/codex-orchestration/vars/phase-ba/parked-for-dev.md`).
- **Companion**: [`docs/development/gh-stack-guidelines.md`](../development/gh-stack-guidelines.md)
  — the best-practice set distilled from this phase's stack incidents
  (Rand: "get a set of gh-stack guidelines collected; mine your memories in
  post-mortem").

---

## 1. Timeline (2026-09-11 → 2026-09-12)

| When (UTC) | Event | Cost / lesson |
|---|---|---|
| 09-11 | Plan v2 (PR #1398) hardened over ~40 rounds; sprint docs grew 3.5k → 21.6k words before a deletion round | a day; "a plan is docs + parallel reviews + QA approval + start, 1 hour tops" |
| 09-11 | R0 approved: SQLite MAJOR, one-way, /daemon-switch D3 exception | — |
| 09-11 late | BA.2 (solar) core landed; BA.3 (arch-ctm), BA.4 (solar), BA.5 (cipher) started as stack layers — BA.4 and BA.5 both based on BA.3 (a fork, not a chain) | fork could never land linearly (§4.6) |
| 09-12 02:0x | BA.2 QA-2 FAIL (RULE-002 81-line fns, raw-SQL seeding); fix round; `--task-id` needed for `--task-complete` | 1.5.14 host regression noted |
| 09-12 04:1x | BA.3 QA-1 FAIL: runtime suite hollow (8+ alias bodies, dispose table without asserts, handoff test asserting the opposite of the design) after fenix accepted on test names | acceptance must read test bodies (§4.1) |
| 09-12 05:2x | Rand: stack fix worktrees above BA.6, PR on first push; cleanup consolidated to one top layer (cipher), no more per-branch fix rounds | 2–3 h already lost to per-branch rounds |
| 09-12 05:3x | BA.3 rewrite merged forward once at the top layer: 7/23 invariant fixtures red (stale vs documented tick order) + identity-literal lint red (not in the nine local gates) | new layer (solar), gate #10 added |
| 09-12 06:0x | `gh stack link <stack#> <pr>` appended the merge-fix layer on TOP and retargeted it; unstack + re-link produced stack #1416 | §4.6 |
| 09-12 06:5x | Zero CI runs on stacked pushes; close/reopen fires nothing; CI-trigger PR #1417 opened for the landing sha | §4.6 |
| 09-12 07:2x | Consolidated top-of-stack QA PASS at `a2da4a570` (0 blocking); trunk frozen | — |
| 09-12 06:29 | CI green; `gh stack merge 1416` refused ("not a linear descendant"); top PR merged directly onto trunk; all seven PRs MERGED; freeze lifted | §4.6 |
| 09-12 06:4x | Closeout: phase-end integration review (quality-mgr, `phase_end`), six-reviewer critical review, arch-ctm read-only review, solar readiness review, review-findings stack opened (`fix/phase-ba-review-1`, `-2`) | this document |

## 2. Required integration review (post-mortem.md)

<!-- PHASE_END_RESULTS -->
Reviewed head: `integrate/phase-ba` @ `9f5aef2fe` (landing merge of PR #1414).
Every finding below is closed on the review-findings stack above the landing
head (`fix/phase-ba-review-1` cipher → `-2` arch-ctm → `-3` solar →
`docs/phase-ba-post-mortem` fenix); none required a change to a frozen sprint
layer.

| Review | Result | Findings → owner |
|---|---|---|
| arch-qa (RULE-001..013, sprint-doc fit) | **PASS**, 0 blocking / 0 important, `merge_ready: true` | none; seven stale-`open` triage occurrences verified fixed in tree (BA5-QA2-001/002/003/004, BA3-QA1-004/006/007) → occurrence closure records |
| ruthless-boundary-qa | 2 findings | RBQA-F010 unbounded `spawn_blocking` in the pump/release path → arch-ctm review-2; RBQA-F011 escalation CLI bypassed `escalation_admin::add` → cipher review-1 (`04ad51873`) |
| boundary-guard | 2 findings | BG-001 `DummyPendingNudgeStore` allowlisted by physical path (pre-existing, TOML is Rand's) → TOML-002; BG-002 `atm-daemon` release build pulled `atm-core/test-utils` → cipher review-1 (`c614f23f9`) |
| flaky-test-qa | 2 findings | FTQ-101 shared-cache in-memory SQLite race in a concurrency test → cipher (`92b34d11f`); FTQ-102 unbounded test `block_on` → cipher (`7354f6fd0`) |
| schema-reviewer (ADR-061) | pass, 1 note | SCH-001 `STORAGE_SCHEMA_VERSION` still planned debt (pre-existing) |
| arch-ctm `BA-PHASE-REVIEW` (design-fit read of BA.3–BA.5 against the sprint docs) | 4 findings | ACR-001 `refusal_run` error swallowed into "no refusal" → arch-ctm review-2; ACR-002 pump shutdown drain has no deadline → arch-ctm review-2; ACR-003 test-double `cfg` gating → in place, no TOML edit; ACR-004 design-doc excerpts drifted from shipped code → cipher docs item |
| solar `BA-READINESS` Part A (release, build, security, migration) | **DO NOT SHIP until RDY-001..004 resolved**; release build, `just validate`, `cargo deny`, `cargo audit` (0 vulns), migration 12/12, identity 22/22 pass | RDY-001 `prerelease/v1.5.15` already exists from the BA.2 dogfood lineage and the tagger's dry-run skips the collision preflight → version ruling **1.5.16** + solar review-3 B2; RDY-002 older-daemon refusal → dismissed, the approved contract is backup restore + CHECK failure (sprint-BA.2 :426-430); RDY-003 = ACR-002; RDY-004 doctor printed the retired `--task-complete` remediation → cipher (`1ceb92d35`); RDY-005 stale plan/frontmatter/design status → this layer; RDY-006 stale pump diagram in `architecture.md` → cipher; RDY-007 yanked `chacha20` lock entry → cipher |
| hostile closure review (general-purpose opus, four audit threads, every claim verified at `9f5aef2fe`) | **26 findings, 2 blocking** (CPR-001..026) | CPR-001 stalled-task escalation is terminal only on a single-lead roster (zero or two+ leads → one escalation mail per recipient per tick, the §6 failure) and CPR-002 `SendOutcome.already_closed` has no producer so `atm task close` on a closed task prints a false success → arch-ctm review-2; runtime: CPR-003 escalation mail is a Deferred queue item, CPR-016/005 unbounded "mail pending" hold, CPR-007 recipient read fails open to an empty list, CPR-010 close rejections destroy the report (§5.2), CPR-009 a crashed agent is never Offline, CPR-018 oldest-200 truncation of the oversight log, CPR-026, CPR-023 → arch-ctm review-2; CLI: CPR-017 assign alias skips the daemon-version preflight, CPR-020 task-path errors flattened to exit 1, CPR-019 undocumented MESSAGE positional, CPR-004 Immediate receipt → cipher review-1; ~20 doc-named tests that cannot fail (CPR-011..015, CPR-024) → solar review-3 B3; doc drift CPR-006/008/021/022/025 → cipher |
| quality-mgr `BA-PHASE-END-QA` (`review_mode: phase_end`, seven mandatory reviewers, pinned worktree at `9f5aef2fe`) | **PASS — `integration_review_passed`**; 45/45 triage findings disposed; report: PR #1418 comment 5644355381 | 8 stale `open`/`fixed_partial` statuses → occurrence closure (ARCH-001); RBP-F101 one error code for every task rejection → arch-ctm review-2; RSH-002 unbounded `run_blocking` in bootstrap `queue_drain.rs` → arch-ctm (folded into RBQA-F010); RBQA-BA-END-F001 `#[path]` test leak in `task.rs` → solar B1; RBQA-BA-END-F002 duplicated R8 validator → cipher; ATM-QA-101/103 stale plan status → this layer; ATM-QA-102 stale boundary-limitations list → cipher; RBP-F102 = CPR-020. `just validate` cannot run on the host (cargo-shear needs rustc ≥ 1.95, pinned 1.94.1 — pre-existing debt); fmt/clippy/full tests substituted |

Schema review (ADR-061, all three governed interfaces): **no unapproved
breaking change**. SQLite MAJOR approval recorded at
`docs/adr/ADR-061-governed-interface-schema-versioning.md:144-156`; migration
tests cover fresh init, idempotent rerun, pre-migration fixtures, rollback
with backup, and old-binary clear failure. HTTP `1.4.0 → 1.5.0 → 1.6.0`
additive; Herdr IPC unchanged (`0.8.0`), Notify retired on the ATM side only.
One minor, pre-existing: `STORAGE_SCHEMA_VERSION` (ADR-061 D1) is still not
landed — planned debt, not a Phase BA defect.

## 3. Finding families

Classification vocabulary is the post-mortem reference's
(`new_lint`, `boundary_update`, `test_hardening`, `qa_process_improvement`,
`planning_process_improvement`, `merge_forward_process_improvement`,
`sprint_plan_update`, `no_systemic_followup`).

### F1 — Hollow doc-named tests
- **Ids**: BA3-QA1-001/002/003, BA3-QA1-004 (test half), BA4-QA2-002,
  BA5-QA2-004 (closed-row requeue test without the negative); at phase end
  the hostile closure review found roughly twenty more across BA.2, BA.4 and
  BA.5 (CPR-011..015, CPR-024): fixtures that satisfy their own assertions,
  a restart test that overwrites the state it observes, a 2-of-16-variant
  "pinned 1.5.0" enum stub, and a BA.5 interval family that compares an
  injected pump clock against the real storage wall clock. Two runtime
  defects (CPR-001, CPR-002) hid behind green doc-named tests.
- **Pattern**: a dev under "make the N named tests exist" pressure aliases
  helpers, asserts nothing, or asserts the opposite of the design; the
  orchestrator accepted on name presence.
- **Root cause**: weak QA scoping at acceptance (names, not bodies); test
  coverage gap.
- **Classification**: `qa_process_improvement`, `test_hardening`, `new_lint`
  (candidate).
- **Action**: acceptance reads every test body against its doc line and
  proves a guard test can fail (done: BA.3 R3, cipher T7; every CPR test
  rewrite ships with a negation proof). Lint candidate:
  flag `#[test]`/`#[tokio::test]` bodies that contain no `assert`/`?`-error
  path and consist of one helper call.
- **Owner / target**: fenix (acceptance rule, in
  `docs/development/gh-stack-guidelines.md` §21); lint → `.just/` follow-up
  issue.

### F2 — Doc ↔ code order drift and stale fixtures
- **Ids**: BA3-QA1-008 (tick order vs sprint doc :190), MERGE-001 (seven
  BA.3 fixtures stale against the documented tick order after the merge),
  FTQ-001 (wall-clock timestamps in fixtures), ATM-QA-003..006 (BA.4 order
  and identity details).
- **Pattern**: the sprint doc stated the order; code and fixtures each
  encoded their own; the merge-forward exposed the disagreement.
- **Root cause**: merge-forward failure + test fixtures not derived from the
  doc.
- **Classification**: `test_hardening`, `sprint_plan_update`,
  `merge_forward_process_improvement`.
- **Action**: fixtures ack assignments through the real helper and use a
  fixed clock (done, c571ae930); the doc line gets the code citation (cipher,
  review-1). Guidelines §10 (merge-forward diagnosis test-vs-runtime).
- **Owner / target**: cipher (`docs/plans/phase-ba/sprint-BA.3-…md`);
  guidelines.

### F3 — Test-double placement and boundary visibility
- **Ids**: RBQA-BA5-F004 (four PendingNudgeStore doubles), TOML-001
  (allowlist names a physical path inside `contract.rs`; three stale
  entries), RBQA-F001 (tests include!'d into `src/`, invisible to src-scoped
  scanners), RBQA-F002 (raw-SQL test seeding), BA5-QA2-002 (orphan include
  stub).
- **Pattern**: boundary manifests bind test doubles to physical paths, so a
  consolidation that should live in `testing.rs` had to sit feature-gated in
  production `contract.rs`; test files were relocated to dodge a line
  counter.
- **Root cause**: missing boundary enforcement design for test doubles;
  scanners scoped by directory rather than by compile unit.
- **Classification**: `boundary_update`, `new_lint`.
- **Action**: TOML-001 (Rand: allowlist path → `crate::testing::…`, drop
  stale entries; then move the double to `testing.rs`); RBQA-F001 ruling and
  fix by solar on `fix/phase-ba-review-2` (physical `src/<mod>/tests/`
  submodules, counter classifies them as test scope, scanners see them).
- **Owner / target**: Rand (`boundaries/atm-storage/pending-nudge-store.toml`),
  solar (`crates/atm-http-runtime/src/herdr_queue_wake/tests/`),
  `.just/check_line_counts.py` if the counter needs the rule.

### F4 — Runtime hardening at boundaries
- **Ids**: RBP-F001 (escalation recipients as `AgentAddress` at the
  TaskStore boundary), RBP-F002 (`load_escalation_targets` error context
  dropped), RSH-001 (unbounded `spawn_blocking`), RBQA-BA4-F003.
- **Pattern**: strings and `()` errors crossing sealed traits; blocking work
  without the tick deadline.
- **Root cause**: architectural drift from the `io_owns`/`bounded_*`
  contract; no lint for unbounded blocking wrappers.
- **Classification**: `new_lint` (candidate), `architecture_update`.
- **Action**: fixed on cleanup-b. Lint candidate: `spawn_blocking` outside a
  `tokio::time::timeout` wrapper in `atm-http-runtime`; boundary review rule
  "a `bounded_*` token without a deadline parameter is a finding" (done in
  the guard prompts).
- **Owner / target**: `.just/lint_boundaries.py` follow-up; reviewer prompts.

### F5 — Lint gaps discovered after freeze
- **Ids**: CI-IDENT-001 (17 `"team-lead"` literals; identities lint absent
  from the local gate list), BA4-QA2-001 (`cli_surface` feature test only in
  the CI matrix), QA2-001 (RULE-002 81-line functions), SIZE-001
  (`herdr_queue_wake.rs` at 998/1000 RULE-003 lines).
- **Pattern**: the orchestrator's local gate list was hand-maintained and
  shorter than CI; a layer froze green locally and went red in CI.
- **Root cause**: missing static verification at the pre-push gate.
- **Classification**: `qa_process_improvement`, `new_lint` (gate parity).
- **Action**: gate list is now thirteen items (guidelines §14). Follow-up:
  derive the local gate list from `just validate` / `ci.yml` mechanically
  instead of by memory.
- **Owner / target**: fenix; `justfile` `validate` target as the single
  source.

### F6 — Stack mechanics (merge-forward and landing)
- **Ids**: MERGE-001, the BA.4/BA.5 fork on BA.3, BA.3 rewrite with children,
  `gh stack link` append/retarget, zero CI on stacked PRs, non-linear
  `gh stack merge` refusal, Phase AX trunk push during landing.
- **Pattern**: every stack operation that was "obvious" behaved differently
  from the assumption (link appends; stacked PRs get no CI; rewriting a
  parent orphans children; sequential merge-async rebases frozen layers).
- **Root cause**: merge-forward process failure; missing written guideline.
- **Classification**: `merge_forward_process_improvement`.
- **Action**: `docs/development/gh-stack-guidelines.md` (this stack's
  landing checklist §5).
- **Owner / target**: fenix; `.claude/skills/codex-orchestration/SKILL.md`
  links the guideline.

### F7 — Planning process
- **Ids**: plan v2 PR #1398 (40 rounds), design-drift sweep findings ruled
  "unauthorized scope expansion", BA5-QA2-101 / BA3-QA1-009 / BA4-QA2-003
  (rejected by design citation).
- **Pattern**: open-ended reviewer sweeps queued serially behind one fixer;
  findings that add rules the design does not name.
- **Root cause**: weak sprint planning process, not weak requirements.
- **Classification**: `planning_process_improvement`,
  `no_systemic_followup` for the rejected findings (the design-authority
  rebuttal worked as intended).
- **Action**: docs → parallel scope + critical review on one head → one
  blocker round → QA approval → start; one hour. Reviewers get closed
  scopes.
- **Owner / target**: fenix; `.claude/skills/plan-hardening/` round cap.

### F8 — Orchestration incidents (no code defect)
- team-lead re-dispatched an acked, in-progress task (verify branch pushes
  and `atm read --from` before re-sending); daemon reminder replays of
  read+acked messages (1.5.14 host, BA.2 schema) are noise; reviewer
  subagents fail closed on prose input (JSON envelopes required;
  `critical-plan-reviewer` is plan-hardening-only).
- **Classification**: `qa_process_improvement`.
- **Action**: recorded in orchestration memory; reviewer dispatch uses the
  agents' JSON contracts.

## 4. What the phase got right

- Design doc as plan authority: every rejected finding was rebutted with a
  citation, none re-opened.
- One consolidated cleanup layer with one QA pass replaced per-branch fix
  rounds the moment Rand ruled it; the last three layers froze within an
  hour of each other.
- Frozen layers were never touched; the top layer's tree is exactly what
  landed, with every lower head an ancestor.
- The BA.3 merge-forward exposed real fixture staleness and was diagnosed
  per test against the doc instead of being force-fitted.
- ADR-061 governance held: MAJOR approved and recorded before code, two
  additive HTTP minors ledgered, Herdr IPC untouched.

## 5. Phase-level outcome

<!-- PHASE_OUTCOME -->
**`integration_review_passed`** (quality-mgr, PR #1418 comment 5644355381,
2026-09-12): seven mandatory reviewers PASS, zero blocking findings at the
landing head, `arch-qa` `merge_ready: true`.

**But not closed.** The parallel hostile closure review found two blocking
defects behind green doc-named tests (CPR-001 terminal escalation holds only
on a single-lead roster; CPR-002 `already_closed` never reaches the caller)
and roughly twenty tests that cannot fail. The phase PR to `develop` stays
draft until every review-findings layer lands on `integrate/phase-ba`, the
consolidated QA on the stack top passes, and the CI-trigger PR is green on
the landing sha. Lesson recorded in §3 F1 and §6 row 2: a seven-reviewer
PASS is a necessary gate, not the critical review; the hostile pass reads
every test body against the design and is the one that found the defects
the phase existed to fix.

## 6. Systemic actions

| # | Action | Class | Owner | Target artifact |
|---|---|---|---|---|
| 1 | gh-stack guidelines + landing checklist | merge_forward_process_improvement | fenix | `docs/development/gh-stack-guidelines.md` |
| 2 | Acceptance reads test bodies; guard tests proven to fail | qa_process_improvement | fenix | guidelines §21 |
| 3 | Local gate list derived from `just validate`/CI, not memory | qa_process_improvement | fenix | `justfile`, guidelines §14 |
| 4 | Test doubles live in `testing.rs`; allowlist by public path | boundary_update | Rand (TOML), solar | `boundaries/atm-storage/pending-nudge-store.toml`, TOML-001 |
| 5 | Tests as physical `src/<mod>/tests/` submodules with `#![cfg(test)]`, counted as test scope | boundary_update | solar | `fix/phase-ba-review-3` (RBQA-F001); counter unchanged |
| 6 | `herdr_queue_wake.rs` production split | test_hardening (RULE-003 margin) | cipher | `fix/phase-ba-review-1` |
| 7 | Lint candidates: assertion-less test bodies; unbounded `spawn_blocking` | new_lint | follow-up issue | `.just/` |
| 8 | `STORAGE_SCHEMA_VERSION` constant (ADR-061 D1) | architecture_update (pre-existing) | future phase | `crates/atm-storage-rusqlite` |
| 9 | Planning: one-hour docs → parallel reviews → QA → start | planning_process_improvement | fenix | `.claude/skills/plan-hardening/` |
| 10 | Prerelease tagger dry-run performs the same tag-collision preflight as publish | new_lint (tooling) | solar | `.just/prerelease_tag.py` (RDY-001) |
| 11 | Readiness checklist cites the approved rollback contract, not an invented refusal | qa_process_improvement | fenix | readiness vars template (RDY-002) |

Decision rule applied: lint or static enforcement first (3, 5, 7), boundary
enforcement second (4), process last (1, 2, 9, 11).
