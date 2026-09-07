---
id: HERMES-GRAFT-COLIMA-R1
phase: AQ-follow-up
title: Hermes agent atm-graft fork review and Colima integration coordination
status: in-progress
branch: docs/hermes-graft-colima-runbook
worktree: /Users/randlee/Documents/github/atm-core-worktrees/docs/hermes-graft-colima-runbook
integration_branch: develop
pr_target: develop
target: develop
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: external-integration
parallel_with: [P3-runbook]
---

# HERMES-GRAFT-COLIMA-R1 — Hermes agent atm-graft review and Colima integration

Review the complete frozen atm-graft integration delta in
`randlee/hermes-agent`, coordinate the isolated Colima integration matrix
against atm-core `prerelease/v1.5.3`, and leave a repeatable runbook that a
lower-cost coordinator can execute next time.

The Phase AQ graft contracts under `docs/plans/phase-aq/` and the
`randlee/atm-hermes-testbed` README are the background authorities. If this
tracking document conflicts with either source, stop and report the mismatch to
`fenix@atm-dev` before continuing.

This follow-up lives under Phase AQ because AQ1.9 established the
`hermes-atm` wheel-verification and graft-boundary precedent. The work here
closes that same boundary at prerelease readiness; it does not introduce a new
product architecture phase or relocate Hermes ownership into atm-core.

## Goal

- Prove the frozen Hermes injection seam and the tagged atm-core 1.5.3 artifact
  pair work together in the isolated Colima testbed, while leaving a durable
  readiness record that another coordinator can execute without rediscovery.

## Hard Dependencies

- Loki must freeze the complete Hermes fork range and own fixes in that fork.
- atm-core artifacts must come from an annotated prerelease tag whose source
  and built wheel metadata admit the same `atm-graft` version.
- The testbed must provide a no-`sudo`, loopback-only fixture path with pinned
  peer authority and sanitized durable evidence.
- Any blocking P1 finding must be fixed and delta-reviewed before P2 starts.

## Dependency Relations

`must_follow` trigger: the preceding phase reports PASS and any new fork fix is
re-frozen before dependent work begins. `parallel_safe` means the work owns
non-intersecting artifacts and does not consume an unsettled result.

- P1 (`must_follow` P0 — review consumes the immutable fork range).
- P2 (`must_follow` P1 — image construction consumes the accepted fork head).
- P3 (`parallel_safe` with P0, P1, and P2 — it changes only this sprint record,
  the atm-core runbook, and the project-plan index; testbed operating steps
  remain Loki-owned).

The docs branch is independent from other open work and targets `develop`
directly. It is not a dependent PR, so `/gh-stack` is not used unless a later
dependency is introduced and documented before linking.

## Exact Targets

- `docs/plans/phase-aq/sprint-HERMES-GRAFT-COLIMA-R1.md`
- `docs/runbooks/hermes-graft-colima-integration.md`
- `docs/project-plan.md`
- Frozen external review range in `randlee/hermes-agent`.
- Versioned result and provenance artifacts in `randlee/atm-hermes-testbed`.

## Deliverables

1. P0 handshake acknowledged by Loki with the frozen fork branch/base/head,
   exact completion signal, testbed image/tag, artifacts and builders, and the
   agreed tier matrix.
2. P1 full-fork-delta review report posted on the Hermes fork work item. It has
   `SUMMARY`, numbered `FINDINGS` with severity/file:line/recommendation,
   `RISKS`, and `TEST-GAPS`, and covers every file in the named range.
3. P2 testbed results committed in `randlee/atm-hermes-testbed` as per-tier JSON
   with PASS/FAIL and a root cause for every failure. The matrix covers A–D,
   D7, the restart matrix, Tier E live graft-Hermes behavior, and the
   container-to-Mac leg only if P0 places it in scope.
4. P3 runbook at `docs/runbooks/hermes-graft-colima-integration.md`, written for
   a lower-cost coordinator with prerequisites, exact commands, addressing,
   tier expectations, issue/recovery records, escalation rules, and report
   templates.
5. Final fenced-JSON readiness report to Fenix stating whole-run PASS/FAIL and
   whether integration unblocks Rand's publish decision.

## Required Work

- Complete and acknowledge the P0 handshake; verify tag, source constraint,
  wheel METADATA, artifact digests, platform, authority, resource allocation,
  and test matrix.
- Review every line and commit in the frozen Hermes range; post structured
  findings on the Hermes work item and verify each fix from a newly frozen SHA.
- Release the Colima hold only after P1 passes, then coordinate one native
  aarch64 build and the agreed functional/restart/live matrix without flaky
  green reruns.
- Update the runbook issue table at discovery time and submit this independent
  docs branch to quality-manager review.

## Explicit Code Samples

The reviewed Hermes boundary must retain this keyword-only public shape:

```python
async def inject_internal_message(
    *,
    profile: str,
    platform: Platform,
    chat_id: str,
    text: str,
    notice_text: Optional[str] = None,
    mode: Literal["queue", "steer"] = "queue",
) -> None: ...
```

Every phase completion uses this machine-readable envelope:

```json
{"task_id":"HERMES-GRAFT-COLIMA-R1-1788742538","phase":"P0|P1|P2|P3","status":"pass|fail|blocked|in_progress","artifacts":[],"blockers":[]}
```

## This Sprint Does Not Close

- Publishing atm-core or any package/channel; Rand owns that decision.
- Patching atm-core product code; any required fix routes to Fenix for separate
  dispatch to `arch-ctm`.
- Cross-host execution when the P0 ruling leaves it outside the primary gate.
- Performance claims from QEMU or other emulated timing.
- Remodeling or hardening the frozen synchronous ATM daemon.

## Acceptance Criteria

1. Loki acknowledges P0 and the immutable completion signal is recorded.
2. Review coverage equals the complete `base..head` delta, not a sample. Every
   blocking finding has a file:line and concrete fix, and is fixed or waived by
   Rand before P2 can pass.
3. P2 emits and commits one machine-readable result for every agreed tier. No
   flaky rerun is relabeled PASS; every intermittent failure receives a root
   cause.
4. QEMU timings, if QEMU is used, are diagnostics only and never benchmark
   evidence.
5. The run uses no `sudo`, no ssh initiated from rand-m4, no tmux control of a
   Hermes agent, and only loopback fixture daemons. Durable docs/results contain
   no message bodies, concrete participant addresses, tokens, secrets, or
   capability values.
6. Every phase report to Fenix is one ATM message containing a fenced JSON
   object with `task_id`, `phase`, `status`, `artifacts`, and `blockers`.
7. The runbook PR targets `develop`, receives quality-manager review, and has a
   0 blocking / 0 important / 0 in-scope minor merge gate.
8. This file's frontmatter is changed to `status: complete` only after P0–P3
   and the final readiness report are complete.

## Required Validation

- Mechanically verify the reviewed fork range and its full changed-file set.
- Run the fork's tests and static checks named by Loki for the frozen head.
- Run every agreed testbed tier from a fresh isolated container and validate
  every result JSON before committing it.
- Run the restart matrix without manual profile repair.
- Validate the docs branch with `git diff --check`, repository spelling/content
  checks, Markdown-link checks where available, and quality-manager QA.
- Verify the planning index and runbook paths exist before closeout.
