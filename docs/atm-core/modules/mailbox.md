# `atm-core::mailbox`

Owns mailbox file discovery, atomic read/write helpers, locking, duplicate
suppression, and origin-inbox merge primitives.

Primary ownership note:
- the mailbox append boundary owns the atomic sender-scoped idle-notification
  dedup-and-replace rule: when a newly appended message is classified as an
  idle notification, remove any older unread idle notification from the same
  sender in the same inbox and append the new record in one atomic sequence
- this behavior satisfies `REQ-P-IDLE-001` through `REQ-CORE-MAILBOX-001`

References:

- Product requirements: `docs/requirements.md` §3.2 and §12
- `REQ-P-CONTRACT-001`
- `REQ-P-IDLE-001` (sender-scoped idle-notification dedup)
- `REQ-P-WORKFLOW-001`
- `REQ-CORE-MAILBOX-001`
- Migration artifact: `docs/file-migration-plan.md`
