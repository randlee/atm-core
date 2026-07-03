# ADR-019 — Direct Post-Send Emission And Claude JSON Runtime Retirement

| Field | Value |
|---|---|
| ID | ADR-019 |
| Status | **Accepted** |
| Date | 2026-07-02 |
| Deciders | Rand Lee |
| Relates to | REQ-P-SEND-001, REQ-P-READ-001, REQ-P-RUNTIME-001, REQ-CORE-BOUNDARY-001, REQ-CORE-DAEMON-002 |
| Supersedes | ADR-010, ADR-013 |

---

## Context

The accepted runtime drifted away from the original ATM model in three ways:

- Claude Code no longer uses the Claude JSON mailbox path, but ATM still
  carries mailbox-JSON compatibility code, watcher/reconcile machinery, and
  related documentation as if that path were live
- post-send behavior is routed through `DeliveryPlan`,
  `NotificationSink`, and daemon notification-runtime machinery even though
  the desired model is simply "persist, then emit a post-send event if the
  recipient exposes that capability"
- daemon-owned watch/reconcile and notification worker subsystems now exist
  mostly to support retired Claude JSON/runtime assumptions rather than the
  retained ATM product surface

## Decision

ATM returns to the direct runtime model.

### 1. Claude JSON mailbox support is retired from the accepted runtime

- Claude JSON mailbox files are not part of the retained production mailbox
  path
- Claude inbox JSON append is not an approved delivery, mailbox,
  notification, or context-injection mechanism
- production ATM command behavior must not depend on Claude JSON mailbox
  ingest, export, watcher import, or rebuild flows

### 2. Reconcile/watch runtime is deleted

- `ReconcileRuntime` is not part of the accepted daemon architecture
- daemon-owned file-watch/debounce/reconcile machinery is retired with the
  Claude JSON mailbox path it was supporting
- no send/read/ack path may depend on reconcile completion, reconcile
  notifications, or watcher-owned filesystem import

### 3. Post-send is a direct post-persist emitter seam

The accepted model is:

- caller-owned command identity is resolved at the CLI boundary from explicit
  override when supported or invoking-shell `ATM_IDENTITY`
- if caller identity is unavailable, the CLI fails before daemon dispatch
- downstream caller-owned request DTOs carry caller identity as required data,
  and the daemon never substitutes daemon ambient identity
- `atm send` persists the message to durable ATM state
- if the recipient exposes a post-send hook capability, ATM emits one
  post-send event
- emission failure is logged and surfaced as a sender-visible warning
- `atm read` reads durable ATM state only

The accepted core seam is:

```rust
pub trait PostSendHookEmitter: sealed::Sealed {
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError>;
}
```

### 4. Notification logging, if retained, is direct append only

- `NotificationSink` is not the governing abstraction for post-send behavior
- if ATM keeps a notification log, event logging must be a direct append at
  the event site
- ATM must not keep a daemon queue/worker subsystem solely to append one
  notification record

### 5. Receiver-side handoff stays capability-specific

- local tmux-backed recipients use a local post-send emitter
- graft-backed recipients use the graft advisory/session handoff
- ATM owns emission, logging, and sender warning behavior
- ATM does not own receiver-side consumption after successful emission

## Consequences

### Positive

- send/read semantics are simple again
- sender warning ownership becomes explicit and testable
- Claude JSON mailbox and reconcile code can be deleted instead of maintained
- daemon architecture becomes thinner and easier to audit

### Negative

- historical compatibility docs and requirements must be rewritten
- runtime code that assumed `NotificationSink`, reconcile, or Claude JSON
  mailbox support must be removed
- any remaining operational dependency on Claude JSON mailbox artifacts must be
  replaced before release
