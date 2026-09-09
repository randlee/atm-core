---
phase: AZ
sprint: AZ.1
title: Bounded task-nudge metadata contract
branch: feature/az1-task-nudge-contract
integration_branch: develop
status: planned
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: standalone
dependency_relations:
  - prerequisite: develop@0ca0878cf4045546fb7f4b4f14dc6c473995158c
    dependent: AZ.1
    relation: must_follow
    rationale: The repair edits the Phase AX template/task-reminder surface and the retained Phase AY Tokio/Axum delivery path. Current develop must be merged forward before implementation and before each QA round.
---

# AZ.1 — Bounded task-nudge metadata contract

## Goal

Repair the content boundary for every ATM nudge without changing task
lifecycle semantics. A nudge may project the persisted message id, persisted
title/summary, and optional task id. Nudge construction must not directly read
or fall back to the immutable message body, rendered J2 output, or
`TaskRow.description`. The recipient reads the full body only through
`atm read --message-id`.

## Current defect, verified on the planning baseline

- `crates/atm-core/src/send/hook.rs::post_send_event_from_message` uses
  `envelope.text` when `envelope.summary` is absent or blank.
- `crates/atm-core/src/nudge_dispatch.rs::rebuild_received_hook_dispatch`
  repeats the same raw-text fallback for a queued/rebuilt event.
- `crates/atm-storage-rusqlite/src/writer/task_ops.rs` persists the rendered
  assignment body as `tasks.description`, and
  `crates/atm-core/src/nudge_dispatch.rs::build_task_reminder_dispatch` places
  `TaskRow.description` into the reminder event.
- `crates/atm-core/src/send/nudge_template.rs` names the body-capable event
  field and placeholder `description`; graft and Python projections repeat
  that field in host-visible notice/body values.

The storage write is evidence for the defect, not a migration target. This
sprint stops consuming `TaskRow.description` for nudges and leaves the column,
schema, replay rule, and task-list behavior unchanged.

## Notification contract

### Event signature

The public boundary uses this field shape; implementations must not retain a
second body-capable display field or an equivalent broad string input:

```rust
pub struct PostSendHookEvent {
    // Existing sender, recipient, routing, ack, and pane fields remain.
    pub message_id: AtmMessageId,
    /// Persisted MessageEnvelope.summary after blank-to-empty normalization.
    pub title: String,
    pub task_id: Option<TaskId>,
}

fn nudge_title(summary: Option<&str>) -> String {
    summary
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_default()
        .to_owned()
}
```

`nudge_title` accepts summary metadata only. It must not accept an envelope,
message body, task description, template source, or rendered template body.
Newly admitted ordinary and J2 messages already persist their computed or
explicit summary before nudge planning; nudge construction consumes that
persisted field and does not recompute it.

Phase AZ does not change `crates/atm-core/src/send/summary.rs` or title
generation. If a sender omits an explicit `--summary`, the existing
`build_summary` policy persists a bounded summary derived from the admitted
ordinary or J2-rendered body. That persisted summary is permitted title
metadata; the forbidden behavior is reading or falling back to full body text
again while constructing or projecting a nudge.

Because `PostSendHookEvent` is serialized at retained compatibility seams,
its serialized key is `title`; there is no `description` or `summary` JSON
alias. The deprecated `{{description}}` compatibility described below applies
only to stored nudge-template placeholder names, not to event or hook payload
fields.

For task reminders, `TaskRow.assignment_message_id` identifies the message.
The builder loads that message once, derives both authenticated source-host
metadata and title from it, and emits an empty title if the assignment message
or its nonblank summary is absent. It still emits the reminder because the
durable task row owns reminder eligibility. It never falls back to
`TaskRow.description`.

### Template projection

The canonical template placeholder is `{{title}}`. The seven default template
kinds retain their existing read/ack/execute/idle controls, but every current
`{{description}}` value position is fed by `title` instead:

```xml
<atm from="{{from}}" message-id="{{message_id}}">
  <action>atm read --message-id {{message_id}}</action>
  <description>{{title}}</description>
  <action>execute the assigned task</action>
  <when idle="immediate" busy="after-current-task"/>
  <console announce="concise" pause="false"/>
</atm>
```

The Task template retains its task marker and uses the same value source:

```xml
<task id="{{task_id}}">{{title}}</task>
```

For compatibility with already persisted team overrides,
`{{description}}` remains a deprecated *placeholder-name* alias for
`{{title}}` in this sprint. It is not a description field: the renderer maps
both names to `PostSendHookEvent.title`, so the alias cannot access a message
body. User documentation names `{{title}}` as canonical and marks the alias as
title-only compatibility behavior. A blank title renders as an empty value;
no fallback text is synthesized from message content.

### Forbidden data flow

At nudge-construction and projection time, for every Steer, Queue, rebuilt
Queue, Task, acknowledge-family nudge, task reminder, graft notice, and Python
`PyNudge` projection:

```text
MessageEnvelope.text / TaskRow.description / rendered J2 body
    -X-> PostSendHookEvent.title
    -X-> rendered nudge
    -X-> graft notice_text or body
    -X-> Herdr prompt
```

`MessageEnvelope.summary -> PostSendHookEvent.title` is the only permitted
message-title edge. Admission-time `body -> build_summary -> persisted
MessageEnvelope.summary` behavior is unchanged and is upstream of this
boundary. Fixed XML/control text and sender/recipient routing metadata are
outside the message-derived payload set.

## Deliverables

This is the sole authoritative deliverables list for Phase AZ.

Every D1–D6 deliverable must land at a production-ready level in AZ.1. A
field rename, documentation-only update, test-only proof, or partial consumer
migration cannot satisfy the sprint while any listed runtime projection or
acceptance criterion remains open.

- [ ] D1 — Amend the normative notification contract in
  `docs/requirements.md`, `docs/architecture.md`, `docs/atm/requirements.md`,
  `docs/atm/architecture.md`, `docs/atm-core/requirements.md`,
  `docs/atm-core/architecture.md`, `docs/atm-core/boundaries.md`,
  `docs/atm-http-runtime/architecture.md`, `docs/atm-graft/requirements.md`,
  `docs/atm-graft/architecture.md`, `docs/atm-graft/boundaries.md`,
  `docs/atm-herdr/requirements.md`, `docs/atm-herdr/architecture.md`, and
  `docs/atm-herdr/boundaries.md`. Update the machine-readable contracts in
  `boundaries/atm-core/message-received-hook-emitter.toml`,
  `boundaries/atm-graft/message-received-hook.toml`, and
  `boundaries/atm-herdr/herdr-process-adapter.toml`. Update
  `docs/user-documents/hooks.md` and its
  `docs/user-documents/examples/hooks/post-send-payload.json` example for the
  serialized `title` key. Append a dated Phase AZ amendment to
  `docs/adr/ADR-019-direct-post-send-and-claude-json-retirement.md`. ADR-019's
  status does not change, so its index entry does not change. The documents
  must state the allowed message-derived fields, title-only source,
  missing-title rule, and `atm read --message-id` body boundary.
- [ ] D2 — Replace the body-capable `PostSendHookEvent.description` semantic
  with `title` in `crates/atm-core/src/boundary/mod.rs`. Add one summary-only
  normalization helper and use it from
  `crates/atm-core/src/send/hook.rs::post_send_event_from_message` and
  `crates/atm-core/src/nudge_dispatch.rs::rebuild_received_hook_dispatch`.
  Delete both `envelope.text` fallbacks.
- [ ] D3 — In `crates/atm-core/src/nudge_dispatch.rs`, build Task reminders
  from the persisted assignment message's summary. Preserve reminder delivery
  when that message/title is missing by using the empty title. Do not read
  `TaskRow.description`; do not change task admission, the `tasks` schema, or
  task list/order output.
- [ ] D4 — Update `crates/atm-core/src/send/nudge_template.rs` and
  `docs/user-documents/nudge-templates.md` plus its delivery, queue, task, and
  management examples: defaults use `{{title}}`, the renderer exposes
  `title`, and `description` is a deprecated title-only alias for persisted
  overrides. All seven kinds must render without a body source, including
  acknowledge-family templates.
- [ ] D5 — Carry the renamed/title-only event through retained consumers and
  test fixtures in `crates/atm-core/src/graft.rs`,
  `crates/atm-core/tests/nudge_mode.rs`,
  `crates/atm/src/commands/internal_nudge.rs`,
  `crates/atm-daemon-bootstrap/src/received_hook_selector.rs`,
  `crates/atm-http-runtime/src/storage_and_nudge_router.rs`,
  `crates/atm-http-runtime/src/herdr_queue_wake.rs`,
  `crates/atm-graft/src/nudge_sink.rs`,
  `crates/atm-graft/src/runtime/mod.rs`,
  `crates/atm-graft/examples/smoke_same_host.rs`, and
  `crates/atm-graft-python/src/lib.rs`. Update the explicit-override helpers
  `scripts/atm-nudge.py`, `scripts/atm-nudge.sh`, and
  `scripts/test_atm_nudge.py` to consume `title` and prove that neither an old
  `description` value nor a `summary` fallback can become body input. Graft
  `body` remains the canonical rendered ATM nudge; graft `notice_text` and
  legacy Python projections may use only title metadata, never immutable body
  text.
- [ ] D6 — Add the focused regression suite in the files above plus
  `crates/atm-core/src/send/tests.rs`,
  `crates/atm-core/src/send/post_write_tests.rs`,
  `crates/atm-core/tests/nudge_dispatch.rs`, and
  `crates/atm-core/tests/task_reminder_dispatch.rs`. Core send/nudge tests own
  direct post-send Steer, deferred Queue, queue rebuild, Task assignment,
  periodic Task reminder, missing-title, and completed-task selection proofs.
  Graft and Herdr tests own their final projections. The real
  `atm-http-runtime` template route in
  `crates/atm-http-runtime/src/storage_and_nudge_router.rs` owns the admitted
  J2 case; a source-construction-only CLI test does not satisfy it. At least
  one ordinary long body and one successfully admitted J2-rendered long body
  must each use a distinct explicit title and carry unique body-only secret
  sentinels. Assert those sentinels are absent from the event, rendered nudge,
  graft notice/body, and Herdr request while the persisted title, message id,
  and optional task id remain present. Preserve a focused proof that task
  completion excludes the completed row from periodic reminder selection.

## Affected paths

This is the authoritative implementation path list. Adding a production path
requires a written scope amendment before code review.

```text
crates/atm-core/src/boundary/mod.rs
crates/atm-core/src/graft.rs
crates/atm-core/src/nudge_dispatch.rs
crates/atm-core/src/send/hook.rs
crates/atm-core/src/send/nudge_template.rs
crates/atm-core/src/send/post_write_tests.rs
crates/atm-core/src/send/tests.rs
crates/atm-core/tests/nudge_dispatch.rs
crates/atm-core/tests/nudge_mode.rs
crates/atm-core/tests/task_reminder_dispatch.rs
crates/atm/src/commands/internal_nudge.rs
crates/atm-daemon-bootstrap/src/received_hook_selector.rs
crates/atm-http-runtime/src/herdr_queue_wake.rs
crates/atm-http-runtime/src/storage_and_nudge_router.rs
crates/atm-graft/src/nudge_sink.rs
crates/atm-graft/src/runtime/mod.rs
crates/atm-graft/examples/smoke_same_host.rs
crates/atm-graft-python/src/lib.rs
scripts/atm-nudge.py
scripts/atm-nudge.sh
scripts/test_atm_nudge.py
boundaries/atm-core/message-received-hook-emitter.toml
boundaries/atm-graft/message-received-hook.toml
boundaries/atm-herdr/herdr-process-adapter.toml
docs/adr/ADR-019-direct-post-send-and-claude-json-retirement.md
docs/architecture.md
docs/atm/architecture.md
docs/atm/requirements.md
docs/atm-core/architecture.md
docs/atm-core/boundaries.md
docs/atm-core/requirements.md
docs/atm-graft/architecture.md
docs/atm-graft/boundaries.md
docs/atm-graft/requirements.md
docs/atm-herdr/architecture.md
docs/atm-herdr/boundaries.md
docs/atm-herdr/requirements.md
docs/atm-http-runtime/architecture.md
docs/requirements.md
docs/user-documents/hooks.md
docs/user-documents/examples/hooks/post-send-payload.json
docs/user-documents/nudge-templates.md
docs/user-documents/examples/nudge-templates/delivery.xml
docs/user-documents/examples/nudge-templates/delivery_ack.xml
docs/user-documents/examples/nudge-templates/manage-templates.sh
docs/user-documents/examples/nudge-templates/queue.xml
docs/user-documents/examples/nudge-templates/queue_ack.xml
docs/user-documents/examples/nudge-templates/task.xml
```

### Paths to delete

None.

### Paths that must not change

- `crates/atm-daemon/**` and every other legacy synchronous daemon runtime or
  dispatch path.
- `crates/atm-storage-rusqlite/src/writer/task_ops.rs`, task schema/migration
  files, and `boundaries/atm-storage/task-store.toml`.
- `crates/atm-core/src/send/summary.rs`; the existing admission-time summary
  generation policy is not part of this repair.
- task list/order/table code.
- pending-nudge claim, requeue, deduplication, invalidation, and resend logic.
- `docs/adr/INDEX.md`; ADR-019 remains accepted and its index summary is not a
  Phase AZ deliverable.
- `docs/atm-http-runtime/openapi.yaml`; no HTTP route or wire DTO changes.
- historical Phase AD/AQ/AX/AY sprint plans and committed evidence.

## Acceptance criteria

This is the sole authoritative acceptance list for Phase AZ.

1. Every current post-send Steer, queued delivery, rebuilt queued delivery,
   Task assignment, acknowledge-family, and task-reminder construction path
   obtains display text only from persisted `MessageEnvelope.summary`.
   Retained serialized event/hook payloads expose that value as `title`, with
   no `description` or `summary` JSON alias.
2. No production nudge builder or sink falls back to
   `MessageEnvelope.text`, `TaskRow.description`, template source, or rendered
   J2 output. A repository search and focused tests prove the forbidden edges
   are absent.
3. A missing assignment message, missing summary, whitespace-only summary, or
   legacy record produces an empty title and still emits the applicable nudge
   with the correct message id and optional task id.
4. All seven default kinds render through the title-only contract. Persisted
   overrides using `{{description}}` continue to render the same title value
   and cannot gain access to body content; new documentation uses
   `{{title}}`.
5. Long ordinary and J2-rendered bodies are persisted/read normally, but their
   unique sentinels appear in no event, rendered nudge, graft notice/body, or
   Herdr request. `atm read --message-id` remains the only tested body path.
6. `tasks.description` remains schema-compatible and unchanged, and a TaskRow
   description sentinel is absent from Task assignment/reminder nudges.
7. Completing a task still removes it from periodic reminder selection. No
   claim is made that completion cancels older independent pending-nudge
   message rows.
8. No legacy synchronous daemon code changes and no live daemon/test-daemon,
   release, tag, publish, or install operation occurs.

## Required validation

This is the sole authoritative validation list for Phase AZ. All commands run
from the AZ.1 worktree with fakes, fixtures, and temporary stores only.

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p agent-team-mail-core nudge_template
cargo test -p agent-team-mail-core --test nudge_dispatch
cargo test -p agent-team-mail-core --test nudge_mode
cargo test -p agent-team-mail-core --test task_reminder_dispatch
cargo test -p agent-team-mail-core graft
cargo test -p agent-team-mail internal_nudge
cargo test -p atm-http-runtime herdr_queue_wake
cargo test -p atm-http-runtime storage_and_nudge_router
cargo test -p atm-daemon-bootstrap received_hook_selector
cargo test -p atm-graft nudge_sink
just test-graft-python
python3 scripts/test_atm_nudge.py
! rg -n 'envelope\.text|row\.description' crates/atm-core/src/send/hook.rs crates/atm-core/src/nudge_dispatch.rs
! rg -n 'payload\.get\("(description|summary)"\)' scripts/atm-nudge.py scripts/atm-nudge.sh
python3 .just/run_lint.py boundaries
python3 .just/run_lint.py nudge-taxonomy
git diff --check develop...HEAD
```

The implementation report must name the focused long-body/J2 test cases,
show the forbidden-sentinel assertions, and report the final head SHA.

## Explicit non-goals and follow-up

- No removal or migration of `tasks.description`.
- No change to `build_summary`, explicit-summary precedence, summary length,
  or admission-time summary persistence.
- No task ordering, list table, or display redesign.
- No resend deduplication.
- No task-aware cancellation of prior pending-nudge rows.
- No legacy daemon work and no live daemon evidence.

The separate completed-task defect remains open: pending nudge state is keyed
by `(team, agent, message_key)`, and claim selection does not join message
`task_id` to task state. Completion acknowledges/clears only the task ledger's
current `assignment_message_id`; earlier task-linked messages or another
pending/requeued message remain eligible for ordinary queue drain.

Required discovery question for that follow-up: **How should task-aware
pending-nudge invalidation/supersession cover every message associated with a
completed task, without coupling it to Phase AZ's notification-body repair?**

## Review gates

- Rust review checks that the title helper is summary-only, no broad string or
  envelope API can accidentally receive a body, and errors remain structured.
- Requirements review checks the same allowed-field set and missing-title rule
  in every normative document.
- QA checks every listed delivery class plus both negative long-body cases.
- Architecture review rejects any edit to the frozen synchronous daemon or any
  replacement of Tokio/Axum `atm-http-runtime` composition.
