# BA.2 — Nudge title metadata (salvage evaluation)

| Field | Value |
| --- | --- |
| Wave | 1 |
| Branch | `feature/ba2-nudge-title-salvage` |
| Base | `integrate/phase-ba` |
| Stack | not stacked — independent PR |
| Dependency | `parallel_safe` with BA.1, BA.4, BA.7 |
| recommended_agent | Cipher-311d |
| recommended_model | fast |

## Goal

Recover the nudge title metadata work from phase AZ, which is general nudge
infrastructure rather than task lifecycle, and would otherwise be lost when AZ
is retired.

This sprint may legitimately close as **not salvaged**. See Non-closure.

## Candidate commits

Both from `origin/integrate/phase-az`. Neither has any added line referencing
`_v2`, `tasks_v`, `TaskLifecycle`, `AsyncTaskMutation`, `TaskOperationId`,
`assignment_attempt`, or `attention`.

| SHA | subject |
|---|---|
| `fc81d97cc` | `fix(nudge): dual-write bounded title metadata` |
| `4682bbbc0` | `fix(nudge): project persisted titles only` |

Combined scope: `crates/atm-core/src/boundary/mod.rs`, `graft.rs`,
`nudge_dispatch.rs`, `send/hook.rs`, `send/nudge_template.rs`,
`storage_and_nudge_router.rs`, `crates/atm/src/commands/internal_nudge.rs`,
`crates/atm-graft-python/src/lib.rs`, three boundary manifests under
`boundaries/`, plus `docs/adr/ADR-054-nudge-taxonomy-and-queue-mechanism.md`
and two other ADR/doc updates.

## Deliverables

1. Determine whether the title metadata is genuinely nudge-general or is in
   practice only populated for task nudges. Record the determination with
   evidence in the PR body. This is the sprint's first gate and it may
   terminate the sprint.
2. If general: cherry-pick both commits in order onto
   `integrate/phase-ba`, resolving against the retired AZ context — the
   boundary manifests and ADR-054 edits must land with the code, not after it.
3. Confirm the boundary manifests still match physical paths on disk.
   `lint_boundaries` resolves manifest Rust paths from file location, not Rust
   module resolution.

## Affected paths

As listed under **Candidate commits**. No file in `atm-storage*`,
`atm-http-runtime`, or `crates/atm/src/commands/task.rs`.

## Paths that must not change

- `crates/atm-storage*` — BA.1 and BA.3
- `crates/atm-http-runtime/src/herdr_*` — BA.4
- `.just/lint-config.toml`, `.just/allowlists/`, `boundaries/*.toml` beyond the
  three the commits already touch, `.just/lint_boundaries.py` — a written
  ruling is required to change any of these

## Acceptance criteria

1. The general-vs-task-only determination is stated in the PR body with the
   grep or test evidence that supports it.
2. If salvaged: `just test` and `just lint` green, boundary enforcement tests
   pass, and ADR-054 reflects the shipped behaviour.
3. If not salvaged: the PR is closed with the determination recorded, and the
   phase plan's salvage table is updated to strike both SHAs.

## Required validation

- `just test`, `just lint`
- `cargo test -p atm-architecture` (boundary enforcement)

## Non-closure

Closing this sprint as "not salvaged" is a valid, successful outcome provided
the determination is evidenced. It is **not** acceptable to salvage the code
and defer the ADR-054 and boundary-manifest updates: that would leave a
documented contract disagreeing with shipped behaviour.
