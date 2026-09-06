---
phase: AY
sprint: AY.1
title: Herdr audit, version table, requirements, and ADR text
branch: feature/ay1-herdr-audit-docs
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay1-herdr-audit-docs
integration_branch: integrate/phase-ay
status: draft
recommended_agent: Cipher-311d
recommended_model: fast
execution_track: docs
parallel_with: [AY.2]
stack_parent: none
pr_target: integrate/phase-ay
dependency_relations:
  - prerequisite: AY.1
    dependent: AY.2
    relation: parallel_safe
    rationale: AY.1 owns only audit, version, requirements, ADR, architecture, and preserved AQ-history documents; AY.2 owns only atm-herdr Rust and fixture paths, so their file sets, public contracts, and artifacts do not intersect.
  - prerequisite: AY.1
    dependent: AY.8
    relation: must_follow
    rationale: AY.8 implements the direct socket/pipe transport against the protocol, compatibility ledger, and platform ruling established here.
---

# AY.1 — Herdr audit, version table, requirements, and ADR text

Establish the normative cross-platform and supported-version contract before
direct socket work starts. This is a documentation-only sprint. It removes the
unapproved Windows scope exclusion, records the Herdr compatibility surface,
and aligns requirements, ADRs, architecture text, and preserved AQ history.

## Delivery topology and `/gh-stack`

AY.1 is a standalone branch from `integrate/phase-ay` and is intentionally not
part of a PR stack. It is parallel-safe with AY.2 because their file ownership
does not intersect. AY.8 is dispatched only after AY.1 has merged into the
integration branch; AY.1 is not used as one parent of a multi-parent stack.

Executors must use the `/gh-stack` skill for Phase AY stacks where a linear
dependency exists. For this standalone sprint, verify the branch and PR base
without creating a stack:

```bash
git merge-base --is-ancestor a7aebefb8 integrate/phase-ay
gh pr view --json headRefName,baseRefName,state
```

All commands are noninteractive. The phase policy uses `gh stack link` for
the external linear stack and `gh pr view --json` to verify bases, but forbids `gh stack rebase`,
`gh stack sync`, and `gh stack merge`; Phase AY uses merge commits and no
force-pushes.

## Preconditions

- P-A — `integrate/phase-ay` is cut from the Phase AX integration branch after
  AX.6, and contains develop merge `a7aebefb8` (PR #1218). The merge-base
  command above exits zero.
- P-B — the Phase AY plan has dated approval from Rand.
- The branch is created from the current `integrate/phase-ay`, not from another
  sprint branch.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or shape-only
completion fails the sprint.

- [ ] D1 — Add `docs/atm-herdr/windows-process-audit.md`. It has one row per
  audited behaviour, with columns for expected behaviour, Herdr source
  file/line at v0.8.0, v0.8.2, and `3a822e81`, observation, and verdict (`no
  action`, `production fix`, or `upstream request`). It includes the six ATM
  operations, the v0.8.0-to-v0.8.2 and v0.8.2-to-`3a822e81` drift, and the
  error-code differences. Windows-observed cells are explicitly marked
  `AY.7` or `release readiness`; this sprint must not claim recorded
  Windows Herdr evidence beyond what the repository holds (Rand's manual
  1.5.0 CLI self-send is context, not a recorded matrix).
- [ ] D2 — Add `docs/atm-herdr/herdr-versions.md` as the compatibility ledger.
  It has one row per supported Herdr release beginning at 0.8.0, records
  `PROTOCOL_VERSION`, argv and JSON shapes for the six operations, material
  drift, and the AY.2 recording-manifest path. It records v0.8.2 as the first
  official Windows release artifact, not as a compatibility floor. It also
  defines the drift-check procedure and the client-facing Herdr paths it scans.
- [ ] D3 — Update `docs/atm-herdr/requirements.md`: delete the Windows
  scope-out, add HR-PLAT-001 and HR-LIFE-001 exactly as Contract C1 requires,
  and amend HR-TEST-006 for the portable fake-Herdr fixture and per-version
  replay.
- [ ] D4 — Append a dated amendment to
  `docs/adr/ADR-058-herdr-local-steer-backend-contract.md`: D3 specifies UDS on
  macOS/Linux and a named pipe on Windows; the former `d79fd746` / protocol 21
  pin is replaced by `HERDR_MINIMUM_VERSION` under ADR-061; the Windows
  scope-out is deleted; and AI.11 is clarified per Contract C2. Preserve the
  original decision history outside the amendment.
- [ ] D5 — Update `docs/architecture.md` and mark the Windows deferral in
  `docs/plans/phase-aq/sprint-AQ2-6-herdr-steer-backend.md` superseded while
  preserving history. No boundary inventory or Rust file changes in this
  sprint.

### Paths that must not change

- Rust source and tests under `crates/**`.
- `boundaries/atm-herdr/herdr-process-adapter.toml` and
  `docs/atm-herdr/boundaries.md`; AY.3 owns the complete boundary inventory
  update after the parallel AY.1/AY.2 wave.
- Historical evidence artifacts.
- Any Phase AX contract except where the Phase AY umbrella plan expressly
  supersedes it.

### Paths to delete

None.

## Contracts

### C1 — normative requirements

```text
HR-PLAT-001: ATM exposes the same Herdr command set, typed errors, and breaker
semantics on macOS, Linux, and Windows. UDS versus named-pipe transport is an
implementation detail and MUST NOT create a platform-specific feature set.

HR-LIFE-001: The ATM daemon never depends on Herdr for startup or readiness,
never starts or stops Herdr, and does not restart for a Herdr upgrade. Missing,
late, unreachable, or crashed Herdr is reported as a bounded per-call failure
on the Herdr harness while ATM messaging, tmux, Hermes, and doctor remain up.
```

These requirements preserve the Phase AY rulings: ATM is a client of the
per-user Herdr singleton, there is no daemon startup gate, and endpoint/binary
configuration is explicit rather than inferred from the environment.

### C2 — ADR-058 amendment

The amendment states all of the following without changing their meaning:

- the direct client uses UDS on macOS/Linux and Herdr's named pipe on Windows;
- `HERDR_MINIMUM_VERSION` from `crates/atm-herdr` and ADR-061 is the floor, and
  one ATM build supports every version at or above it;
- new capabilities are additive and runtime-detected or version-gated;
- parsers key on stable codes and tolerate unknown fields, never message text;
- the AI.11 retired-listener ban governs ATM's own IPC listener. AY.8 may exempt
  only `crates/atm-herdr/src/transport_socket.rs` for the Herdr client, while
  `named_pipe` / `NamedPipe` remain banned everywhere else under `crates/`.

### C3 — drift ledger procedure

Each sprint-start and phase-end drift review compares the last recorded Herdr
revision with the current reference checkout over:

```text
src/api/ src/cli/agent.rs src/cli/notification.rs src/cli/spec.rs
src/cli.rs src/cli/protocol_guard.rs src/ipc.rs src/session.rs
src/integration/env.rs src/protocol/wire.rs distribution/
```

Results are appended to `docs/atm-herdr/herdr-versions.md`. A new recording set
is required only when the diff changes one of ATM's six operations.

## Required work

1. Verify the cited Herdr tags/revision against the maintained reference
   checkout and record file/line provenance in the audit, without copying
   mutable message text into a runtime contract.
2. Reconcile all three pre-existing Windows exclusions named by the umbrella
   plan, preserving historical text where the deliverable says “superseded.”
3. Make the version ledger point to AY.2 manifests rather than duplicating
   fixture bodies.
4. Check that exactly one ADR-061 filename exists after the Phase AX ADR-number
   collision is resolved.

## Acceptance criteria

1. Every file named in D1–D5 exists or is updated with all named sections; the
   audit's Windows observation cells say `AY.7`, not “pass” or equivalent.
2. `grep -n "out of scope" docs/atm-herdr/requirements.md docs/adr/ADR-058*`
   returns no Windows scope exclusion.
3. Req-QA can enumerate HR-PLAT-001 and HR-LIFE-001 directly from the
   requirements, and each preserves Contract C1.
4. Schema review finds ADR-061 cited, the minimum fixed at 0.8.0 unless a newer
   dated Rand ruling exists, and no newest-Herdr assumption.
5. `ls docs/adr/ADR-061-*.md | wc -l` prints `1`.
6. `git diff --name-only` contains neither boundary-inventory file; those files
   and their public-surface pins are one production-complete AY.3 deliverable.
7. The sprint meets the common phase merge gate: zero blocking, important, or
   in-scope minor findings; quality-manager PASS on the PR; relevant CI green;
   no flaky-test allowance; frozen files untouched without a written ruling.

## Required validation

- `just lint spell`
- `just lint adr-index`
- `git diff --name-only integrate/phase-ay...HEAD` confirms a docs-only diff,
  no Rust source change, and no AY.2-owned boundary-inventory path.
- Schema-reviewer review of the version ledger and ADR-061 reference.

This docs-only PR does not require a full CI cycle under the repository rule;
the listed focused validation and any mandatory PR checks must pass.

## Non-closure and out of scope

- No Rust, fixture, process, installer, transport, daemon, or composition-root
  implementation.
- No live Windows observation or evidence campaign; AY.7 fills the audit
  columns from Windows CI artifacts, and the live matrix is release
  readiness (ruling 5).
- No AY.2 fixture bytes in docs; this sprint records their manifest paths.
- No public boundary inventory update; AY.3 owns both boundary files and their
  matching public-item pins.
- No change to the legacy synchronous daemon.
