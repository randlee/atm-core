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

### sc-lint / boundary rules

1. only the approved Claude executor module may call:
   - `RetainedServiceRuntime::append_compat_inbox_message(...)`
   - `mailbox::store::append_compat_mailbox_message(...)`
2. only approved repair/rebuild modules may call:
   - `mailbox::store::write_compat_mailbox_projection(...)`
   - `direct_boundaries::reexport_messages(...)`
3. `send/persistence.rs` must not call:
   - any compatibility append/write primitive
   - any post-send notification primitive
4. `send/mod.rs` and `ack/mod.rs` must not:
   - branch on `DeliveryHarnessPath`
   - branch on `allows_claude_jsonl_append()`
   - translate persistence dispositions into state-machine outcomes
5. `send/hook.rs` must not:
   - accept full `MessageEnvelope` delivery authority
   - become a second outbound payload boundary

### Module-ownership documentation

- one module family for state-machine planning/output
- one module family for execution
- one repair/rebuild-only module family

Required shape:

- `delivery_policy` / machine modules:
  - decide
  - emit typed plan
- execution modules:
  - perform payload delivery
  - perform notification
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

