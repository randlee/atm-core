# `atm-core::mailbox`

Historical compatibility scope:
- this module documents retained mailbox file discovery, atomic read/write
  helpers, locking, duplicate suppression, and origin-inbox merge primitives
  from the earlier Claude inbox compatibility line
- accepted ATM runtime durability is store-backed after `ADR-019`

Primary ownership note:
- mailbox code must distinguish:
  - read-only snapshot helpers
  - read-possible-write flows
  - true read-modify-write flows
- mailbox writes must flow through one owner-layer write boundary rather than
  ad hoc call-site persistence logic
- the concrete mailbox helper boundaries are
  `mailbox::store::observe_source_files(...)` for lock-free snapshots,
  `mailbox::store::with_locked_source_files(...)` for shared read/ack/clear
  lock+reload orchestration,
  `mailbox::store::commit_mailbox_state(...)` for one file, and
  `mailbox::store::commit_source_files(...)` for multi-source persistence
- ATM-owned mailbox workflow durability is not owned by `mailbox`; it lives in
  `workflow.rs` and is joined onto the Claude-owned inbox surface by the
  higher-level read/ack/clear services
- historical shared-inbox rewrite behavior was a compatibility boundary over a
  Claude-owned surface, not a general license to store new ATM-local source of
  truth in Claude-owned files
- on the earlier compatibility line, the mailbox append boundary owned the
  atomic sender-scoped idle-notification dedup-and-replace rule: when a newly
  appended message was classified as an idle notification, remove any older
  unread idle notification from the same sender in the same inbox and append
  the new record in one atomic sequence
- that historical behavior satisfied the sender-scoped idle-notification dedup
  contract in `docs/requirements.md` alongside `REQ-CORE-MAILBOX-001`
- mailbox ownership stopped at the Claude-owned inbox compatibility surface; a
  mailbox append that implied workflow-sidecar seeding handed off to the
  workflow owner boundary rather than persisting sidecar JSON itself
- review-sensitive corner cases for this boundary are:
  - `read` observational snapshot differs from the eventual under-lock reread
  - `ack` reply-target expansion requires a larger final lock set than the
    unlocked preflight saw
  - `clear --dry-run` must remain observational while mutating `clear` uses the
    shared lock+reload+persist path

References:

- Product requirements: `docs/requirements.md` §3.2 and §14
- `REQ-P-CONTRACT-001`
- `REQ-P-WORKFLOW-001`
- `REQ-CORE-MAILBOX-001`
- Migration artifact: `docs/archive/file-migration-plan.md`
