---
phase: AY
sprint: AY.10
title: Live macOS and Windows Herdr socket proof and phase disposition
branch: feature/ay10-herdr-live-proof
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay10-herdr-live-proof
integration_branch: integrate/phase-ay
stack_parent: none
pr_target: integrate/phase-ay
status: draft
recommended_agent: none
recommended_model: n/a
execution_track: proof
parallel_with: []
dependency_relations:
  - prerequisite: AY.9
    dependent: AY.10
    relation: must_follow
    rationale: live proof is captured only from the merged integration head after socket default, CLI fallback, doctor projection, and all automated lifecycle gates are complete.
---

# AY.10 — Live macOS and Windows Herdr socket proof and phase disposition

Prove the merged Phase AY socket-default behavior with operator-captured,
byte-for-byte evidence on rand-m5 and FastPC4, then record the phase disposition.
This is a proof sprint: it changes evidence and disposition documents only.

## Dispatch and PR topology

Dispatch AY.10 only after AY.9 has merged into `integrate/phase-ay` and both
hosts can run the same integration-head build. Create
`feature/ay10-herdr-live-proof` from that integration head and target its PR to
`integrate/phase-ay`.

AY.10 is standalone and has no stack parent. Do not append it to the completed
implementation `/gh-stack`: this proof consumes the fully merged phase graph and has
no unmerged code dependency to review as a stacked diff. If the old stack must
be inspected, use only the skill's non-interactive form
`gh pr view feature/ay10-herdr-live-proof --json headRefName,baseRefName,state`;
no `gh stack` mutation is required for AY.10.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial, synthetic,
or shape-only completion fails the sprint.

- [ ] D1 — build and install the same `integrate/phase-ay` commit on rand-m5
  and FastPC4 using the repository's signed daemon-switch workflow. Record the
  atm build SHA, Herdr version, host, operator, and start/end timestamps before
  running any case. Confirm `atm doctor` is healthy and that the Herdr endpoint
  record reports `transport: socket` and the expected resolved endpoint.
- [ ] D2 — operators capture the D4 matrix byte-for-byte. Commit the index at
  `docs/plans/phase-ay/ay10-live-proof.md` and raw artifacts under
  `docs/plans/phase-ay/evidence/AY10/macos/` and
  `docs/plans/phase-ay/evidence/AY10/windows/`. Agents may format the index and
  validate files but never author, synthesize, or retype observations.
- [ ] D3 — for every matrix row, the index uses C1 and links the exact request,
  response, doctor, tasklist/process, or log artifact. Exclude secrets, config
  contents, unrelated message bodies, and terminal output outside the named
  test case.
- [ ] D4 — run on both platforms: prompt, wait, get, list, and notify round
  trips; endpoint stopped/absent with daemon still ready and tmux/hermes
  unaffected; breaker open and recovery; agent not found; agent blocked; slow
  call at the 5 s cap with no orphan; late Herdr start; Herdr update while
  nudges continue without daemon restart; socket-boundary structured logs;
  latency samples. Also run one cross-host nudge with timestamps captured at
  both ends. On Windows additionally capture the full named-pipe endpoint,
  `tasklist` before/after the timeout case, and an operator confirmation that
  no console window flashes.
- [ ] D5 — re-run `atm doctor --json` after the negative/recovery and update
  cases. The final capture shows the breaker closed, the new Herdr version
  after update, and every configured endpoint healthy; it contains no
  aggregate `herdr.state` or `herdr.remedy`.
- [ ] D6 — quality-mgr compares the proof index against D4 and every linked
  artifact before disposition. Missing, edited, cross-build, or operatorless
  evidence fails the sprint; there is no `not run` allowance for a required
  D4 case on either host.
- [ ] D7 — after D1–D6 pass, Rand records exactly one dated phase disposition
  in `docs/plans/phase-ay/phase-ay-plan.md` and `docs/project-plan.md` using
  C2. `Ship` closes AY.10. `Defer` or `Cancel` follows the phase-plan cleanup and
  supersession rules and does not falsely mark live socket parity shipped.

### Paths to delete

None in the `Ship` case. The umbrella phase plan owns the explicit cleanup list
for `Cancel`.

## Evidence contracts

### C1 — one authoritative proof row

Every case appears exactly once in `ay10-live-proof.md`:

```text
case_id | platform | host | build_sha | herdr_version | started_at |
finished_at | request_artifact | response_artifact | expected | observed |
pass/fail | operator
```

The two timestamps for the cross-host row are source timestamps from the
sender and receiver artifacts. Latency is calculated from those captured
values. Every artifact path is repository-relative and resolves on the branch.

### C2 — disposition line

The umbrella plan contains exactly one line in this form:

```text
Decision (Rand, YYYY-MM-DD): Ship|Defer <phase>|Cancel
```

The same decision and date are reflected in `docs/project-plan.md`. Mechanical
gate:

```sh
grep -E '^Decision \(Rand, [0-9]{4}-[0-9]{2}-[0-9]{2}\): (Ship|Defer [A-Z]+|Cancel)$' docs/plans/phase-ay/phase-ay-plan.md
```

## Required work

1. Build and install the identical merged AY.9 integration SHA on rand-m5 and
   FastPC4, then have the named operators capture every matrix case without an
   agent rewriting observed bytes.
2. Assemble the proof index strictly from those captures, verify artifact
   identity and redaction, and obtain quality-manager review with no `not run`
   case.
3. Record Rand's dated disposition in both plan surfaces only after the proof
   gate completes; any code defect returns to a new implementation sprint.

## Acceptance criteria

1. Both hosts ran the identical recorded integration SHA after AY.9 merged;
   their initial and final doctor captures identify the socket transport and
   expected Unix socket/full Windows pipe endpoint.
2. Every D4 case has a PASS row and resolves to byte-for-byte raw artifacts for
   the correct host, build, Herdr version, timestamps, and operator.
3. Negative cases prove the daemon remains ready, tmux/hermes remain usable,
   the breaker opens and recovers, the 5 s deadline leaves no orphan, and no
   automatic CLI fallback occurs.
4. The update case proves nudges continue across `herdr update` without a daemon
   restart; final doctor reports the new server version.
5. The cross-host row contains both endpoint timestamps and a calculated
   latency sample; Windows artifacts contain tasklist, full pipe name, and
   no-console-flash confirmation.
6. `cmp` confirms every committed raw artifact equals its operator capture;
   quality-mgr signs off on the matrix with 0 missing or `not run` cases.
7. C2 appears exactly once in the umbrella plan, matches
   `docs/project-plan.md`, and expresses the disposition Rand selected after
   reviewing the evidence.
8. The AY.10 PR changes only `docs/plans/phase-ay/ay10-live-proof.md`,
   `docs/plans/phase-ay/evidence/AY10/**`, the disposition line in the umbrella
   plan, and the matching project-plan status entry.

## Required validation

- Validate both installed builds with `atm doctor --json` before and after the
  matrix.
- Run every D4 command on rand-m5 and FastPC4 and preserve original exit status,
  stdout/stderr JSON, and relevant structured logs.
- `cmp` each repository artifact against the operator-owned capture before
  commit.
- Mechanically check C1 completeness, artifact existence, identical build SHAs,
  zero `not run` rows, and the C2 grep gate.
- `just validate` for documentation and repository gates.
- quality-mgr Final Quality Report: 0 blocking, 0 important, 0 minor in scope.

## Out of scope

- Any Rust, installer, boundary, configuration, or runtime change. Findings
  requiring code return to a new implementation sprint; AY.10 does not patch
  live failures in place.
- Removing the CLI fallback or adopting new Herdr capabilities.
- Inventing or editing operator evidence.
- Any patch, hardening, or remodeling of the legacy synchronous daemon.
