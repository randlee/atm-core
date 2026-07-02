# Phase Y State-Machine Coverage Audit

Baseline:

- planning branch: `feature/pY-s0-planning`

## 1. CLI Command State Machines Already Documented

Existing command diagrams under `docs/atm/`:

- `atm-clear.mmd`
- `atm-doctor.mmd`
- `atm-list.mmd`
- `atm-log.mmd`
- `atm-members.mmd`
- `atm-read.mmd`
- `atm-send-ack.mmd`
- `atm-send-compose.mmd`
- `atm-teams.mmd`

Current gap notes:

- clap `--help` output already exists; the missing item is an `atm help`
  subcommand or equivalent Phase `Y` UX surface, which `Y.1` is expected to
  implement
- the delivery-machine diagram set now exists in:
  - `docs/plans/phase-Y/state-diagrams.md`
  - `docs/reports/delivery-state-diagrams.html`
- the normative enum + transition definitions live in:
  - `docs/plans/phase-Y/delivery-state-machines.md`
- remaining missing ownership diagrams:
  - compatibility inbox export / rewrite owner
  - watcher/import/export write ownership

## 2. Client-Socket / Daemon Request Coverage

Current documentation sources:

- `docs/atm-daemon/protocol-icd.md`
- `docs/atm-graft/architecture.md`
- dedicated request-family diagrams:
  - note: `atm-graft` is a library crate, not an `atm` CLI subcommand surface;
    the `atm-graft-*.mmd` diagrams belong here in daemon/client request
    coverage rather than in the CLI command list above
  - `atm-list.mmd`
  - `atm-read.mmd`
  - `atm-send-compose.mmd`
  - `atm-send-ack.mmd`
  - `atm-clear.mmd`
  - `atm-doctor.mmd`
  - `atm-graft-register.mmd`
  - `atm-graft-unregister.mmd`
  - `atm-graft-fetch.mmd`
  - `atm-graft-drain.mmd`

Current gap notes:

- the current daemon ICD packet families are:
  - `send_compose`
  - `send_acknowledge`
  - `heartbeat`
  - `list`
  - `receive`
  - `clear`
  - `doctor`
  - `advisory_register`
  - `advisory_unregister`
  - `advisory_fetch`
  - `advisory_drain`
  - `advisory_stream`
- existing dedicated diagrams already cover:
  - `send_compose`
  - `send_acknowledge`
  - `list`
    - `atm-list.mmd` covers both the CLI `atm list` surface and the
      `list_request (0x0004)` daemon packet family
  - `receive`
  - `clear`
  - `doctor`
  - `advisory_register`
  - `advisory_unregister`
  - `advisory_fetch`
  - `advisory_drain`
- missing explicit request-family diagrams:
  - `heartbeat`
  - `advisory_stream`
- missing write-ownership diagrams:
  - compatibility inbox export / rewrite owner
  - `config.json` write owner and allowed admin/recovery flows
- delivery-policy diagrams now exist for:
  - central event-family dispatcher keyed by `RosterHarness`
  - Claude-harness new-message flow
  - non-Claude-harness new-message flow
  - thread-update legality and delivery flow
  - ack-reply legality and delegated reply-delivery flow
  - inbox-repair staged rebuild flow
  - restore-inbox-rebuild staged publish flow
- `Y.4` code now lands the matching retained-command coordinator/state-machine
  seam in:
  - `crates/atm-core/src/delivery_policy.rs`
  - `crates/atm-core/src/service_runtime.rs`
  - `crates/atm-core/src/send/mod.rs`
  - `crates/atm-core/src/ack/mod.rs`

## 3. SQLite Query Diagrams Already Documented

Existing query diagrams under `docs/atm-rusqlite/`:

- `sql_load-message.mmd`
- `sql_load-message-state.mmd`
- `sql_query-mailbox-metadata-counts.mmd`
- `sql_query-mailbox-metadata-rows.mmd`
- `sql_save-message.mmd`
- `sql_save-message-state.mmd`

Current gap notes:

- this covers the core message hot path, but Phase `Y` must verify whether
  every live runtime query/write used by daemon mail flow is represented
- currently undocumented runtime/store operations include:
  - roster store:
    - `replace_roster`
    - `load_roster`
    - `query_membership`
    - `list_teams`
  - replay state:
    - per-agent `mail_ingest_replay_states` via
      `MailStore::record_ingest_replay_state` /
      `MailStore::load_ingest_replay_state`
    - daemon-global `daemon_remote_replay_states` via
      `SqliteBoundaryAssembly::record_remote_replay_state` /
      `SqliteBoundaryAssembly::load_remote_replay_states`
  - task store:
    - `load_task`
    - `query_task_metadata`
- `record_ack_transition` is an internal SQLite helper; planning must decide
  whether it deserves its own diagram or is covered by the message-state write
  diagram

## 4. Immediate Planning Deliverables

Planning must produce:

- a checked inventory of every CLI command state machine
- a checked inventory of every client-socket request family
- a checked inventory of every SQLite query diagram used by the daemon/runtime
- an explicit backlog list for any missing diagrams required by QA
- one diagram set for the central delivery-policy coordinator and the required
  event-family state machines
- one normative design note for those machines:
  - `docs/plans/phase-Y/delivery-state-machines.md`
- one explicit QA-owned diagram artifact for each required machine:
  - `ClaudeHarnessNewMessage`
  - `NonClaudeHarnessNewMessage`
  - `ThreadUpdateStateMachine`
  - `AckReplyStateMachine`
  - `InboxRepairStateMachine`
  - `RestoreInboxRebuildStateMachine`

## 5. QA Enforcement Intent

Phase `Y` QA should not accept:

- undocumented write-affecting command flows
- undocumented socket request families that can trigger writes
- undocumented SQLite query/write state transitions on the mail hot path

The point of this inventory is to make deletion and simplification safe:
undocumented implied behavior is exactly the behavior most likely to leak old
compatibility assumptions back into the daemon line.

## Backlog

- `heartbeat` diagram
  - origin: Section 2 current gap notes
  - owning sprint: `Y.4`
- `advisory_stream` diagram
  - origin: Section 2 current gap notes
  - owning sprint: `Y.4`
- compatibility inbox export / rewrite owner diagram
  - origin: Sections 1 and 2 current gap notes
  - owning sprint: `Y.3`
- `config.json` write owner and allowed admin/recovery flows diagram
  - origin: Section 2 current gap notes
  - owning sprint: `Y.3`
- roster-store SQLite ops diagram set
  - origin: Section 3 current gap notes
  - owning sprint: `Y.3`
- replay-state ops diagram set
  - origin: Section 3 current gap notes
  - owning sprint: `Y.4`
- task-store ops diagram set
  - origin: Section 3 current gap notes
  - owning sprint: `TBD`
