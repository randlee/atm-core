---
id: AE.6
title: Help Surfacing
status: planned
branch: feature/pAE-s6-help-surfacing
worktree: ../atm-core-worktrees/feature/pAE-s6-help-surfacing
target: integrate/phase-AE
---

# Sprint AE.6 — Help Surfacing

## Goal

Point concise `atm help` topic output at the installed long-form corpus.

## Hard Dependencies

- `AE.5` complete
- `docs/plans/phase-AE/plan-phase-AE.md`

## Exact Targets

- `crates/atm/src/commands/help.rs`
- `docs/atm/commands/help.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`

## Interfaces To Add Or Modify

The CLI-owned help surface needs one explicit installed-doc mapping seam:

```rust
pub struct HelpDocLink {
    pub topic: HelpTopic,
    pub relative_path: &'static str,
}

fn installed_doc_root() -> Option<PathBuf>;
fn doc_link_for_topic(topic: HelpTopic) -> Option<HelpDocLink>;
```

## Deliverables

- `atm help` topic output stays concise
- help topics that have long-form docs point to the installed corpus path
- `atm help <subcommand>` still starts with authoritative clap output
- no new help-only CLI commands or flags are introduced

## Acceptance Criteria

- help output never attempts to inline the full long-form hook or nudge manual
- doc links point to the installed tree, not repo-relative developer paths
- JSON help output carries the same installed-doc pointer data

## Required Validation

- `cargo test -p atm commands::help -- --nocapture`
- `git diff --check`
