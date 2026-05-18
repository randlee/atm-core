---
id: Y.12
title: Claude Degraded Delivery Set Closure
status: planned
branch: feature/pYc-s12-claude-degraded-delivery-set-closure
worktree: ../atm-core-worktrees/feature/pYc-s12-claude-degraded-delivery-set-closure
target: integrate/phase-Y
---

# Sprint Y.12 — Claude Degraded Delivery Set Closure

## Goal

- close the final behavioral gap in the Claude compatibility delivery path
- make the SQLite-failure recovered Claude path land at a production-ready
  level
- prohibit partial `message[1]` success with missing `message[2]` while still
  claiming delivery success

## Hard Dependencies

- `integrate/phase-Y` at `4d6bd883` is the authoritative implementation
  baseline
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- `docs/phase-Yc/plan-phase-Yc.md`
- the existing `Yb` line is a dependency, not a reopen target
- `Y.12` is the first implementation sprint in the new `Yc` line

## Exact Targets

- `crates/atm-core/src/delivery_execution.rs`
- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/direct_boundaries.rs`
- `crates/atm-core/src/boundary_support.rs`
- `crates/atm-core/src/mailbox/atomic.rs`
- `crates/atm-core/src/mailbox/store.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/direct_boundaries.rs`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims. If that cannot be done cleanly in one sprint, the
sprint must be split before implementation begins. No deliverable may be
silently dropped or partially deferred.

- the Claude compatibility path owns one explicit logical-message-set delivery
  seam for `DeliveryPlanDisposition::SqliteFailedRecovered`
- the recovered Claude path either materializes the full logical message set or
  returns a hard error; it must not warn-and-continue after a partial outward
  append
- the implementation exposes one explicit batch/message-set compatibility export
  contract instead of hiding the behavior in `execute_claude_delivery(...)`
- named tests prove that `message[1]` and `message[2]` are treated as one
  behavioral unit on the recovered Claude path
- the plan names the translation seam from
  `DeliveryPlanDisposition::SqliteFailedRecovered` into the recovered Claude
  compatibility export mode explicitly, rather than leaving that mapping
  implicit in executor control flow

## Required Work

- replace the current per-message recovered-path append loop in
  `execute_claude_delivery(...)` with one explicit message-set execution seam
- add the necessary `InboxExport`/runtime helper surface so the Claude
  compatibility path can materialize the recovered logical message set through
  one owned contract
- narrow the `service_runtime.rs` role to the runtime-facing helper seam that
  forwards the recovered message-set request into `InboxExport`; it must not
  retain per-message recovered-path policy
- keep normal persisted Claude append delivery explicit and separate from the
  recovered message-set path; do not reopen silent full-mailbox rewrite on the
  normal path
- update the daemon adapter and low-level mailbox helpers only as far as needed
  to support the owned recovered message-set seam
- update ADR/boundary docs so the recovered Claude message-set rule is
  documented as an explicit contract, not an inferred behavior

## Paths To Delete

- `crates/atm-core/src/delivery_execution.rs`
  - delete the recovered-path behavior that loops over `messages` and mutates
    `AppendDegraded` warning state one message at a time inside
    `execute_claude_delivery(...)`
  - delete the recovered-path `break` behavior that allows `message[1]` to
    append, `message[2]` to fail, and the executor to return without a hard
    delivery failure
- `crates/atm-core/src/delivery_execution.rs`
  - delete the blanket `ClaudeInboxWriter` implementation detail that only
    exposes one-message append semantics through
    `self.append_compat_inbox_message(inbox_path, message)`
    when the active disposition is `SqliteFailedRecovered`
  - replace the recovered-path branch rather than extending it in parallel;
    the persisted one-message append path survives, but the recovered one
    message-at-a-time implementation must be removed

## Approved Surviving Paths

- persisted Claude append delivery may continue to use:
  - `RetainedServiceRuntime::append_compat_inbox_message(...)`
  - `crate::mailbox::store::append_compat_mailbox_message(...)`
- explicit repair/rebuild projection may continue to use:
  - `RetainedServiceRuntime::rebuild_compat_inbox_projection(...)`
  - `crate::direct_boundaries::reexport_messages(...)`
- the new recovered Claude path must survive only as one explicit owned
  message-set seam documented in `InboxExport` and consumed by the shared
  delivery executor

## Explicit Code Samples

If the sprint introduces or changes important traits, features, enums, protocol
types, boundary contracts, or execution seams, this section must include
explicit code samples or signatures showing the intended end state.

```rust
pub enum ClaudeCompatibilityDeliveryMode {
    RecoveredLogicalMessageSet,
}

pub struct InboxExportAppendMessageSetRequest {
    pub path: PathBuf,
    pub messages: Vec<MessageEnvelope>,
    pub mode: ClaudeCompatibilityDeliveryMode,
}

pub struct InboxExportAppendMessageSetResponse {
    pub wrote_messages: usize,
}

pub trait InboxExport: sealed::Sealed {
    fn export_record(
        &self,
        request: InboxExportRecordRequest,
    ) -> Result<InboxExportRecordResponse, AtmError>;

    fn reexport_message(
        &self,
        request: InboxExportReexportMessageRequest,
    ) -> Result<InboxExportReexportMessageResponse, AtmError>;

    fn append_message_set(
        &self,
        request: InboxExportAppendMessageSetRequest,
    ) -> Result<InboxExportAppendMessageSetResponse, AtmError>;
}
```

```rust
fn claude_compatibility_delivery_mode_for_disposition(
    disposition: DeliveryPlanDisposition,
) -> Result<ClaudeCompatibilityDeliveryMode, AtmError>;
```

```rust
pub(crate) trait ClaudeInboxWriter {
    fn append_claude_message_set(
        &self,
        inbox_path: &Path,
        recipient: &DeliveryRecipientSnapshot,
        disposition: DeliveryPlanDisposition,
        messages: &[LogicalMessage],
    ) -> Result<(), AtmError>;
}
```

```rust
fn execute_claude_delivery<R: ClaudeInboxWriter + ?Sized>(
    runtime: &R,
    disposition: DeliveryPlanDisposition,
    inbox_path: &Path,
    recipient: &DeliveryRecipientSnapshot,
    messages: &[LogicalMessage],
    result: &mut DeliveryExecutionResult,
) -> Result<(), AtmError>;
```

## Error Inventory

- `InboxExport::append_message_set(...)` returns `AtmError` when:
  - the recovered logical message set cannot be projected to the compatibility
    inbox/export surface at all
  - the target inbox/export path is unavailable or invalid for recovered
    export
  - the export fails before the full logical message set is materialized
- recovered Claude export must not degrade a partial `message[1]` write into an
  `AppendDegraded` warning and continue
- persisted append degradation remains separate:
  - it may still surface the existing typed append warning on the persisted
    append path
  - it must not be reused as proof that recovered logical-message-set export is
    allowed to partially succeed
- `claude_compatibility_delivery_mode_for_disposition(...)` returns
  `AtmError::validation(...)` with `AtmErrorCode::MessageValidationFailed`
  when called with any `DeliveryPlanDisposition` other than
  `DeliveryPlanDisposition::SqliteFailedRecovered`; non-recovered dispositions
  are invalid inputs for the recovered Claude message-set mapping seam

## This Sprint Does Not Close

- the `NotificationSink` boundary bypass in the post-send notification path
- final integrated production-readiness sign-off for the whole `Phase Y` line
- any new smoke/dogfood execution work in `Phase Z`
- post-mortem lint recommendations or rule additions from
  `integrate/phase-Y/.triage/phase-Yb/post-mortem.md`

## Acceptance Criteria

- `rg -n "if disposition == DeliveryPlanDisposition::SqliteFailedRecovered \\{[[:space:]]*break;" crates/atm-core/src/delivery_execution.rs`
  returns no matches
- `rg -n "append_claude_inbox_message\\(inbox_path, recipient, &message\\.envelope\\)" crates/atm-core/src/delivery_execution.rs`
  no longer shows the recovered Claude path implemented as a one-message loop
- the recovered Claude path no longer reports success after a partial outward
  logical-message-set append
- the sprint introduces exactly one approved message-set compatibility export
  seam for the recovered Claude path, and that seam is documented in both
  `atm-core` and `atm-daemon` boundary docs
- named tests prove all-or-nothing recovered Claude logical-message-set
  behavior:
  - `sqlite_failure_for_claude_requires_full_logical_message_set_delivery`
  - `sqlite_failure_for_claude_does_not_emit_message1_without_message2`
  - `persisted_claude_append_degradation_remains_explicit_and_warning_typed`
    must prove that the persisted append-only path still emits the existing
    typed warning entry on append degradation and never silently drops the
    failure
- no acceptance criterion relies on “shared shape” alone; the runtime behavior
  must be proven by the tests above

## Required Validation

- `rg -n "if disposition == DeliveryPlanDisposition::SqliteFailedRecovered \\{[[:space:]]*break;" crates/atm-core/src/delivery_execution.rs`
- `rg -n "append_claude_inbox_message\\(inbox_path, recipient, &message\\.envelope\\)" crates/atm-core/src/delivery_execution.rs`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
