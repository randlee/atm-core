# `atm-core::workflow`

Owns the ATM-managed workflow sidecar for mailbox messages:
`.claude/teams/<team>/.atm-state/workflow/<agent>.json`.

Primary ownership note:
- this module owns the workflow sidecar file family as transitional
  compatibility state during Phase Q
- SQLite-backed mail, ack, visibility, and task state is the target durable
  truth for Phase Q mail correctness
- `workflow::project_envelope(...)` is the only shared projection helper for
  joining Claude-owned inbox records with ATM-owned workflow state
- `workflow::save_workflow_state(...)` is the only owner-layer persistence
  entry point for the workflow sidecar file family
- callers must not shape workflow JSON directly at the command layer
- messages without a stable ATM identity remain compatibility-only and may
  still rely on legacy inbox-local fields until a later enrichment phase lands
- inbox ingress currently imports workflow-sidecar state into SQLite as part of
  the transition line; that import path is why the sidecar still exists after
  Phase Q durable-state cutover work
- current limitation: send-side seeding still reaches this module through an
  atomic `load -> mutate -> save` sequence, so concurrent same-recipient sends
  are not yet protected by a dedicated freshness helper
- P.6 is the tracked hardening item to introduce that freshness boundary
- review-sensitive corner cases for this module are:
  - two ATM-authored sends race to seed distinct message ids for the same
    recipient sidecar file
  - one sender wins the atomic rename while another must reload and preserve
    the winning entry before adding its own
  - malformed sidecar JSON must fail with explicit diagnostics rather than
    silently resetting workflow state

Supersession note:
- older wording in this document that called the sidecar the ATM-owned durable
  source of truth is obsolete for the Phase Q target architecture

References:

- Product requirements: `docs/requirements.md` §3.2.2, §14, and §20.2
- Architecture: `docs/architecture.md` §5 and §18.4.3
- Message schema: `docs/atm-message-schema.md` §3
