# BA.2 — Nudge title metadata (salvage evaluation)

| Field | Value |
| --- | --- |
| Wave | 1 |
| Branch | `feature/ba2-nudge-title-salvage` |
| Base | `integrate/phase-ba` |
| Stack | not stacked — independent PR |
| Dependency (superseded, see below) | `parallel_safe` with BA.1, BA.4, BA.7 |
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

Combined scope, enumerated from the actual commits — **`fc81d97cc` is 32
files, not the ~14 an earlier draft of this doc listed** (PLAN-SCOPE-007, and
worse than reported):

*Code:* `crates/atm-core/src/boundary/mod.rs`, `graft.rs`, `nudge_dispatch.rs`,
`send/hook.rs`, `send/nudge_template.rs`, `send/tests.rs`,
`crates/atm-core/tests/nudge_mode.rs`, `tests/task_reminder_dispatch.rs`,
`crates/atm-http-runtime/src/storage_and_nudge_router.rs`,
`received_hook_selector.rs`, `crates/atm/src/commands/internal_nudge.rs`,
`crates/atm-graft/src/nudge_sink.rs`, `runtime/mod.rs`,
`examples/smoke_same_host.rs`, `crates/atm-graft-python/src/lib.rs`

*Boundary manifests:* `boundaries/atm-core/message-received-hook-emitter.toml`,
`boundaries/atm-graft/message-received-hook.toml`,
`boundaries/atm-herdr/herdr-process-adapter.toml`

*Docs:* ADR-052, ADR-054, `docs/architecture.md`, `docs/requirements.md`,
`docs/project-plan.md`, `docs/atm-core/{architecture,boundaries,requirements}.md`,
`docs/atm-graft/{architecture,boundaries,requirements}.md`,
`docs/atm-herdr/{architecture,boundaries,requirements}.md`,
`docs/atm-http-runtime/architecture.md`,
`docs/atm/{architecture,requirements}.md`, `docs/user-documents/hooks.md`,
`docs/examples/hooks/post-send-payload.json`,
`docs/plans/phase-az/sprint-AZ.1-task-nudge-contract.md`

*Scripts:* `scripts/atm-nudge.py`, `atm-nudge.sh`, `check-nudge-taxonomy.py`,
`test_atm_nudge.py`

Three consequences the sprint must handle before writing any code.

### Collision with BA.4 — PLAN-SCOPE-002

`crates/atm-http-runtime/src/storage_and_nudge_router.rs` is touched by **both**
salvage commits and independently by BA.4 D5a, which adds the dispatch-time
fence at `:644-692`. Both sprints are in wave 1. They are therefore **not**
`parallel_safe` on that file and the phase plan has been corrected.

Ordering: **BA.2 goes first.** Its edits to that file are small (27 and 2
lines) and mechanical; BA.4's fence is the substantive change and should be
written on top of the salvaged state, not rebased under it. BA.4 must merge
forward from BA.2 before touching the file.

### Boundary manifests need a written ruling — standing constraint

`fc81d97cc` edits three `boundaries/*.toml` manifests. Devs may not edit
`boundaries/*.toml` without a written ruling. **Obtain the ruling before the
cherry-pick, or drop those three files from the salvage and record what is
lost.** Do not carry them across silently because they arrived inside a
cherry-pick.

### Do not resurrect the phase-AZ sprint doc

`fc81d97cc` modifies `docs/plans/phase-az/sprint-AZ.1-task-nudge-contract.md`.
Phase AZ is retired. **Drop that file from the cherry-pick.** Likewise, check
`docs/project-plan.md` and `docs/requirements.md` hunks for AZ-phase language
before taking them; take only what is true of phase BA.

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
