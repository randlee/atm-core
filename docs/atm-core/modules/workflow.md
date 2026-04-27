# `atm-core::workflow`

Owns the ATM-managed workflow sidecar for mailbox messages:
`.claude/teams/<team>/.atm-state/workflow/<agent>.json`.

Primary ownership note:
- this module is the ATM-owned source of truth for mailbox-local workflow
  durability when a message has a stable ATM identity
- `workflow::project_envelope(...)` is the only shared projection helper for
  joining Claude-owned inbox records with ATM-owned workflow state
- `workflow::save_workflow_state(...)` is the only owner-layer persistence
  entry point for the workflow sidecar file family
- callers must not shape workflow JSON directly at the command layer
- messages without a stable ATM identity remain compatibility-only and may
  still rely on legacy inbox-local fields until a later enrichment phase lands
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

References:

- Product requirements: `docs/requirements.md` §14 and §18
- Architecture: `docs/architecture.md` §5 and §18.4.3
- Message schema: `docs/atm-message-schema.md` §3
