# Phase U — Mailbox Simplification And Identity Cleanup

Goal:
- remove unapproved or confusing mailbox, identity, roster, and task-carrying
  design from the current ATM line before SQLite/store contracts are treated as
  settled
- keep SQLite as the only ATM-owned mailbox state/query authority outside the
  private Claude compatibility watcher/import/export boundary
- leave the codebase with smaller, more explicit storage and query contracts

Execution model:
- `team-lead` should execute Phase U as a sequential cleanup line
- each sprint should delete redirected structure rather than preserve fallback
  branches
- later sprints may assume earlier cleanup decisions are final
- `U.8` through `U.10` assume `U.0` is already complete

Authoritative sprint sequence:
- `docs/phase-U/sprint-U0.md`
- `docs/phase-U/sprint-U1.md`
- `docs/phase-U/sprint-U2.md`
- `docs/phase-U/sprint-U3.md`
- `docs/phase-U/sprint-U4.md`
- `docs/phase-U/sprint-U5.md`
- `docs/phase-U/sprint-U6.md`
- `docs/phase-U/sprint-U7.md`
- `docs/phase-U/sprint-U8.md`
- `docs/phase-U/sprint-U9.md`
- `docs/phase-U/sprint-U10.md`
- `docs/phase-U/removal-inventory.md`

Sprint summary:
- `U.0` remove the old `atm-graft` implementation line (`completed by team-lead`)
- `U.1` delete `metadata.atm` read-path dependence
- `U.2` one message identity ADR and implementation cleanup
- `U.3` thread/update/supersede hardening
- `U.4` unified mutable message state
- `U.5` SQLite query cutover and query simplification
- `U.6` provenance/timing field reduction
- `U.7` roster simplification and explicit member model
- `U.8` shared thin-client ICD for CLI and graft
- `U.9` client-owned graft runtime with one persistent receive thread, one
  open daemon nudge connection, and one host wake/event path
- `U.10` generic daemon advisory-notification surface kept intentionally lean

Phase rules:
- no normal ATM runtime/query path may read Claude JSON or `config.json`
  directly for durable truth; those inputs must stay behind watcher/import
  boundaries
- no sprint should preserve duplicated identity/state structures “for now”
  unless the sprint doc states a concrete approved reason
- any unapproved schema surface discovered during implementation should be
  deleted or moved behind an explicitly non-implemented trait surface
- the authoritative file/line removal inventory for the Phase U cleanup line
  lives in `docs/phase-U/removal-inventory.md`
