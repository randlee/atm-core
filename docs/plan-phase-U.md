# Phase U — Mailbox Simplification And Identity Cleanup

Goal:
- remove unapproved or confusing mailbox, identity, roster, and task-carrying
  design from the current ATM line before SQLite/store contracts are treated as
  settled
- keep SQLite as the only ATM-owned mailbox state/query authority outside the
  private Claude compatibility watcher/import/export boundary
- leave the codebase with smaller, more explicit storage and query contracts

Integration branch: `integrate/phase-U`

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
- `U.1` delete `metadata.atm` read-path dependence and remove the namespace
  from active compatibility output
- `U.2` one message identity ADR and implementation cleanup, with Claude
  `message_id` retained only as the UUID wire form of `AtmMessageId`
- `U.3` thread/update/supersede hardening
  - `add-details` preserves predecessor context in the effective current body
  - `supersede` exposes only the replacement body
- `U.4` unified mutable message state (`mail_message_states`)
- `U.5` SQLite query cutover and query simplification
  - this sprint cuts over `atm list` and `atm read`
  - `atm ack` and `atm clear` remain on their existing runtime path until a
    later dedicated rewrite
- `U.6` provenance/timing field reduction
- `U.7` roster simplification and explicit member model
- `U.8` shared thin-client ICD for CLI and graft
- `U.9` client-owned graft runtime with one persistent receive thread, one
  open dedicated daemon advisory-stream connection, one minimal client-side
  pending queue until host consumption, and one host wake/event path
- `U.10` generic daemon advisory-notification surface kept intentionally lean

Graft ownership split:
- `U.8` owns shared ICD family and naming/DTO planning
- `U.9` owns client runtime cutover
  - `U.9` is allowed to build on the current graft-named session/advisory
    substrate from `develop` as a temporary compatibility surface
  - that temporary substrate must not be treated as the final boundary shape
- `U.10` owns daemon advisory-surface generification
  - `U.10` must remove or generify the remaining daemon-owned graft-specific
    code so the temporary `U.9` substrate does not harden into the final
    architecture
  - specifically, `U.10` is responsible for cleaning up the daemon-owned
    graft runtime line, graft-named daemon packet family, and related daemon
    tests/docs that are only being tolerated temporarily so `U.9` can land on
    the existing substrate

Phase rules:
- no normal ATM runtime/query path may read Claude JSON or `config.json`
  directly for durable truth; those inputs must stay behind watcher/import
  boundaries
- no sprint should preserve duplicated identity/state structures “for now”
  unless the sprint doc states a concrete approved reason
- any unapproved schema surface discovered during implementation should be
  deleted or moved behind an explicitly non-implemented trait surface
- Phase U schema simplification was discovered concretely on
  `feature/fix-sqlite-load-writer-shutdown`, especially in
  `crates/atm-rusqlite/src/shared_db.rs`; use that discovery work as evidence
  when executing the schema sprints rather than re-inventing the removal set
  - the authoritative file/line removal inventory for the Phase U cleanup line
  lives in `docs/phase-U/removal-inventory.md`
