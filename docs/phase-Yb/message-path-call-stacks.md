# Phase Yb Message-Path Call Stacks

Baseline:

- planning branch: `message-path-consolidation-plan-Yb`
- implementation baseline under review: `integrate/phase-Y` at `b8785617`

This document traces the current production call stacks that Yb must simplify
or replace.

## 1. New Message Success, Claude Harness

Current stack:

1. `crates/atm-core/src/send/mod.rs:190`
   - `send_mail_with_runtime_impl(...)`
2. `crates/atm-core/src/send/mod.rs:369`
   - `prepare_send_context(...)`
3. `crates/atm-core/src/send/mod.rs:515`
   - `persist_send_message(...)`
4. `crates/atm-core/src/send/persistence.rs:19`
   - `persist_message_and_seed_workflow(...)`
5. `crates/atm-core/src/send/persistence.rs:36`
   - `runtime.commit_workflow_state(...)`
6. `crates/atm-core/src/send/persistence.rs:51`
   - `runtime.append_compat_inbox_message(...)`
7. `crates/atm-core/src/service_runtime.rs:196`
   - `append_compat_inbox_message(...)`
8. `crates/atm-core/src/mailbox/store.rs:29`
   - `append_compat_mailbox_message(...)`
9. `crates/atm-core/src/send/mod.rs:238`
   - `finalize_send_outcome(...)`
10. `crates/atm-core/src/send/mod.rs:322`
    - `run_send_post_send_hooks(...)`
11. `crates/atm-core/src/send/hook.rs:57`
    - `maybe_run_post_send_hook(...)`

Issue:

- the outer send flow still owns post-persist notification branching and
  transition emission

## 2. New Message Success, Non-Claude Harness

Current stack:

1. `send_mail_with_runtime_impl(...)`
2. `prepare_send_context(...)`
3. `persist_send_message(...)`
4. `persist_message_and_seed_workflow(...)`
5. `runtime.commit_workflow_state(...)`
6. `runtime.append_compat_inbox_message(...)`
7. `crates/atm-core/src/service_runtime.rs:202`
   - immediate non-Claude no-op return
8. `finalize_send_outcome(...)`
9. `run_send_post_send_hooks(...)`
10. `maybe_run_post_send_hook(...)`

Issue:

- the non-Claude path shares the outer send flow, but the low-level Claude
  writer silently no-ops instead of never being selected in the first place

## 3. SQLite Failure, Claude Harness

Current stack:

1. `persist_send_message(...)`
2. `persist_message_and_seed_workflow(...)`
3. `runtime.commit_workflow_state(...)` returns mailbox-write error
4. `crates/atm-core/src/send/persistence.rs:72`
   - `recover_after_sqlite_failure(...)`
5. `crates/atm-core/src/send/persistence.rs:85`
   - `recipient.allows_claude_jsonl_append()`
6. `crates/atm-core/src/send/persistence.rs:86`
   - append original message directly
7. `crates/atm-core/src/send/persistence.rs:87`
   - append companion error directly
8. `finalize_send_outcome(...)`
9. `run_send_post_send_hooks(...)`
10. original hook + companion hook path

Issue:

- two outward deliveries happen inside persistence code
- partial append is possible between steps 6 and 7
- the contract is not atomic at the plan level

## 4. SQLite Failure, Non-Claude Harness

Current stack:

1. `persist_send_message(...)`
2. `persist_message_and_seed_workflow(...)`
3. `runtime.commit_workflow_state(...)` returns mailbox-write error
4. `recover_after_sqlite_failure(...)`
5. no outward payload delivery occurs in persistence
6. `DeliveryPersistenceResult::sqlite_failed_recovered(...)`
   returns `CompanionNudgePlan`
7. `finalize_send_outcome(...)`
8. `run_send_post_send_hooks(...)`
9. original post-send hook + companion post-send hook

Issue:

- the plan produces two hook invocations, not two explicitly delivered logical
  messages
- hook payloads are metadata only and therefore cannot prove identical payload
  semantics with the Claude path

## 5. Append Degraded After SQLite Success

Current stack:

1. `persist_message_and_seed_workflow(...)`
2. SQLite workflow commit succeeds
3. `runtime.append_compat_inbox_message(...)` fails
4. `DeliveryPersistenceResult::append_degraded(...)`
5. `finalize_send_outcome(...)`
6. `run_send_post_send_hooks(...)`
7. `emit_delivery_transitions(...)`
8. `append_failure_transition_names(...)`

Issue:

- append degradation is translated in outer send code instead of being emitted
  by the machine/executor that owns the actual path

## 6. Ack Reply Delivery

Current stack:

1. `crates/atm-core/src/ack/mod.rs:368`
   - `persist_ack_reply(...)`
2. ack state persisted in SQLite
3. `crates/atm-core/src/ack/mod.rs:416`
   - `persist_message_and_seed_workflow(...)`
4. same new-message persistence and degraded-delivery logic as send
5. `crates/atm-core/src/ack/mod.rs:442`
   - `finalize_ack_outcome(...)`
6. `crates/atm-core/src/ack/mod.rs:511`
   - `collect_ack_hook_warnings(...)`
7. original hook + optional companion hook

Issue:

- ack reply still depends on the same shared degraded-delivery helper and the
  same hook semantics, but through a separate outer call graph

## 7. Required End-State

After Yb:

1. caller constructs event-family request
2. coordinator resolves canonical harness snapshot
3. event-family machine returns a uniform delivery plan
4. shared executors run:
   - Claude inbox writer only for Claude delivery targets
   - non-Claude outbound writer only for non-Claude delivery targets
   - post-send notification executor for the same logical plan
5. transition emission occurs from the same machine/executor result surface

Outside the state machines and shared executors, there should be no harness
branching and no payload-shape branching.

