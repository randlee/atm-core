---
phase: AX
sprint: AX.2
title: Herdr renders the built-in nudge template
branch: feature/ax2-herdr-template-rendering
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ax2-herdr-template-rendering
integration_branch: integrate/phase-ax
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
dependency_relations:
  - related: AX.1
    relation: must_follow
    rationale: needs the Queue-family kinds for the pump path and shares send/hook.rs edits.
  - related: AX.4a
    relation: parallel_safe
    rationale: this sprint owns hook.rs, HerdrNudgeTarget, atm-herdr, the bootstrap selector, and herdr_queue_wake.rs; AX.4a owns the write pipeline, ack path, task storage, and CLI flags. No shared files, contracts, or artifacts.
  - related: AX.4b
    relation: must_follow
    rationale: AX.4b extends the pump tick this sprint changes to pass rendered text.
---

# AX.2 — Herdr renders the built-in nudge template

Make the Herdr sink consume the same rendered template the tmux and graft
sinks consume, and retire the fixed wake text.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [ ] D1 — `HerdrNudgeTarget` carries `rendered_nudge: String`
  (`crates/atm-core/src/boundary/mod.rs`, code contract C1). The Herdr
  branch of `build_built_in_dispatch` in `crates/atm-core/src/send/hook.rs`
  calls `render_built_in_nudge_for_dispatch` exactly as the tmux branch
  does and returns `None` on render failure the same way.
- [ ] D2 — Herdr emitter takes the text (`crates/atm-herdr/src/lib.rs`,
  code contract C2). `HERDR_WAKE_TEXT` and the test
  `prompt_text_is_fixed_and_non_empty` are deleted. Empty rendered text is
  rejected before spawning (ADR-058 D8 `empty_agent_prompt` stays an
  atm-core defect signal).
- [ ] D3 — callers pass the text through:
  `crates/atm-daemon-bootstrap/src/received_hook_selector.rs` (immediate
  steer, `HerdrNudgeTarget` arm);
  `crates/atm-http-runtime/src/herdr_queue_wake.rs` `emit_claim` (the
  pump rebuilds dispatch with `NudgeKind::Queue`, which after AX.1
  resolves a Queue-family or Task template);
  `crates/atm-http-runtime/src/storage_and_nudge_router.rs`
  `HerdrNudgeTarget` arm.
- [ ] D4 — PTY line-safety fixture test in `atm-herdr` asserting the
  emitter passes multi-line text through unmodified (no newline stripping
  or joining). Basis: live check on rand-m5 with herdr 0.8.2 on
  2026-09-05, a `codex` agent and a `claude-code` agent each received the
  six-line Delivery body as one submission.
- [ ] D5 — ADR-058 amendment
  (`docs/adr/ADR-058-herdr-local-steer-backend-contract.md`): D2 and D4
  replace "fixed prompt text" with "the rendered built-in nudge template
  resolved for the recipient team and kind"; argv shape unchanged; the
  line-safety rule from D4 recorded; dated history entry.
- [ ] D6 — tests listed under Required validation.

### Paths to delete

None (two symbols deleted inside `crates/atm-herdr/src/lib.rs`).

## Code contracts

### C1 — target carries rendered text

```rust
// crates/atm-core/src/boundary/mod.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct HerdrNudgeTarget {
    pub session: Option<crate::HerdrSession>,
    pub rendered_nudge: String,
}
```

### C2 — emitter signature

```rust
// crates/atm-herdr/src/lib.rs
fn prompt_args(agent: &AgentName, session: Option<&HerdrSession>, text: &str) -> Vec<String>;
// ["agent", "prompt", <agent>, <text>]  plus ["--session", <s>] when session is Some.
// text.trim().is_empty() => Err(empty_agent_prompt) before spawn.
```

The public emitter entry point (`emit_received_message` or its current
name) gains the same `text: &str` parameter; no other argv change.

### Unchanged surfaces

`LocalTmuxNudgeTarget`; `render_built_in_nudge_for_dispatch`; ADR-058
D1, D3, D5–D8; the `herdr agent rename` identity rule.

## Acceptance criteria

1. `grep -rn HERDR_WAKE_TEXT crates docs` returns nothing.
2. A Herdr-backed member's prompt text equals the tmux-backed render for
   the same `PostSendHookEvent`.
3. Pump nudges carry a Queue-family or Task body (no `<when>`).
4. ADR-058 amended with a dated history entry; `just validate` green.

## Required validation

- Unit (`crates/atm-core/src/send/hook.rs` tests): render one
  `PostSendHookEvent` through the tmux and Herdr dispatch builders;
  assert identical `rendered_nudge`.
- `crates/atm-core/tests/nudge_mode.rs`: Herdr member, `atm send` then
  `atm queue`; assert the emitted prompt text for each equals the expected
  Delivery / Queue default render.
- `crates/atm-herdr` process tests updated for the parameterised prompt;
  the D4 fixture test passes a six-line body and asserts argv[3] is
  byte-identical.
- `crates/atm-http-runtime/src/herdr_queue_wake.rs` tests: `ac01`–`ac06`
  still pass with the recording emitter asserting the rendered text.
- `just validate`; quality-mgr Final Quality Report on the PR; `arch-qa`
  review of the ADR amendment.

## Out of scope

Coalescing of rapid steers; watermark; re-nudge for non-task immediate
sends; doctor mixed-backend finding; the splice-into-typing limitation
(PTY cursor position), tracked on #1173.
