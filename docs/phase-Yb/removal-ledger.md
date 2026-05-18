# Phase Yb Removal Ledger

Baseline:

- planning branch: `message-path-consolidation-plan-Yb`
- implementation baseline under review: `integrate/phase-Y` at `b8785617`

This ledger is authoritative for Yb implementation planning. Any newly
discovered production message-path decision point must be added here before the
corresponding implementation sprint starts.

| ID | Sprint | File | Line | Function / Method | Keep / Delete / Move | Replacement Path | Reason |
| --- | --- | --- | ---: | --- | --- | --- | --- |
| `YB-RM-001` | `Y.7` | `crates/atm-core/src/send/persistence.rs` | 72 | `recover_after_sqlite_failure` | `Delete` outer-path delivery logic | `NewMessageStateMachine -> DeliveryPlan -> shared executors` | Persistence layer must not decide harness-specific delivery behavior. |
| `YB-RM-002` | `Y.7` | `crates/atm-core/src/send/persistence.rs` | 85 | `recipient.allows_claude_jsonl_append()` branch inside `recover_after_sqlite_failure` | `Delete` | `DeliveryPlan.target` chosen inside state machine | Harness branching here violates the “no policy outside state machines” rule. |
| `YB-RM-003` | `Y.7` | `crates/atm-core/src/send/persistence.rs` | 86 | `runtime.append_compat_inbox_message(... original_message)` | `Delete` from persistence layer | `ClaudeInboxWriter::deliver(plan.messages[0])` | Direct outward delivery from persistence prevents shared executor shape. |
| `YB-RM-004` | `Y.7` | `crates/atm-core/src/send/persistence.rs` | 87 | `runtime.append_compat_inbox_message(... &companion)` | `Delete` from persistence layer | `ClaudeInboxWriter::deliver(plan.messages[1])` | Partial append here creates the current atomicity gap. |
| `YB-RM-005` | `Y.7` | `crates/atm-core/src/send/persistence.rs` | 98 | `DeliveryPersistenceResult::sqlite_failed_recovered(...)` carrying `CompanionNudgePlan` only | `Move` / redesign | typed degraded `DeliveryPlan` + error disposition | Companion nudge alone is not a complete non-Claude delivery contract. |
| `YB-RM-006` | `Y.8` | `crates/atm-core/src/send/mod.rs` | 322 | `run_send_post_send_hooks` | `Move` behind shared executor boundary | `PostSendNotificationExecutor` | Outer send path must not own harness/degradation notification branching. |
| `YB-RM-007` | `Y.8` | `crates/atm-core/src/send/mod.rs` | 345 | `if let Some(companion_nudge)` branch | `Delete` from outer send flow | machine-owned notification plan | Companion notification decisions belong to the machine plan. |
| `YB-RM-008` | `Y.7` | `crates/atm-core/src/send/mod.rs` | 238 | `finalize_send_outcome` | `Move` / narrow | `delivery_execution::execute_delivery_plan(...)` | Final outcome translation must follow the typed delivery plan rather than re-deriving harness semantics in `send/mod.rs`. |
| `YB-RM-009` | `Y.8` | `crates/atm-core/src/send/mod.rs` | 571 | `emit_delivery_transitions` | `Move` into coordinator/machine execution layer | coordinator emits transitions from machine result | Outer send path should not translate dispositions into transition tables. |
| `YB-RM-010` | `Y.8` | `crates/atm-core/src/send/mod.rs` | 586 | `append_failure_transition_names(route.harness)` | `Delete` outer use | machine-owned transition sequence | Append failure is not a generic outer routing concern. |
| `YB-RM-011` | `Y.8` | `crates/atm-core/src/send/hook.rs` | 57 | `maybe_run_post_send_hook` as a direct outer send caller seam | `Move` behind shared executor boundary | `PostSendNotificationExecutor` | Outer send/ack code should call one executor, not the hook helper directly. |
| `YB-RM-012` | `Y.9` | `crates/atm-core/src/send/hook.rs` | 90 | `execute_post_send_hook` as current degraded non-Claude stand-in | `Keep`, but notification-only | `PostSendNotificationExecutor` | Must remain notification-only and must not imply message delivery. |
| `YB-RM-013` | `Y.9` | `crates/atm-core/src/send/hook.rs` | 104 | JSON payload construction in `execute_post_send_hook` | `Keep`, but narrow | notification metadata only | This payload does not carry message bodies and therefore cannot satisfy delivery semantics. |
| `YB-RM-014` | `Y.9` | `crates/atm-core/src/delivery_policy.rs` | 61 | `DeliveryRecipientSnapshot::fallback_claude` | `Delete` or replace with fail-closed deferred error | typed unsupported / roster-missing error | Silent fallback to Claude path is incompatible with strict harness-based routing. |
| `YB-RM-015` | `Y.9` | `crates/atm-core/src/delivery_policy.rs` | 573 | `append_failure_transition_names` | `Move` / narrow to Claude-only machine artifact | `ClaudeHarnessNewMessageState` transitions | Generic helper keeps impossible or misleading surfaces alive. |
| `YB-RM-016` | `Y.9` | `crates/atm-core/src/delivery_policy.rs` | 585 | `DeliveryHarnessPath::NonClaude` arm in `append_failure_transition_names` | `Delete` | none | Non-Claude append degradation is not a valid runtime concept. |
| `YB-RM-017` | `Y.10` | `crates/atm-core/src/service_runtime.rs` | 176 | `refresh_compat_inbox_projection` | `Move` to repair/rebuild-only seam | explicit rebuild executor | Runtime refresh logic must not remain a general mutable message path. |
| `YB-RM-018` | `Y.10` | `crates/atm-core/src/service_runtime.rs` | 196 | `append_compat_inbox_message` | `Keep`, but daemon-private executor-only | `ClaudeInboxWriter` | Low-level append primitive survives, but only behind executor boundary. |
| `YB-RM-019` | `Y.9` | `crates/atm-core/src/service_runtime.rs` | 202 | non-Claude early return in `append_compat_inbox_message` | `Delete` from low-level writer | no call at all for non-Claude plans | The caller should never ask the Claude writer to handle non-Claude delivery. |
| `YB-RM-020` | `Y.10` | `crates/atm-core/src/service_runtime.rs` | 205 | legacy array-format branch in `append_compat_inbox_message` | `Move` / isolate | repair/rebuild compatibility path only | Normal runtime path must not silently choose rewrite fallback in the writer. |
| `YB-RM-021` | `Y.10` | `crates/atm-core/src/boundary_support.rs` | 169 | `reexport_messages` | `Keep`, but repair/rebuild-only | admin / repair executor | This stays only for explicit rebuild flows. |
| `YB-RM-022` | `Y.10` | `crates/atm-core/src/direct_boundaries.rs` | 44 | `reexport_messages` façade | `Keep`, but repair/rebuild-only | admin / repair executor | Must not become a normal send/ack runtime write escape hatch. |
| `YB-RM-023` | `Y.10` | `crates/atm-core/src/mailbox/mod.rs` | 141 | `export_compat_mailbox_projection` | `Keep`, but repair/rebuild-only | admin / repair executor | Rewrite projection belongs only to rebuild flows after Yb. |
| `YB-RM-024` | `Y.10` | `crates/atm-core/src/mailbox/store.rs` | 21 | `write_compat_mailbox_projection` | `Keep`, but repair/rebuild-only | admin / repair executor | Array rewrite must not remain on normal runtime message path. |
| `YB-RM-025` | `Y.10` | `crates/atm-core/src/mailbox/store.rs` | 29 | `append_compat_mailbox_message` | `Keep`, but executor-only | `ClaudeInboxWriter` | Survives as Claude-only low-level append primitive. |
| `YB-RM-026` | `Y.7` | `crates/atm-core/src/ack/mod.rs` | 416 | `persist_message_and_seed_workflow` in `persist_ack_reply` | `Move` through reply delivery plan | `AckReplyStateMachine -> ReplyDeliveryPlan` | Ack reply must use the same shared execution model as new-message. |
| `YB-RM-027` | `Y.7` | `crates/atm-core/src/ack/mod.rs` | 442 | `finalize_ack_outcome` | `Move` / narrow | `delivery_execution::execute_reply_delivery_plan(...)` | Ack should not own a second outer disposition-to-notification translation path. |
| `YB-RM-028` | `Y.7` | `crates/atm-core/src/ack/mod.rs` | 511 | `collect_ack_hook_warnings` | `Move` behind shared executor boundary | `PostSendNotificationExecutor` | Ack path should not own separate notification logic shape. |
| `YB-RM-029` | `Y.11` | `crates/atm-core/src/service_runtime.rs` | 238 | non-Claude no-op branch in `refresh_compat_inbox_projection` | `Delete` from mixed runtime seam | explicit Claude repair/rebuild seam only | The refresh seam must be repair/rebuild-only by construction, not a generic recipient-routed helper that silently ignores non-Claude requests. |
| `YB-RM-030` | `Y.11` | `crates/atm-core/src/service_runtime.rs` | 259 | non-Claude validation branch in `append_compat_inbox_message` | `Delete` from low-level Claude writer | no call at all for non-Claude plans | The Claude append primitive must never be selected for `DeliveryHarnessPath::NonClaude`; rejecting that route inside the low-level writer still leaves policy at the wrong seam. |

## Keep Rules

The following categories survive Yb, but only behind the documented executor
or repair boundaries:

- Claude-only append primitive
- repair / rebuild mailbox projection rewrite
- post-send notification execution
- coordinator-level transition emission

What must not survive:

- harness-specific outer branching in `send`, `ack`, or persistence helpers
- metadata-only hook invocation used as proof of message delivery
- direct persistence-to-outward-delivery coupling
- implicit fallback from unknown/missing roster data to Claude append

## Y.7 Closure Notes

`feature/pYb-s7-degraded-delivery-contract-hardening` closes the required
`Y.7` rows by replacing persistence-owned delivery with typed plan/executor
seams.

Closed rows:

- `YB-RM-001`
- `YB-RM-002`
- `YB-RM-003`
- `YB-RM-004`
- `YB-RM-005`
- `YB-RM-008`
- `YB-RM-026`
- `YB-RM-027`
- `YB-RM-028`

Implemented seam on this branch:

- [send/persistence.rs:17](../../crates/atm-core/src/send/persistence.rs)
  now persists only and returns typed degraded payloads
- [send/mod.rs:323](../../crates/atm-core/src/send/mod.rs)
  builds `DeliveryPlan`
- [delivery_execution.rs:97](../../crates/atm-core/src/delivery_execution.rs)
  executes `DeliveryPlan`
- [ack/mod.rs:514](../../crates/atm-core/src/ack/mod.rs)
  routes reply delivery through
  `crates/atm-core/src/ack/mod.rs::AckReplyStateMachine -> ReplyDeliveryPlan`

Rows that remain intentionally open after `Y.7`:

- `YB-RM-006` through `YB-RM-016`
- `YB-RM-017` through `YB-RM-025`

Those remaining rows are deferred to `Y.8` through `Y.10` exactly as assigned
above; they are not regressions in `Y.7`.

## Y.8 Closure Notes

`feature/pYb-s8-policy-cleanup-and-impossible-path-removal` closes the
required `Y.8` rows by moving target construction and transition translation
out of outer send/ack callers and into `delivery_plan.rs` /
`delivery_execution.rs`.

Closed rows:

- `YB-RM-006`
- `YB-RM-007`
- `YB-RM-009`
- `YB-RM-010`
- `YB-RM-011`

Implemented seam on this branch:

- `crates/atm-core/src/delivery_plan.rs`
  - `delivery_target_for_snapshot(...)` now owns typed
    `DeliveryTarget::{ClaudeCode,NonClaude}` construction
- `crates/atm-core/src/delivery_execution.rs`
  - `emit_delivery_plan_transitions(...)`
  - `emit_reply_delivery_plan_transitions(...)`
  - `validate_delivery_target(...)`
- `crates/atm-core/src/send/mod.rs`
  - no longer translates persistence/execution outcomes into transition names
- `crates/atm-core/src/ack/mod.rs`
  - no longer translates persistence/execution outcomes into transition names

Rows that remain intentionally open after `Y.8`:

- `YB-RM-012` through `YB-RM-016`
- `YB-RM-017` through `YB-RM-025`

Those remaining rows are deferred to `Y.9` and `Y.10` exactly as assigned
above; they are not regressions in `Y.8`.

## Y.9 Closure Notes

`feature/pYb-s9-non-claude-outbound-boundary-formalization` closes the
required `Y.9` rows by making non-Claude delivery a first-class payload
boundary and removing the remaining fallback surfaces.

Closed rows:

- `YB-RM-012`
- `YB-RM-013`
- `YB-RM-014`
- `YB-RM-015`
- `YB-RM-016`
- `YB-RM-019`

Implemented seam on this branch:

- [delivery_execution.rs:112](../../crates/atm-core/src/delivery_execution.rs)
  now owns `NonClaudeOutboundDeliveryWriter`
- [service_runtime.rs:295](../../crates/atm-core/src/service_runtime.rs)
  now hands typed non-Claude payloads to
  `atm_core::boundary::NonClaudeOutbound`
- [non_claude_outbound_runtime.rs:14](../../crates/atm-daemon/src/non_claude_outbound_runtime.rs)
  now provides the daemon-owned
  `atm_daemon::non_claude_outbound_runtime::DaemonNonClaudeOutbound` adapter
- [delivery_policy.rs:300](../../crates/atm-core/src/delivery_policy.rs)
  now fails closed when roster-backed harness data is missing
- [delivery_policy.rs:506](../../crates/atm-core/src/delivery_policy.rs)
  now keeps append-degraded transitions Claude-only
- [send/mod.rs:1538](../../crates/atm-core/src/send/mod.rs)
  now proves non-Claude delivery through the outbound payload boundary rather
  than through hook metadata alone

Rows that remain intentionally open after `Y.9`:

- `YB-RM-017`
- `YB-RM-018`
- `YB-RM-020`
- `YB-RM-021`
- `YB-RM-022`
- `YB-RM-023`
- `YB-RM-024`
- `YB-RM-025`

Those remaining rows are deferred to `Y.10` exactly as assigned above; they
are not regressions in `Y.9`.

## Y.10 Closure Notes

`feature/pYb-s10-boundary-enforcement-and-smoke-handoff` closes the original
`Y.10` rows by isolating full mailbox rewrite behind the explicit
repair/rebuild seam and removing the last silent runtime fallback from append
delivery into full re-export, but the post-sprint review reopened two
mixed-seam runtime issues for follow-up in `Y.11`.

Closed rows:

- `YB-RM-017`
- `YB-RM-018`
- `YB-RM-020`
- `YB-RM-021`
- `YB-RM-022`
- `YB-RM-023`
- `YB-RM-024`
- `YB-RM-025`

Implemented seam on this branch:

- `crates/atm-core/src/service_runtime.rs`
  - `refresh_compat_inbox_projection(...)` remained the explicit
    repair/rebuild-only rewrite seam
  - `append_compat_inbox_message(...)` now fails closed on legacy array
    inboxes instead of silently triggering full mailbox rewrite
- `crates/atm-core/src/direct_boundaries.rs`
  - `reexport_messages(...)` is now documented as repair/rebuild-only
- `crates/atm-core/src/boundary_support.rs`
  - `reexport_messages(...)` remains the low-level rebuild/write bridge only
- `crates/atm-core/src/mailbox/mod.rs`
  - `export_compat_mailbox_projection(...)` is now explicitly documented as
    the repair/rebuild-only rewrite seam
- `boundaries/atm-core/inbox-export.toml`
  - final caller allowlists and repair/rebuild-only rewrite contract are
    declared for the `InboxExport` boundary
- `boundaries/atm-daemon/daemon-inbox-export.toml`
  - final daemon adapter allowlists and repair/rebuild-only rewrite contract
    are declared for the daemon `InboxExport` adapter

Phase-end review after `Y.10` reopened two mixed-seam runtime issues:

- `YB-RM-029`
- `YB-RM-030`

Those rows are assigned to `Y.11`. Yb is not fully closed until they are
resolved.

## Y.11 Closure Notes

`feature/pYb-s11-y10-gap-closure` closes the reopened post-`Y.10` seam issues
by tightening the retained runtime helper shapes to match the actual executor
and repair/rebuild ownership model.

Closed rows:

- `YB-RM-029`
- `YB-RM-030`

Implemented seam on this branch:

- `crates/atm-core/src/service_runtime.rs`
  - `rebuild_compat_inbox_projection(...)` now requires explicit
    `inbox_path` / `team` / `agent` inputs instead of a generic recipient
    snapshot and no longer carries a non-Claude no-op branch
  - `append_compat_inbox_message(...)` is now Claude-only by seam shape and no
    longer contains a non-Claude rejection branch
- `crates/atm-core/src/delivery_execution.rs`
  - `ClaudeInboxWriter` now delegates to the narrowed Claude append seam
    without route checking in the low-level runtime helper
- `docs/phase-Yb/testing-and-validation.md`
  - the shared validation matrix now names outbound-boundary proof instead of
    the obsolete hook-path wording

No Yb removal-ledger rows remain open after `Y.11`.
