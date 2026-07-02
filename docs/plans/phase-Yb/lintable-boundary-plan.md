# Phase Yb Lintable Boundary Plan

## Goal

Turn the Yb message-path rules into documented, machine-checkable boundaries so
future work cannot reintroduce policy leakage through convenience calls.

## Required Rule Set

### 1. No policy outside state machines

Allowed owners:

- delivery-policy coordinator
- event-family state machines

Forbidden outside those owners:

- harness branching
- payload-count decisions
- degraded-delivery branching
- append-failure routing decisions

### 2. Shared executors only

Allowed owners:

- Claude inbox writer
- non-Claude outbound delivery writer
- post-send notification executor

Forbidden direct callers:

- `send/mod.rs`
- `send/persistence.rs`
- `ack/mod.rs`
- generic service-runtime helpers acting as policy layers

### 3. Notification is not delivery

- post-send-hook code may emit notification metadata only
- notification hooks must not be used as proof that a logical message was
  delivered
- tests must validate outbound payloads through the owning delivery boundary,
  not just hook invocation count

## Proposed Mechanical Enforcement

Timeline:

- `Y.8` lands the final prose ownership rules, grep-based QA checks, and
  boundary-doc caller allowlists
- `Y.10` converts those allowlists into the final machine-enforced
  `sc-lint`/boundary gates and fail-closed runtime checks

### 1. Compile-time ownership rules

Enforcement point:

- Rust module privacy and constructor visibility in:
  - `atm_core::delivery_execution`
  - `atm_daemon::non_claude_outbound_runtime`
  - `atm_daemon::notification_runtime`

Rules:

1. concrete daemon adapters remain `pub(crate)` only
2. state-machine-owned output types live in `atm_core::delivery_plan`
3. outer callers receive typed plans, not direct writer handles
4. delivery-target construction and transition translation live only in:
   - `atm_core::delivery_plan`
   - `atm_core::delivery_execution`

### 2. `sc-lint` / boundary rules

Enforcement point:

- boundary TOML allowlists plus `python3 .just/run_lint.py all`
- rule families owned by `sc_lint_boundary`

Primitive caller allowlist:

| Primitive | Approved callers | Enforcement stance |
| --- | --- | --- |
| `RetainedServiceRuntime::append_compat_inbox_message(...)` | `atm_core::delivery_execution::ClaudeInboxWriter` | `LINT-BOUNDARY-INBOX-EXPORT-REFERENCES` plus runtime fail-closed checks |
| `mailbox::store::append_compat_mailbox_message(...)` | `atm_core::service_runtime::RetainedServiceRuntime::append_compat_inbox_message(...)` | internal implementation detail below the Claude executor seam |
| `RetainedServiceRuntime::deliver_non_claude_payloads(...)` | `atm_core::delivery_execution::NonClaudeOutboundDeliveryWriter` | `LINT-BOUNDARY-NON-CLAUDE-OUTBOUND-REFERENCES` |
| `atm_core::boundary::NonClaudeOutbound::deliver_payloads(...)` | `atm_core::service_runtime::RetainedServiceRuntime::deliver_non_claude_payloads(...)` | daemon/runtime adapter seam only |
| `RetainedServiceRuntime::maybe_run_post_send_hook(...)` | `atm_core::delivery_execution::PostSendNotificationExecutor` | notification-only seam; not accepted as delivery proof |
| `send::hook::maybe_run_post_send_hook(...)` | `atm_core::service_runtime::LocalServiceRuntime` | pub(crate) required — caller is outside the send/ module; pub(super) would break the cross-module RetainedServiceRuntime trait-impl call pattern (introduced Y.8, documented Y.12) |
| `mailbox::store::write_compat_mailbox_projection(...)` | explicit repair/rebuild-only seams | runtime delivery path forbidden |
| `direct_boundaries::reexport_messages(...)` | explicit repair/rebuild-only seams | runtime delivery path forbidden |

1. only the approved Claude executor module may call:
   - `RetainedServiceRuntime::append_compat_inbox_message(...)`
   - `mailbox::store::append_compat_mailbox_message(...)`
   - approved owner:
     `atm_core::delivery_execution::ClaudeInboxWriter`
2. only the approved non-Claude executor module may call:
   - `atm_core::boundary::NonClaudeOutbound`
   - approved owner:
     `atm_core::delivery_execution::NonClaudeOutboundDeliveryWriter`
3. only approved repair/rebuild modules may call:
   - `mailbox::store::write_compat_mailbox_projection(...)`
   - `direct_boundaries::reexport_messages(...)`
   - approved owners:
     - `atm_core::service_runtime::RetainedServiceRuntime::rebuild_compat_inbox_projection(...)`
     - `atm_core::direct_boundaries::reexport_messages(...)`
     - `atm_daemon::boundary_adapters::DaemonInboxExport::reexport_message(...)`
4. `send/persistence.rs` must not call:
   - any compatibility append/write primitive
   - any post-send notification primitive
5. `send/mod.rs` and `ack/mod.rs` must not:
   - branch on `DeliveryHarnessPath`
   - translate persistence dispositions into state-machine outcomes
   - translate execution dispositions into transition names
6. `send/hook.rs` must not:
   - accept full `MessageEnvelope` delivery authority
   - become a second outbound payload boundary
7. `service_runtime::append_compat_inbox_message(...)` must:
   - fail closed on legacy array mailboxes
   - direct callers to the explicit repair/rebuild projection seam
   - never trigger `direct_boundaries::reexport_messages(...)` from the normal
     append-only runtime path
8. the retained repair/rebuild refresh seam must not:
   - accept `DeliveryHarnessPath::NonClaude` and silently no-op
   - present a generic recipient-routed runtime helper shape when the allowed
     ownership is actually repair/rebuild-only

### 3. Runtime fail-closed checks

Enforcement point:

- `atm_core::delivery_execution::execute_delivery_plan(...)`
- `atm_core::delivery_execution::execute_reply_delivery_plan(...)`

Rules:

1. a Claude-targeted plan fails closed if routed to `NonClaudeOutbound`
2. a non-Claude-targeted plan fails closed if routed to `InboxExport`
3. `NotificationSink` rejects any attempt to serve as the sole message-delivery
   proof surface
4. append-degraded transition emission for `DeliveryHarnessPath::NonClaude`
   fails closed inside `delivery_execution` because non-Claude append
   degradation is not a valid runtime concept
5. JSON-array inbox files fail closed from the normal Claude append path and can
  be rewritten only through the explicit repair/rebuild seam
6. the retained Claude append seam fails closed before execution if a
   `DeliveryHarnessPath::NonClaude` route is attempted; the low-level writer is
   not itself the place where that route is supposed to be discovered

### Module-ownership documentation

- one module family for state-machine planning/output
- one module family for execution
- one repair/rebuild-only module family

Required shape:

- `delivery_policy` / machine modules:
  - decide
  - emit typed plan
- `delivery_plan` module:
  - defines `DeliveryPlan`
  - defines `ReplyDeliveryPlan`
  - defines logical message and delivery-target DTOs
- execution modules:
  - perform payload delivery
  - perform notification
  - translate typed delivery outcomes into transition emissions
  - reject impossible target/execution combinations
- repair modules:
  - perform rebuild/reexport only

## Required QA Checks

- every approved low-level writer has an allowlist of legal callers
- every illegal direct caller is tested through lint, not just by convention
- state-machine tests prove:
  - same payload count across harness families
  - same payload ordering across harness families
  - same payload content across harness families
  - different delivery target only
