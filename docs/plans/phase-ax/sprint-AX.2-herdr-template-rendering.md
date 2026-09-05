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
execution_track: A
parallel_with: [AX.3]
dependency_relations:
  - prerequisite: AX.1
    dependent: AX.2
    relation: must_follow
    rationale: needs the Queue-family kinds for the pump path and shares crates/atm-core/src/send/hook.rs edits.
  - prerequisite: AX.2
    dependent: AX.3
    relation: parallel_safe
    rationale: no functional dependency; both add lines to crates/atm-core/src/boundary/mod.rs (HerdrNudgeTarget field here, TaskStore re-exports in AX.3), resolved by AX.3 merging integrate/phase-ax forward before its PR.
  - prerequisite: AX.2
    dependent: AX.5
    relation: must_follow
    rationale: the AX.5 pump task step emits through the rendered-text path this sprint introduces (HerdrNudgeTarget.rendered_nudge, prompt(text)); AX.4 is not a dependent of this sprint.
---

# AX.2 — Herdr renders the built-in nudge template

Make the Herdr sink consume the same rendered template the tmux and graft
sinks consume, and retire the fixed wake text. Evidence base for the
multi-line prompt: live check on rand-m5 with herdr 0.8.2 on 2026-09-05,
where a `codex` agent and a `claude-code` agent each received the
six-line Delivery body as one submission.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [ ] D1 — `HerdrNudgeTarget` carries `rendered_nudge: String`
  (`crates/atm-core/src/boundary/mod.rs`, code contract C1). The Herdr
  branch of `build_built_in_dispatch` in `crates/atm-core/src/send/hook.rs`
  calls `render_built_in_nudge_for_dispatch(runtime, event, kind)` exactly
  as the tmux branch does and returns `None` on render failure the same
  way.
- [ ] D2 — `HerdrProcessAdapter::prompt` gains the text parameter
  (`crates/atm-herdr/src/lib.rs`, code contract C2). Implementations
  updated: `HerdrProcessInvoker` (lib.rs, `impl HerdrProcessAdapter for
  HerdrProcessInvoker`), `atm_herdr::testing::FakeHerdrProcessAdapter`,
  and `BenchmarkNoopHerdrProcessAdapter` in
  `crates/atm-daemon-bootstrap/src/received_hook_selector.rs`.
  `prompt_args` gains the text; `HERDR_WAKE_TEXT` and the test
  `prompt_text_is_fixed_and_non_empty` are deleted. Empty or
  whitespace-only text is rejected before spawning with the existing
  `empty_agent_prompt` error (ADR-058 D8).
- [ ] D3 — callers pass the text through: the single production call
  site in `crates/atm-daemon-bootstrap/src/received_hook_selector.rs`
  (Herdr emitter, currently around line 420) reads
  `target.rendered_nudge`. The pump in
  `crates/atm-http-runtime/src/herdr_queue_wake.rs` builds no
  `HerdrNudgeTarget` of its own: it inherits the rendered text through
  `rebuild_received_hook_dispatch` → `build_built_in_dispatch` and passes
  the dispatch to the same emitter. No change to
  `crates/atm-http-runtime/src/storage_and_nudge_router.rs`.
- [ ] D4 — PTY line-safety fixture test in `atm-herdr` asserting the
  emitter passes multi-line text through unmodified (no newline stripping
  or joining) and that argv has exactly four elements.
- [ ] D5 — ADR-058 amendment
  (`docs/adr/ADR-058-herdr-local-steer-backend-contract.md`): D2 and D4
  replace "fixed prompt text" with "the rendered built-in nudge template
  resolved for the recipient team and kind"; argv shape unchanged
  (`agent prompt <name> <text>`); session still travels via
  `HERDR_SESSION` in the child environment; the line-safety rule from D4
  recorded; dated history entry.
- [ ] D6 — `boundaries/atm-herdr/herdr-process-adapter.toml`:
  `[contracts]` notes record that prompt text is caller-supplied rendered
  template text and that the adapter never composes text; `[status]`
  note dated.
- [ ] D7 — `docs/atm-herdr/requirements.md`: `HR-CORE-002` (line 112)
  rewritten to "emits exactly `herdr agent prompt <AgentName> <text>`
  where `<text>` is the caller-supplied rendered built-in nudge template
  (ADR-058 D2 as amended); no `--wait` and no other flag; the crate never
  composes or alters the text". `HR-SAFE-003` (line 208) restated with a
  dated note: "The only text this crate writes into a Herdr child's argv
  is the rendered built-in nudge template produced by atm-core's renderer
  for the recipient team and kind — the same text the tmux and graft
  sinks inject. The crate never reads message content itself. The
  template's `description` placeholder resolves to the message summary,
  or to the message text when no summary was given, exactly as on the
  other backends; no additional length bound is imposed beyond the
  operating system's single-argument limit." The verification bullet at
  line 396 keeps "match `herdr-cli-contract-fixture.md` verbatim".
  `docs/atm-herdr/architecture.md`: trait signature near line 186 gains
  the `text` parameter; fixture references near lines 513 and 534 keep
  their wording. `docs/plans/phase-aq/fixtures/herdr-cli-contract-fixture.md`
  line 51: the prompt row becomes
  `["herdr","agent","prompt","agent-a","<rendered built-in nudge template>"]`
  with one sentence stating the fourth element is the rendered template.
- [ ] D8 — tests listed under Required validation.

### Paths to delete

None (two symbols deleted inside `crates/atm-herdr/src/lib.rs`).

### Paths that must not change

- `docs/plans/**` other than the fixture row above: planning prose may
  keep the name `HERDR_WAKE_TEXT`.

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

`rendered_nudge` is already an accepted identifier in
`scripts/check-nudge-taxonomy.py`; no allowlist change.

### C2 — adapter contract

```rust
// crates/atm-herdr/src/lib.rs
pub trait HerdrProcessAdapter: Send + Sync {
    fn prompt<'a>(
        &'a self,
        agent: &'a AgentName,
        session: Option<&'a HerdrSession>,
        text: &'a str,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrPromptOutcome, HerdrError>> + Send + 'a>>;
    // wait, get, list: unchanged
}

fn prompt_args(agent: &AgentName, text: &str) -> Vec<String>;
// exactly ["agent", "prompt", <agent>, <text>]; session is NOT an argv
// element — session_environment(session) sets HERDR_SESSION as today.
// text.trim().is_empty() => Err(HerdrError::EmptyAgentPrompt) before spawn.
```

argv[3] changes from the fixed wake text to the rendered template; no
argv element is added or removed for `session: Some` or `session: None`.

### Unchanged surfaces

`LocalTmuxNudgeTarget`; `session_environment`; ADR-058 D1, D3, D5–D8;
the `herdr agent rename` identity rule; `HerdrProcessAdapter::{wait, get,
list}`; `crates/atm-http-runtime/src/storage_and_nudge_router.rs`.

## Acceptance criteria

1. `grep -rn HERDR_WAKE_TEXT crates docs/adr docs/atm-herdr docs/requirements.md docs/architecture.md boundaries`
   returns nothing (`docs/plans/**` is excluded by design).
2. A Herdr-backed member's prompt text equals the tmux-backed render for
   the same `PostSendHookEvent`.
3. The prompt text the pump emits for a queued message equals the
   Queue / QueueAck / Task render `build_built_in_dispatch` produces for
   the same member and message.
4. `prompt_args` output has exactly four elements for `session: Some` and
   `session: None`, and argv[3] is byte-identical to a six-line input.
5. ADR-058 amended with a dated history entry; `HR-CORE-002` and
   `HR-SAFE-003` read as in D7 and `grep -n 'fixed mailbox-read prompt\|never interpolated' docs/atm-herdr/requirements.md`
   returns nothing; `boundary-guard` review of
   `herdr-process-adapter.toml` passes; `just validate` green.

## Required validation

- Unit (`crates/atm-core/src/send/hook.rs` tests): render one
  `PostSendHookEvent` through the tmux and Herdr dispatch builders;
  assert identical `rendered_nudge`.
- `crates/atm-core/tests/nudge_mode.rs`: Herdr member, `atm send` then
  `atm queue`; assert the emitted prompt text for each equals the expected
  Delivery / Queue default render.
- `crates/atm-herdr` tests updated for the parameterised prompt; the D4
  fixture test (AC 4); `FakeHerdrProcessAdapter` records the text.
- `crates/atm-http-runtime/src/herdr_queue_wake.rs`: the full
  `ac01`–`ac12` set still passes, with
  `ac08_dispatch_selector_is_used_by_tick_once` updated to assert the
  rendered text carried on the Herdr target.
- `crates/atm-daemon-bootstrap` selector tests updated for the call site.
- `crates/atm-herdr` argv-equality tests updated to the D7 fixture row.
- `just validate`; quality-mgr Final Quality Report on the PR; `arch-qa`
  review of the ADR-058 amendment; `boundary-guard` on the Herdr boundary
  record.

## Out of scope

Coalescing of rapid steers; watermark; re-nudge for non-task immediate
sends; doctor mixed-backend finding; the splice-into-typing limitation
(PTY cursor position), tracked on #1173.
