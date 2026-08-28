---
status: complete-pending-m5-followup
branch: feature/aq-1-9-hermes-atm-wheel-verification
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/aq-1-9-hermes-atm-wheel-verification
---

# Sprint AQ1.9 — hermes-atm Wheel Bump + Live Verification

Status: restart-matrix harness in-tree, unit-tested · local loopback run
OPEN (blocked on this workstation by an ambient account-owned `atm-daemon`;
see `restart-matrix-local.pending.md`) · m5 live run OPEN (pending) · Branch:
`feature/aq-1-9-hermes-atm-wheel-verification` off `integrate/phase-aq` · PR
target: `integrate/phase-aq`
recommended_agent: Cipher-311d · recommended_model: fast

Final graft connection-model sprint (see AQ1.5). The Python surface never
touches the record file (verified: `hermes_atm/runtime.py` /
`native_tools.py` only import the `atm_graft` PyO3 binding), so this is a
dependency bump plus live proof that the Hermes workaround era is over —
coordinated with team-lead@atm-dev on M5, framed per standing convention as
an atm-graft API change notification, not a Python code change request.

> **Scope ruling (fenix, 2026-08-27):** the automated loopback restart-matrix
> tests belong to AQ1.7; this sprint owns the live m5 evidence run (harness +
> committed evidence), and may reuse AQ1.7's test fixtures.

## Deliverables

1. Rebuild/bump the `atm-graft-python` wheel against the AQ1.5–AQ1.8
   crates. **Changelog and version target (closes M6)**: no per-wheel
   changelog exists in this repo (verified: only the root `CHANGELOG.md`,
   workspace-wide, currently documents up through `1.4.3` while
   `Cargo.toml`'s workspace version was `1.4.4` before this sprint) — the entry lands
   in `CHANGELOG.md` under the next release heading, naming the
   registration cutover and ADR-056, matching the existing bullet style
   (e.g. "... (Phase AQ)"). The registration model change is internal to
   `atm-graft`'s Rust implementation; hermes-atm's Python surface only
   imports the `atm_graft` PyO3 binding and never touches the record file
   directly or indirectly, so this is a non-breaking **patch** release
   within the existing `1.4.x` line — the version target is the next patch
   release, not a minor/major bump. `crates/hermes-atm/pyproject.toml`'s
   existing pin, `atm-graft>=1.4,<1.5`, already covers any `1.4.x` patch
   and needs **no change**.
2. **FOLLOW-UP (AQ1.9-m5) — Live verification on m5** (the Hermes host): a
   real Hermes agent session sends/receives via graft across BOTH restart
   orders — daemon restarted under a live receiver, and receiver restarted
   under a live daemon — with zero manual steps (no profile reset).
   **Restart-matrix row for immediate same-host displacement (closes
   I10)**: a third scenario — receiver crash (SIGKILL, no clean
   unregister) followed by an immediate restart within
   `ACTIVE_LEASE_WINDOW` (15s) — is captured alongside the other two
   orders, showing the stale lease is displaced at the successor's
   bind-time registration (zero refresh ticks) rather than waiting out the
   window (AQ1.6 AC #5, backed by the AQ1.5 amendment removing the
   window-gated `AlreadyActive` rejection). The row is measured on product
   observables — `atm doctor --json` `graft_receivers` shows exactly one
   lease for the receiver at a new endpoint whose `registered_at` is at or
   before the successor's own `ready` event, plus successor delivery of the
   marker (`displaced_at_bind`); wall-clock recovery (`crash_recovery_ms`,
   `successor_spawn_to_ready_ms`, `lease_displaced_at_ms`) is recorded as
   diagnostic only. **Incident (docs/aq-closeout @ 9674f64b7, merge ref
   b78c041f1)**: the earlier harness failed the row on a one-tick wall-clock
   bound stamped before the successor's interpreter spawn; on the Windows
   clean runner the product displaced the lease at +211 ms while the bound
   tripped on the 933 ms CPython + `atm_graft` spawn. Committed records under
   `evidence/AQ1.9/` predate the fix and keep the old
   `within_one_refresh_tick` field; they are not rewritten. Transcript + `atm doctor --json` graft
   section captured as sprint evidence for all three rows. **Not executed
   this sprint**: m5 is unreachable from the execution network (see Scope
   ruling below); tracked as `AQ1.9-m5` for Phase AQ closeout (AQ6) or as
   soon as m5 is reachable, whichever comes first.
3. Notify M5 team-lead (ATM message) with the cutover summary and the
   evidence, and collect confirmation that the previously reported
   endpoint-decode failures / CLI-file workarounds are no longer needed.

## Evidence

What is committed under `docs/plans/phase-aq/evidence/AQ1.9/` is: the runner
(`scripts/phase-aq/run_hermes_atm_restart_matrix.py`) and its unit tests
(`scripts/phase-aq/test_run_hermes_atm_restart_matrix.py`), the clean-runner
Linux transcript, and two explicit pending stubs (`restart-matrix-local.pending.md`,
`restart-matrix-m5.pending.md`). Those pending stub files (not this doc)
remain the source of truth for the local-loopback and m5 rows until real
`restart-matrix-<host>.json` / `.md` evidence is produced on those hosts.

| Host | Status | Run | Evidence |
|---|---|---|---|
| clean-runner-linux | PASS 3/3 | run [33094805689](https://github.com/randlee/atm-core/actions/runs/33094805689), head `a2dc79e52` | `docs/plans/phase-aq/evidence/AQ1.9/restart-matrix-clean-runner-linux.json`, `restart-matrix-clean-runner-linux.md` |
| clean-runner-macos | PASS 3/3 | run [33110341894](https://github.com/randlee/atm-core/actions/runs/33110341894), head `585eff5e4` | `docs/plans/phase-aq/evidence/AQ1.9/restart-matrix-clean-runner-macos.json`, `restart-matrix-clean-runner-macos.md` |
| m5 | FOLLOW-UP (AQ1.9-m5) | not attempted (m5 unreachable) | `docs/plans/phase-aq/evidence/AQ1.9/restart-matrix-m5.pending.md` (stub); owner: Phase AQ closeout (AQ6); prerequisite: m5 reachable |

## Scope ruling (fenix, Phase AQ driver, 2026-08-27)

m5 is unreachable from the execution network (hostname does not resolve;
checked 2026-08-27T05:33Z and since) and no operator access was provided,
so the m5-dependent items — Deliverable #2 (live m5 restart matrix), the
AC1 clause requiring hermes-atm's own test suite on M5 with the 1.4.5
wheel, and AC3 (M5 team-lead confirmation with a recorded message id) —
move to a tracked follow-up named `AQ1.9-m5`, to be executed at Phase AQ
closeout (AQ6) or as soon as m5 is reachable, whichever comes first. Rand
may override this ruling.

## Acceptance criteria

1. Wheel builds green against the phase branch. **FOLLOW-UP (AQ1.9-m5):**
   hermes-atm's own test suite (as run on M5, with the 1.4.5 wheel) passes
   with the new wheel — not executed this sprint, m5 unreachable; see
   Scope ruling.
2. The restart-matrix live evidence shows delivery recovering
   automatically in both daemon/receiver restart orders **and** the
   crash-within-window row (I10) shows the stale lease displaced at the
   successor's bind-time registration (zero refresh ticks), measured on
   product observables (`displaced_at_bind`), with wall-clock recovery
   recorded as diagnostic — timestamps + message ids in the transcript for
   all three rows.
3. **FOLLOW-UP (AQ1.9-m5):** M5 team-lead confirmation recorded (message
   id in the sprint evidence notes) — not executed this sprint, m5
   unreachable; see Scope ruling. Never claimed PASS.

**Re-scoped merge gate**: this sprint's merge gate is satisfied by the
clean-runner Linux restart-matrix PASS (run 33094805689, head a2dc79e52)
and the clean-runner macOS restart-matrix (run 33110341894; see Evidence
table). AC1's M5 test-suite clause, AC2's m5 row (superseded by the
clean-runner matrices for merge purposes), and AC3 are excluded from the
merge gate and tracked as `AQ1.9-m5`.

## Non-closure / out of scope

- Any hermes-atm Python source change (none required).
- Hermes-side `/queue` routing (AQ2's Python-surface deliverable).

## Dependencies

- must_follow: AQ1.7 (cutover must be live before verification proves it).
  PR-completion trigger: AQ1.7 PR merges first.
- parallel_safe: AQ1.8 (disjoint files; see AQ1.8); AQ2.6, AQ2.7 (Herdr —
  disjoint files; 2026-08-26 reorder).
