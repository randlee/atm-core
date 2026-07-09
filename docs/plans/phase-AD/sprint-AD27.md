---
id: AD.27
title: Upstream Built-In Template Resolution Extraction
status: planned
branch: feature/pAD-s27-upstream-built-in-template-resolution
worktree: ../atm-core-worktrees/feature/pAD-s27-upstream-built-in-template-resolution
target: integrate/phase-AD
---

# Sprint AD.27 — Upstream Built-In Template Resolution Extraction

## Goal

- move built-in template resolution fully upstream of `atm internal-nudge` so
  the retained helper only renders and delivers a pre-resolved template choice

## Hard Dependencies

- `AD.26` complete
- `docs/plans/phase-AD/plan-phase-AD.md`

`AD.26` is a functional dependency, not just merge order: upstream extraction
is only review-safe after the accepted line already uses live
`PostSendHookEmitter` / `GraftPostSendPort` seams and the remaining
`ADR-019-EXC-AD26-001` exception is isolated to template-resolution lookup.

## Exact Targets

- `crates/atm-core/src/send/hook.rs`
- `crates/atm/src/commands/internal_nudge.rs`
- `crates/atm/src/commands/mod.rs`
- `crates/atm-daemon-bootstrap/src/lib.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm-daemon/architecture.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD27.md`

## Interfaces To Add Or Modify

The retained internal helper must consume a resolved template payload rather
than reopening runtime composition:

```rust
pub struct ResolvedBuiltInNudgeTemplate {
    pub kind: BuiltInNudgeTemplateKind,
    pub body: Option<String>,
}

pub struct InternalNudgeEnvelope {
    pub event: PostSendHookEvent,
    pub sink_target: BuiltInNudgeSinkTarget,
    pub template: ResolvedBuiltInNudgeTemplate,
}
```

Required ownership after this sprint:

- override/default/disabled resolution happens before `atm internal-nudge`
  runs or before its renderer helpers are called in-process
- `crates/atm/src/commands/internal_nudge.rs` does not import
  `with_default_nudge_template_override_store`
- `atm internal-nudge` receives the exact template body to render, including
  explicit disabled state as `body: None`
- the helper still owns:
  - fixed placeholder substitution
  - local tmux rendering/delivery details that remain in its scope
  - any thin compatibility CLI parsing required for the hidden command
- the helper no longer owns:
  - SQLite/bootstrap lookup
  - team-row resolution
  - override/default precedence decisions
- `AD.27` closes the named interim exception `ADR-019-EXC-AD26-001` by moving
  the last built-in override lookup upstream of `PostSendHookEmitter`

## Paths To Delete

- `with_default_nudge_template_override_store` import and lookup inside
  `crates/atm/src/commands/internal_nudge.rs`
- any env-payload or helper path that forces the renderer to rediscover team
  override state after the caller already knows the selected template kind
- any doc wording that claims upstream resolution while the renderer still
  reopens runtime composition itself

## Deliverables

- template override resolution is upstream of the renderer on the accepted
  built-in nudge path
- `atm internal-nudge` becomes a pure render/deliver helper over resolved input
- docs state clearly which layer owns precedence and which layer only renders
- bootstrap composition seams removed from `internal_nudge.rs` are not
  reintroduced through another hidden helper
- `ADR-019-EXC-AD26-001` is removed from the accepted line rather than carried
  forward as a permanent caveat

## This Sprint Does Not Close

- explicit lifecycle semantics for override rows
- post-send boundary wiring and accounting
- the `atm-graft` timing race
- end-to-end smoke/service-hardening coverage

## Acceptance Criteria

- `rg 'with_default_nudge_template_override_store' crates/atm/src/commands/internal_nudge.rs`
  returns no matches
- targeted tests prove:
  - explicit override body reaches the renderer without secondary lookup
  - explicit disabled state reaches the renderer without secondary lookup
  - no-row default behavior is selected upstream and delivered as explicit
    resolved template input
- docs describe the upstream resolution seam and the retained renderer scope
  consistently

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted internal-nudge resolution extraction regression tests
- `git diff --check`
