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

- `crates/atm-core/src/boundary_support.rs::export_compat_source_projections(...)`
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

### Path D — Team Config Write Paths

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

## 3. Final Allowed Write Classes

Phase `Y` should converge on only these ATM-authored Claude-inbox write
classes:

1. normal runtime compatibility export
   - daemon-private or tightly watcher-owned
   - synchronized inside the owned writer subsystem
   - append-only if and only if the approved compatibility wire contract
     supports it

2. explicit admin / restore / repair staging path
   - not part of normal runtime command flow
   - staged and atomic by design

## 4. Write Paths To Eliminate

- direct command-path compatibility inbox rewrites from `send`
- direct command-path compatibility inbox rewrites from `ack`
- any future mailbox rewrite helper reachable from arbitrary command code
- any normal-runtime `config.json` write path outside an explicit admin /
  restore / repair boundary

## 5. Open Phase Y Questions

- whether `source_team` and the ack/thread/task fields are still justified on
  the shared inbox surface once SQLite is the sole mutable truth
- whether the current array-shaped ATM-authored compatibility output remains an
  approved contract or should be retired in favor of append-only JSONL
- whether the watcher/import/export subsystem itself should own the writer, or
  whether a smaller daemon-private compatibility-export sub-boundary should sit
  beside it
- whether any shared-inbox mutable ATM field still has a legitimate
  compatibility role
