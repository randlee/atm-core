---
id: AD.10
title: Directory Metadata And Doctor Contract Cleanup
status: planned
branch: feature/pAD-s10-directory-metadata-and-doctor-contract-cleanup
worktree: ../atm-core-worktrees/feature/pAD-s10-directory-metadata-and-doctor-contract-cleanup
target: integrate/phase-AD
---

# Sprint AD.10 — Directory Metadata And Doctor Contract Cleanup

## Goal

- close directory metadata ownership and doctor/member terminology after the
  accepted CLI repair path exists

## Hard Dependencies

- `AD.9` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`

## Exact Targets

- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm-core/src/schema/agent_member.rs`
- `crates/atm-core/src/boundary/store.rs`
- `crates/atm/src/commands/members.rs`
- `crates/atm/src/output.rs`
- team startup / rmux guidance touched by member directory metadata
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`

## Interfaces To Add Or Modify

```rust
pub struct AgentMember {
    pub home_dir: PathBuf,
    pub tmux_pane_id: Option<PaneId>,
    // legacy durable `cwd` is removed from the accepted canonical member shape
}
```

```rust
// startup-only context:
log::info!(launch_cwd = %startup_cwd.display(), "atm process started");
```

- canonical durable member directory metadata is `home_dir` only
- `live_cwd` is runtime-observed state only and must not be persisted on the
  canonical roster row
- `launch_cwd` is startup log context only and must not be persisted on the
  canonical roster row or runtime roster row
- modify doctor/member output so it uses `home_dir`, `live_cwd`, and
  `launch_cwd` consistently and never reports bare `cwd`

## Obsolescence Instructions

- any accepted canonical roster field named `cwd` becomes obsolete in this
  sprint
- any accepted doctor/member output that uses bare `cwd` without distinguishing
  `home_dir` or `live_cwd` becomes obsolete in this sprint
- any retained startup metadata path that persists `launch_cwd` beyond logging
  becomes obsolete in this sprint
- if a compatibility projection must carry a temporary derived field for a
  short period, mark it `Phase AD obsolete: derived compatibility field only`,
  forbid new production reads from it, and delete it once downstream
  compatibility is cleared

## Deliverables

- canonical member schema uses durable `home_dir` instead of durable `cwd`
- `live_cwd` is runtime-only state and is not persisted as canonical member
  metadata
- `launch_cwd` is log-only startup context and is not persisted
- doctor and member output use `home_dir`, `live_cwd`, and `launch_cwd`
  consistently without bare `cwd`
- no new directory-state coordinator or compatibility-only directory struct is
  introduced where direct roster/runtime fields are sufficient

## Required Work

- remove durable canonical `cwd` usage from member schema and member output
- tighten doctor/member projection terminology around `home_dir` and
  `live_cwd`
- reduce `launch_cwd` to startup logging only
- remove or obsolete compatibility-only directory fields or helpers that no
  longer carry accepted runtime meaning
- update startup/operator guidance so the accepted directory contract is clear

## This Sprint Does Not Close

- caller identity ownership
- post-send emitter contract
- final readiness

## Acceptance Criteria

- no accepted canonical member shape persists bare `cwd`
- doctor and `atm members` output use `home_dir` for durable member location
- any surfaced `live_cwd` is explicitly runtime-only and not persisted
- no accepted path persists `launch_cwd` beyond startup logging
- no accepted AD doc uses bare `cwd` ambiguously where `home_dir` or
  `live_cwd` is intended
- no new directory-state coordinator or persistence-only compatibility struct
  is introduced where an existing roster row or runtime roster field is
  sufficient

## Required Validation

- targeted doctor/member-output/schema tests
- `! rg -n "pub cwd:" crates/atm-core/src/schema/agent_member.rs`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`
