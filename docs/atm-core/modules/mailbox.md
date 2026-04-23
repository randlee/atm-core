# `atm-core::mailbox`

Owns mailbox file discovery, atomic read/write helpers, locking, duplicate
suppression, and origin-inbox merge primitives.

Primary ownership note:
- mailbox code must distinguish:
  - read-only snapshot helpers
  - read-possible-write flows
  - true read-modify-write flows
- mailbox writes must flow through one owner-layer write boundary rather than
  ad hoc call-site persistence logic
- the concrete mailbox helper boundaries are
  `mailbox::store::observe_source_files(...)` for lock-free snapshots,
  `mailbox::store::commit_source_mutation(...)` for shared read/ack/clear
  writeback orchestration,
  `mailbox::store::commit_mailbox_state(...)` for one file, and
  `mailbox::store::commit_source_files(...)` for multi-source persistence
- current shared-inbox rewrite behavior is a compatibility boundary over a
  Claude-owned surface, not a general license to store new ATM-local source of
  truth in Claude-owned files
- the mailbox append boundary owns the atomic sender-scoped idle-notification
  dedup-and-replace rule: when a newly appended message is classified as an
  idle notification, remove any older unread idle notification from the same
  sender in the same inbox and append the new record in one atomic sequence
- this behavior satisfies `REQ-P-IDLE-001` through `REQ-CORE-MAILBOX-001`

References:

- Product requirements: `docs/requirements.md` §3.2 and §14
- `REQ-P-CONTRACT-001`
- `REQ-P-IDLE-001` (sender-scoped idle-notification dedup)
- `REQ-P-WORKFLOW-001`
- `REQ-CORE-MAILBOX-001`
- Migration artifact: `docs/archive/file-migration-plan.md`
