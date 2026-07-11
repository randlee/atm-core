---
id: AD.25
title: Built-In Nudge Override Lifecycle And Reset Semantics
status: complete
branch: feature/pAD-s25-post-send-hook-emitter-live-wiring
worktree: ../atm-core-worktrees/feature/pAD-s25-post-send-hook-emitter-live-wiring
target: integrate/phase-AD
---

# Sprint AD.25 — Built-In Nudge Override Lifecycle And Reset Semantics

## Goal

- replace the hidden empty-string disable trap with explicit team-owned
  override, disable, and reset-to-default semantics for built-in nudge
  templates

## Hard Dependencies

- accepted `AD.22` baseline already merged into `integrate/phase-AD` by
  `PR #490` / merge commit `477c3cef`; this sprint depends on that accepted
  baseline rather than on changing `sprint-AD22.md` frontmatter
- `docs/plans/phase-AD/plan-phase-AD.md`
- review provenance:
  - ATM message `01KX1P4D0SEZXWW90VW2F7FF27` from `quality-mgr`,
    `2026-07-08`, subject `PHASE-AD-END-QA FINAL VERDICT`
  - ATM message `01KX1MTJE596JE8SC2766V0Q10` from `arch-ctm`,
    `2026-07-08`, subject `PHASE-AD-END-REVIEW complete`

## Exact Targets

- `boundaries/atm-storage/nudge-template-override-store.toml`
- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/team_admin.rs`
- `crates/atm/src/commands/teams.rs`
- `crates/atm/src/output.rs`
- `crates/atm/src/commands/internal_nudge.rs`
- `crates/atm-storage-rusqlite/src/nudge_template_override_store.rs`
- `crates/atm-storage-rusqlite/src/shared_db.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm-rusqlite/requirements.md`
- `docs/atm-rusqlite/architecture.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD25.md`

## Interfaces To Add Or Modify

Postmerge ownership note:

- the canonical machine-readable boundary record for
  `NudgeTemplateOverrideStore` now lives at
  `boundaries/atm-storage/nudge-template-override-store.toml`
- any follow-up fix or review work against this sprint should treat the old
  `atm-core` boundary file as retired historical context only

The override-store contract after this sprint is explicit about all four
operator-visible states:

```rust
pub enum TeamNudgeTemplateOverrideMode {
    Override { template_body: String },
    Disabled,
}

pub struct TeamNudgeTemplateOverrideRow {
    pub team_name: TeamName,
    pub kind: BuiltInNudgeTemplateKind,
    pub mode: TeamNudgeTemplateOverrideMode,
    pub updated_at: OffsetDateTime,
}

pub trait NudgeTemplateOverrideStore: sealed::Sealed + Send + Sync {
    fn load_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
    ) -> Result<Option<TeamNudgeTemplateOverrideRow>, AtmError>;

    fn save_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
        template_body: &str,
    ) -> Result<TeamNudgeTemplateOverrideRow, AtmError>;

    fn disable_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
    ) -> Result<TeamNudgeTemplateOverrideRow, AtmError>;

    fn clear_template_override(
        &self,
        team: &TeamName,
        kind: BuiltInNudgeTemplateKind,
    ) -> Result<bool, AtmError>;
}
```

The accepted operator model after this sprint is:

- no stored row means product default
- `save_template_override(...)` requires a non-empty body and means explicit
  override
- `disable_template_override(...)` means explicit no-nudge for that template
  kind
- `clear_template_override(...)` removes the row and restores product default
- empty-string template bodies are invalid at the CLI boundary, invalid in
  `team_admin`, and invalid at the store contract

The accepted CLI surface after this sprint is:

- `atm teams set-nudge-template --team <team> --kind <kind> --template-body <non-empty>`
- `atm teams disable-nudge-template --team <team> --kind <kind>`
- `atm teams clear-nudge-template --team <team> --kind <kind>`

Empty-body rejection is one shared contract, not three competing ones:

- stable variant: `EmptyNudgeTemplateBody`
- stable code: `ATM_NUDGE_TEMPLATE_BODY_EMPTY`
- emitted by:
  - CLI argument validation for `atm teams set-nudge-template`
  - `team_admin` request validation before store mutation
  - store-side defensive validation if a caller bypasses the earlier layers
- recovery: provide a non-empty template body, or use the explicit disable or
  clear/reset command instead of an empty string

## Paths To Delete

- any interpretation of `template_body == ""` as an implicit disable signal
- any operator guidance that requires raw SQLite editing to restore product
  defaults
- any doc wording that describes only override-or-default while the runtime
  still supports a hidden disable state

## Deliverables

- the override-store contract exposes explicit override, disable, and clear
  operations
- the persisted row shape distinguishes disabled from overridden without using
  empty strings as control flow
- CLI/admin surfaces expose explicit set/disable/clear commands or equivalent
  command structure with the same semantics
- `atm internal-nudge` resolves rows as:
  - no row => product default
  - disabled row => no emission
  - override row => explicit replacement body
- docs define the full lifecycle and the precedence rules with no hidden
  fourth state
- all three enforcement points reuse the same
  `EmptyNudgeTemplateBody` / `ATM_NUDGE_TEMPLATE_BODY_EMPTY` contract rather
  than inventing layer-specific empty-body errors

## This Sprint Does Not Close

- post-send hook accounting bugs
- dead or bypassed `PostSendHookEmitter` / `GraftPostSendPort` seams
- upstream extraction of template resolution out of `atm internal-nudge`

## Acceptance Criteria

- targeted tests prove all four lifecycle states:
  - product default
  - explicit override
  - explicit disable
  - clear/reset to product default
- targeted tests prove empty-string override bodies are rejected before they can
  persist
- targeted tests prove CLI, `team_admin`, and store defensive validation all
  surface `EmptyNudgeTemplateBody` / `ATM_NUDGE_TEMPLATE_BODY_EMPTY`
- docs state unambiguously that reset-to-default is row deletion, not empty
  string persistence
- the accepted CLI/admin workflow no longer requires direct database mutation
  to undo a template override

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted override-lifecycle regression tests covering set, disable, clear,
  and reset-to-default
- `git diff --check`
