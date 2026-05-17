# Phase Y Inbox Write Path Audit

Baseline:

- planning branch: `feature/pY-s0-planning`
- implementation branch sampled: `feature/pY-trivial-fixes` at `31373a1`

## 1. Current Production ATM-Authored Claude-Inbox Write Paths

### Path A — Command Send Path

Code:

- `crates/atm-core/src/send/mod.rs::append_mailbox_message_and_seed_workflow(...)`
- callers:
  - normal send path
  - missing-config notice path

Behavior:

- load the current mailbox projection from SQLite metadata/records
- prepare threading/supersede state against that projected mailbox
- rewrite the full compatibility inbox file through
  `mailbox::store::write_compat_mailbox_projection(...)`
- mirror the new message into the SQLite store
- persist workflow state in the same coordinated write block

Current assessment:

- this is a direct command-layer compatibility inbox write path
- this path should not survive the final Phase `Y` boundary

### Path B — Command Ack Reply Path

Code:

- `crates/atm-core/src/ack/mod.rs`
- reply emission calls `append_mailbox_message_and_seed_workflow(...)`

Behavior:

- update the acked message state in SQLite
- emit the reply message through the same compatibility inbox rewrite helper
  used by send

Current assessment:

- this is not a separate low-level writer, but it is a separate production
  command path that can trigger a compatibility inbox rewrite
- this path should not survive the final Phase `Y` boundary

### Path C — Compatibility Export / Source Projection Path

Code:

- `crates/atm-core/src/boundary_support.rs::export_source_files(...)`
- `crates/atm-core/src/mailbox/mod.rs::export_compat_source_projections(...)`
- `crates/atm-core/src/mailbox/store.rs::write_compat_source_projections(...)`
- `crates/atm-core/src/mailbox/store.rs::write_compat_mailbox_projection(...)`

Behavior:

- accept already-loaded source projections
- write the compatibility inbox projection through the mailbox owner layer

Current assessment:

- this is the path that most closely matches the intended daemon-private
  watcher/import/export ownership model
- this is the best candidate to survive as the sole normal runtime writer
  shape after boundary cleanup

### Path D — Team-Admin Inbox Creation Path

Code:

- `crates/atm-core/src/team_admin.rs::add_member(...)`
- `crates/atm-core/src/team_admin.rs::ensure_inbox_exists(...)`

Behavior:

- create a new inbox file for a newly added member
- use `OpenOptions::new().write(true).create_new(true)` to create the inbox
  without rewriting an existing mailbox
- used by admin/recovery flows, not by normal `send` / `read` / `ack` /
  `clear` runtime flow

Current assessment:

- this is an approved exceptional inbox-write path
- it belongs to the retained admin boundary, not to the normal runtime
  compatibility export owner
- the original Phase `Y` framing focused on normal runtime writes; this audit
  expands scope deliberately so approved admin inbox creation is documented and
  auditable too

### Path E — Team Config Write Paths

Code:

- `crates/atm-core/src/team_admin.rs::write_team_config(...)`
- `crates/atm-core/src/team_admin/restore.rs`

Behavior:

- rewrite `.claude/teams/<team>/config.json` atomically
- used by team-admin membership mutation and restore/recovery paths
- not used by normal `send` / `read` / `ack` / `clear` runtime flow

Current assessment:

- `config.json` writes are not watcher-owned today
- these are admin/recovery write paths and should stay explicitly separate from
  normal runtime inbox export ownership in Phase `Y`

## 2. Current Write Semantics

- ATM-authored compatibility inbox writes are currently full-file atomic
  rewrites, not append-only writes
- the current writer emits one JSON array document, not one appended JSONL
  record per write
- workflow + mailbox writes are still lock-coordinated through
  `workflow::commit_workflow_state(...)`
- the low-level Claude-surface writer is `mailbox::atomic::write_messages(...)`,
  which serializes the full mailbox projection and atomically replaces the file
- with the current array-shaped wire format, append-only lock-free writes are
  not available without a compatibility-contract change

## 3. Agreed Phase Y Runtime Rules

- JSONL append is a `Claude Code` harness output only; harness type, not model,
  decides whether a compatibility append is allowed
- harness routing must be resolved from canonical roster `harness` truth, not
  from ad hoc command inputs or model strings
- non-`Claude Code` harnesses must never receive ATM-authored JSONL appends
- multiple command/daemon flows may persist messages into SQLite, but only one
  Claude-compatible append path is allowed after that write flow completes
- the compatibility append is a Claude-facing notification/output path and must
  not become the mutable source of truth
- the final normal runtime write owner must sit behind one hard boundary and
  must not be directly callable from arbitrary command code
- the branch between Claude and non-Claude behavior must live in one central
  delivery-policy coordinator plus explicit event-family state machines
- `NewMessageStateMachine` and `ThreadUpdateStateMachine` are separate by
  design; they must not be collapsed into one generic send machine
- normal runtime append behavior is:
  - SQLite inbox write completes
  - one owned Claude-Code-only append path runs
  - one nudge path runs
- SQLite failure behavior is intentionally stricter than a normal degradation:
  - for `Claude Code` harnesses:
    - the original message is still appended to the Claude inbox
    - ATM also appends a second error message from `atm-system@<team>`
  - for non-Claude harnesses:
    - the original message still proceeds through the non-Claude delivery path
    - ATM also emits a second error message through that same path
  - the nudge/notification path mirrors that two-message behavior
  - no alternate fallback path is allowed in place of that companion error
    message
- if the Claude Code append itself fails, the fallback notification path is the
  configured post-send-hook path, not a second ad hoc notification mechanism

## 4. Final Allowed Write Classes

Phase `Y` should converge on only these approved ATM-authored Claude-inbox
write classes:

1. append one message to a Claude Code inbox
   - daemon-private or tightly watcher-owned
   - synchronized inside the owned writer subsystem
   - must skip non-Claude harnesses for normal runtime compatibility writes
   - append-only if and only if the approved compatibility wire contract
     supports it

2. bulk mailbox creation / rebuild for a new or repaired inbox
   - explicit admin / restore / repair path
   - may project a bounded historical message set such as the last 24 hours of
     non-deleted messages into a newly created inbox
   - not part of normal runtime command flow
   - staged and atomic by design

## 5. Line-Numbered Runtime Write Ledger

### Remove Or Move From Normal Command Ownership

1. Send command compatibility rewrite stack
   - caller: `crates/atm-core/src/send/mod.rs:280`
     - `append_mailbox_message_and_seed_workflow(...)`
     - status: delete command ownership in `Y.3`
   - missing-config caller: `crates/atm-core/src/send/mod.rs:463`
     - `append_mailbox_message_and_seed_workflow(...)`
     - status: delete command ownership in `Y.3`
   - shared helper: `crates/atm-core/src/send/mod.rs:482`
     - `append_mailbox_message_and_seed_workflow(...)`
     - status: delete or reduce to pure SQLite/workflow helper in `Y.3`
   - projection loader: `crates/atm-core/src/send/mod.rs:516`
     - `load_store_backed_mailbox_projection(...)`
     - status: delete from the runtime write stack in `Y.3`; retain only if a
       non-write read/projection use remains justified separately
   - SQLite mirror helper: `crates/atm-core/src/send/mod.rs:548`
     - `mirror_message_to_store(...)`
     - status: retain or move as SQLite-only persistence helper after inbox
       rewrite ownership is removed
   - lock owner: `crates/atm-core/src/workflow.rs:166`
     - `commit_workflow_state(...)`
     - status: remove inbox-file ownership from this stack in `Y.3`
   - mailbox projection writer: `crates/atm-core/src/mailbox/store.rs:19`
     - `write_compat_mailbox_projection(...)`
     - status: command stack must stop calling this in `Y.3`
   - mailbox projection policy helper: `crates/atm-core/src/mailbox/store.rs:27`
     - `write_compat_mailbox_projection_with_policy(...)`
     - status: command stack must stop reaching this helper through mailbox
       projection writes in `Y.3`
   - low-level serializer: `crates/atm-core/src/mailbox/atomic.rs:28`
     - `write_messages(...)`
     - status: retained only behind the surviving owner boundary or deleted in
       `Y.6` if append-only cutover replaces array rewrite

2. Ack reply compatibility rewrite stack
   - caller: `crates/atm-core/src/ack/mod.rs:391`
     - `append_mailbox_message_and_seed_workflow(...)`
     - status: delete command ownership in `Y.3`
   - shared helper: `crates/atm-core/src/send/mod.rs:482`
     - `append_mailbox_message_and_seed_workflow(...)`
     - status: same removal target as send path
   - projection loader: `crates/atm-core/src/send/mod.rs:516`
     - `load_store_backed_mailbox_projection(...)`
     - status: same removal target as send path
   - SQLite mirror helper: `crates/atm-core/src/send/mod.rs:548`
     - `mirror_message_to_store(...)`
     - status: same retention/move target as send path
   - lock owner: `crates/atm-core/src/workflow.rs:166`
     - `commit_workflow_state(...)`
     - status: remove inbox-file ownership from this stack in `Y.3`
   - mailbox projection writer: `crates/atm-core/src/mailbox/store.rs:19`
     - `write_compat_mailbox_projection(...)`
     - status: ack stack must stop calling this in `Y.3`
   - mailbox projection policy helper: `crates/atm-core/src/mailbox/store.rs:27`
     - `write_compat_mailbox_projection_with_policy(...)`
     - status: ack stack must stop reaching this helper through mailbox
       projection writes in `Y.3`
   - low-level serializer: `crates/atm-core/src/mailbox/atomic.rs:28`
     - `write_messages(...)`
     - status: retained only behind surviving owner boundary or deleted in
       `Y.6`

### Retain Behind One Owned Boundary

1. Daemon/runtime export path
   - public bridge: `crates/atm-core/src/direct_boundaries.rs:38`
     - `export_source_files(...)`
     - status: move/retain as the sole normal runtime owner entrypoint
   - private bridge: `crates/atm-core/src/boundary_support.rs:147`
     - `export_source_files(...)`
     - status: retain only if daemon-private and harness-gated
   - projection wrapper: `crates/atm-core/src/mailbox/mod.rs:137`
     - `export_compat_source_projections(...)`
     - status: retain as owned export helper or fold into new owner
   - source projection writer: `crates/atm-core/src/mailbox/store.rs:36`
     - `write_compat_source_projections(...)`
     - status: retain behind one owner in `Y.3`; reevaluate in `Y.6`
   - mailbox projection policy helper: `crates/atm-core/src/mailbox/store.rs:27`
     - `write_compat_mailbox_projection_with_policy(...)`
     - status: retain only behind the sole runtime owner until `Y.6`
   - low-level serializer: `crates/atm-core/src/mailbox/atomic.rs:28`
     - `write_messages(...)`
     - status: retained only until append-only cutover lands

### Retain As Notification / Fallback Side-Effect Stack

1. Notification sink stack
   - runtime trait entrypoint: `crates/atm-core/src/service_runtime.rs:44`
     - `maybe_run_post_send_hook(...)`
     - status: retain as side-effect interface only; do not let it own event
       legality
   - runtime implementation: `crates/atm-core/src/service_runtime.rs:143`
     - `maybe_run_post_send_hook(...)`
     - status: retain as runtime bridge only
   - send façade: `crates/atm-core/src/send/mod.rs:705`
     - `maybe_run_post_send_hook(...)`
     - status: retain only as thin façade or inline into the eventual
       notification sink boundary
   - hook executor: `crates/atm-core/src/send/hook.rs:57`
     - `hook::maybe_run_post_send_hook(...)`
     - status: retain as notification fallback executor only; do not let it
       decide harness routing or SQLite failure policy
   - hook child-process supervisor: `crates/atm-core/src/send/hook.rs:90`
     - `execute_post_send_hook(...)`
     - status: retain under NotificationSink side-effect ownership only

2. Explicit re-export / repair path
   - public bridge: `crates/atm-core/src/direct_boundaries.rs:44`
     - `reexport_messages(...)`
     - status: retain as explicit repair/admin path only
   - private bridge: `crates/atm-core/src/boundary_support.rs:169`
     - `reexport_messages(...)`
     - status: retain as repair/rebuild owner only
   - projection wrapper: `crates/atm-core/src/mailbox/mod.rs:143`
     - `export_compat_mailbox_projection(...)`
     - status: retain for repair/rebuild only, not normal send/ack
   - mailbox projection writer: `crates/atm-core/src/mailbox/store.rs:19`
     - `write_compat_mailbox_projection(...)`
     - status: after `Y.3`, reachable only from repair/rebuild owner
   - mailbox projection policy helper: `crates/atm-core/src/mailbox/store.rs:27`
     - `write_compat_mailbox_projection_with_policy(...)`
     - status: after `Y.3`, reachable only from repair/rebuild owner

### Retain As Admin / Repair Exceptions

1. New inbox creation
   - caller: `crates/atm-core/src/team_admin.rs:313`
     - `ensure_inbox_exists(...)`
     - status: retain as explicit admin/create path
   - creator: `crates/atm-core/src/team_admin.rs:455`
     - `ensure_inbox_exists(...)`
     - status: retain; do not route normal runtime send/ack here

2. Team config writes
   - add-member caller: `crates/atm-core/src/team_admin.rs:333`
     - `write_team_config(...)`
     - status: retain as admin boundary
   - restore caller: `crates/atm-core/src/team_admin/restore.rs:97`
     - `super::write_team_config(...)`
     - status: retain as restore boundary
   - writer: `crates/atm-core/src/team_admin.rs:486`
     - `write_team_config(...)`
     - status: retain as sole `config.json` write owner unless a later sprint
       moves all config writes under a narrower admin subsystem
   - atomic config helper: `crates/atm-core/src/team_admin.rs:492`
     - `atomic_write(...)`
     - status: retain behind `write_team_config(...)`

### Adjacent Restore-State Writers That Are Not Inbox Paths

- `crates/atm-core/src/team_admin/restore.rs:339`
  - `recompute_highwatermark(...)`
  - status: retain under restore/task-state boundary
- `crates/atm-core/src/team_admin/restore.rs:426`
  - `prepare_restore_workspace(...)`
  - status: retain under restore boundary

## 6. Mechanical Completeness Checks

The line-numbered ledger above is only acceptable if these source queries stay
consistent with the sampled implementation branch.

Production caller census used for this audit:

```bash
rg -n "append_mailbox_message_and_seed_workflow|write_compat_mailbox_projection\\(|write_compat_mailbox_projection_with_policy\\(|write_compat_source_projections\\(|export_source_files\\(|reexport_messages\\(|ensure_inbox_exists\\(|write_team_config\\(|maybe_run_post_send_hook\\(|execute_post_send_hook\\(" \
  crates/atm-core/src
```

Required review rule:

- if this query returns a new production call site not already classified in
  Sections 5.1 through 5.4, that is a planning miss and must be treated as a
  blocking finding before the corresponding sprint starts

Known non-production exclusions:

- `crates/atm-core/src/mailbox/mod.rs:76`
  - `store::write_compat_mailbox_projection(...)`
  - classification: `#[cfg(test)]` helper only; not part of the production
    write/removal ledger
- `crates/atm-core/src/team_admin.rs:639`
  - local `write_team_config(...)` test helper
  - classification: unit-test helper only
- `crates/atm-core/src/team_admin/restore.rs:577`
  - local `write_team_config(...)` test helper
  - classification: unit-test helper only

Normal runtime completion rule for `Y.3`:

- after command-owned rewrite removal, the same caller census must show no
  `send` or `ack` production path reaching:
  - `append_mailbox_message_and_seed_workflow(...)`
  - `write_compat_mailbox_projection(...)`
  - `write_compat_mailbox_projection_with_policy(...)`
- `crates/atm-core/src/team_admin/restore.rs:450`
  - `apply_restored_inboxes(...)`
  - status: retain as bulk inbox rebuild/install owner
- `crates/atm-core/src/team_admin/restore.rs:506`
  - `restore_task_state_from_backup(...)`
  - status: retain under restore boundary
- `crates/atm-core/src/team_admin/restore.rs:514`
  - `write_restore_marker(...)`
  - status: retain under restore boundary
- `crates/atm-core/src/team_admin/restore.rs:531`
  - `clear_restore_marker(...)`
  - status: retain under restore boundary

## 6. Write Paths To Eliminate

- direct command-path compatibility inbox rewrites from `send`
- direct command-path compatibility inbox rewrites from `ack`
- any future mailbox rewrite helper reachable from arbitrary command code
- any normal-runtime `config.json` write path outside an explicit admin /
  restore / repair boundary

## 7. Open Phase Y Questions

- whether `source_team` and the ack/thread/task fields are still justified on
  the shared inbox surface once SQLite is the sole mutable truth
- whether the current array-shaped ATM-authored compatibility output remains an
  approved contract or should be retired in favor of append-only JSONL
- whether the watcher/import/export subsystem itself should own both approved
  inbox-write classes, or whether the bulk-mailbox-creation path should remain
  a separately owned admin/repair boundary
- whether any shared-inbox mutable ATM field still has a legitimate
  compatibility role
