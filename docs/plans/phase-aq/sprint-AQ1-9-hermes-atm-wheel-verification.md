---
status: pending-live-run
branch: feature/aq-1-9-hermes-atm-wheel-verification
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/aq-1-9-hermes-atm-wheel-verification
---

# Sprint AQ1.9 — hermes-atm Wheel Bump + Live Verification

Status: local loopback complete · m5 live run pending · Branch:
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
2. **Live verification on m5** (the Hermes host): a real Hermes agent
   session sends/receives via graft across BOTH restart orders — daemon
   restarted under a live receiver, and receiver restarted under a live
   daemon — with zero manual steps (no profile reset). **Restart-matrix
   row for immediate same-host displacement (closes I10)**: a third
   scenario — receiver crash (SIGKILL, no clean unregister) followed by an
   immediate restart within `ACTIVE_LEASE_WINDOW` (15s) — is captured
   alongside the other two orders, showing the successor registers and
   delivers within one `GRAFT_LEASE_REFRESH_INTERVAL` tick rather than
   waiting out the window (AQ1.6 AC #5, backed by the AQ1.5 amendment
   removing the window-gated `AlreadyActive` rejection). Transcript +
   `atm doctor --json` graft section captured as sprint evidence for all
   three rows.
3. Notify M5 team-lead (ATM message) with the cutover summary and the
   evidence, and collect confirmation that the previously reported
   endpoint-decode failures / CLI-file workarounds are no longer needed.

## Evidence

The local loopback transcript and `atm doctor --json` captures are committed
under `docs/plans/phase-aq/evidence/AQ1.9/`. The m5 evidence slot remains
explicitly `PENDING-LIVE-RUN`; its exact single-command procedure is in
`restart-matrix-m5.pending.md` in that directory.

## Acceptance criteria

1. Wheel builds green against the phase branch; hermes-atm's own test
   suite (as run on M5) passes with the new wheel.
2. The restart-matrix live evidence shows delivery recovering
   automatically in both daemon/receiver restart orders **and** the
   crash-within-window row (I10) shows sub-tick recovery — timestamps +
   message ids in the transcript for all three rows.
3. M5 team-lead confirmation recorded (message id in the sprint evidence
   notes).

## Non-closure / out of scope

- Any hermes-atm Python source change (none required).
- Hermes-side `/queue` routing (AQ2's Python-surface deliverable).

## Dependencies

- must_follow: AQ1.7 (cutover must be live before verification proves it).
  PR-completion trigger: AQ1.7 PR merges first.
- parallel_safe: AQ1.8 (disjoint files; see AQ1.8); AQ2.6, AQ2.7 (Herdr —
  disjoint files; 2026-08-26 reorder).
