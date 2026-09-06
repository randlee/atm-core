# ADR-019 — Direct Post-Send Emission And Claude Backend Retirement

| Field | Value |
|---|---|
| ID | ADR-019 |
| Status | **Accepted** |
| Date | 2026-07-02 |
| Deciders | Rand Lee |
| Relates to | REQ-P-SEND-001, REQ-P-READ-001, REQ-P-RUNTIME-001, REQ-CORE-BOUNDARY-001, REQ-CORE-DAEMON-002 |
| Supersedes | ADR-010, ADR-013, ADR-018 §7 |

*Terminology note (Phase AQ): 'nudge' below means the steer (immediate) kind; see the nudge taxonomy in docs/requirements.md.*

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

### 1. Claude inbox-append runtime behavior is retired from the accepted runtime

- Claude inbox JSON append is not an approved delivery, mailbox,
  notification, or context-injection mechanism on the accepted runtime
- production ATM command behavior must not depend on mailbox JSON append,
  watcher import, or rebuild flows as the governing send/read/runtime path

### 2. The Claude backend is retired, but backend interoperability is not

- `atm-storage-claude` is retired from the accepted product architecture
  because Claude Code no longer uses that backend
- the accepted line must not continue to ship the `atm-storage-claude` crate or
  its boundary records as a production backend
- retiring `atm-storage-claude` does not retire the shared `atm-storage`
  semantic contracts
- SQLite remains one backend implementation, not the architecture
- future SQL backend support remains an explicit architectural requirement
- backend interoperability does not require multiple live concrete backends in
  every release; it requires that the shared contract still admits new
  backends without architectural reset
- no Phase AD change may collapse the architecture into a permanent
  SQLite-only/single-backend contract

### 3. Reconcile/watch runtime is deleted

- `ReconcileRuntime` is not part of the accepted daemon architecture
- daemon-owned file-watch/debounce/reconcile machinery is retired with the
  Claude JSON mailbox path it was supporting
- no send/read/ack path may depend on reconcile completion, reconcile
  notifications, or watcher-owned filesystem import

### 4. Post-send is a direct post-persist emitter seam

The accepted model is:

- caller-owned command identity is resolved at the CLI boundary from explicit
  override when supported or invoking-shell `ATM_IDENTITY`
- if caller identity is unavailable, the CLI fails before daemon dispatch
- downstream caller-owned request DTOs carry caller identity as required data,
  and the daemon never substitutes daemon ambient identity
- `atm send` persists the message to durable ATM state
- if the recipient exposes a post-send hook capability, ATM emits one
  post-send event
- the shipped default post-send path is the built-in in-process daemon-owned
  delivery path rather than a repo-local Python or shell script
- external `[[atm.post_send_hooks]]` commands remain the explicit override
  path when configured
- the built-in renderer is bounded to six named template kinds only:
  - `delivery`
  - `delivery_ack`
  - `delivery_task`
  - `delivery_task_ack`
  - `acknowledge`
  - `acknowledge_task`
- any team-scoped built-in template override lookup must cross the accepted
  storage-neutral `NudgeTemplateOverrideStore` contract upstream of
  `PostSendHookEmitter`; neither `atm` nor `atm-core` may perform direct
  SQLite lookup in the emitter path
- emission failure is logged and surfaced as a sender-visible warning
- `atm read` reads durable ATM state only

The accepted core seam is:

```rust
pub trait PostSendHookEmitter: sealed::Sealed {
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError>;
}
```

The accepted ownership rule around that seam is equally important:

- caller-owned send/ack logic decides whether the recipient exposes post-send
  capability
- caller-owned send/ack logic resolves external-hook matches, built-in fallback
  eligibility, and any team-scoped built-in override row before invoking the
  emitter
- `PostSendHookEmitter` performs the chosen recipient-side emission attempt
  only; it does not reopen config/store lookup or own caller-facing warning
  policy
- `GraftPostSendPort` is the receiver-specific leaf handoff used when the
  chosen built-in target is graft-backed

### AD.25-AD.30 closeout note

The accepted architecture above stayed fixed while implementation converged:

- `AD.26` made `PostSendHookEmitter` and `GraftPostSendPort` live on the
  production send/ack path and removed the subprocess bypass
- `AD.27` completed the remaining cleanup by moving retained built-in template
  resolution fully upstream of the live emitter path
- the hidden built-in helper, when invoked directly, consumes a resolved
  envelope only; no accepted implementation path is allowed to reopen
  template-override lookup below `PostSendHookEmitter`

### 5. Notification logging, if retained, is direct append only

- `NotificationSink` is not the governing abstraction for post-send behavior
- if ATM keeps a notification log, event logging must be a direct append at
  the event site
- ATM must not keep a daemon queue/worker subsystem solely to append one
  notification record

### 6. Receiver-side handoff stays capability-specific

- local tmux-backed recipients use a local post-send emitter
- graft-backed recipients use a graft receiver implementation behind the same
  post-send capability seam
- the concrete built-in sink names are `TmuxNudgeSink` and `GraftNudgeSink`
- graft receiver details such as host wakeup, temporary buffering, or
  active/inactive runtime state stay private to `atm-graft` and must not leak
  into shared daemon request/response families
- the accepted architecture does not require daemon-owned graft session
  registration, daemon-owned per-session nudge queues, or a dedicated shared
  advisory-stream packet family
- ATM owns emission, logging, and sender warning behavior
- ATM does not own receiver-side consumption after successful emission

## Consequences

### Positive

- send/read semantics are simple again
- sender warning ownership becomes explicit and testable
- Claude inbox-append delivery/context-injection assumptions and reconcile code
  can be deleted instead of maintained
- the shared backend contract survives even though the Claude backend is
  retired
- daemon architecture becomes thinner and easier to audit

### Negative

- historical compatibility docs and requirements must be rewritten
- runtime code that assumed `NotificationSink`, reconcile, or Claude JSON
  mailbox support must be removed
- storage/backend docs and boundary records must be updated so the architecture
  no longer claims a live Claude backend while still preserving future SQL
  readiness
- any remaining operational dependency on Claude inbox-append runtime artifacts
  must be replaced before release

## Amendment — Phase AX seven-kind queue template surface (2026-09-05)

Phase AX adds queue-specific built-in template kinds and retires the two
unreachable task-steer kinds. The complete seven-kind inventory is:

- `delivery`
- `delivery_ack`
- `queue`
- `queue_ack`
- `task`
- `acknowledge`
- `acknowledge_task`

`NudgeKind` selects the delivery or queue family. Task-tagged messages always
select `task` and are deferred on every backend. The former
`delivery_task` and `delivery_task_ack` values are rejected on input with a
recovery hint to use `task`.

Existing SQLite override tables are rebuilt on database open when their
constraint does not include `queue`; accepted rows are copied, retired rows
are dropped with a warning naming the team and kind, and unknown rows fail the
open loudly. Fresh databases use the seven-value constraint directly.
