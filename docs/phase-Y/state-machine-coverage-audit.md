# Phase Y State-Machine Coverage Audit

Baseline:

- planning branch: `feature/pY-s0-planning`

## 1. CLI Command State Machines Already Documented

Existing command diagrams under `docs/atm/`:

- `atm-clear.mmd`
- `atm-doctor.mmd`
- `atm-graft-drain.mmd`
- `atm-graft-fetch.mmd`
- `atm-graft-register.mmd`
- `atm-graft-unregister.mmd`
- `atm-list.mmd`
- `atm-log.mmd`
- `atm-members.mmd`
- `atm-read.mmd`
- `atm-send-ack.mmd`
- `atm-send-compose.mmd`
- `atm-teams.mmd`

Current gap notes:

- `atm help` does not exist yet and therefore has no state machine
- there is no dedicated state-machine diagram for the compatibility inbox
  export/write path itself
- there is no dedicated state-machine diagram for watcher/import/export write
  ownership

## 2. Client-Socket / Daemon Request Coverage

Current documentation sources:

- `docs/atm-daemon/protocol-icd.md`
- `docs/atm-graft/architecture.md`
- the graft request-family command diagrams above

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
    - `record_remote_replay_state`
    - `load_remote_replay_states`
    - ingest replay state record/load
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

## 5. QA Enforcement Intent

Phase `Y` QA should not accept:

- undocumented write-affecting command flows
- undocumented socket request families that can trigger writes
- undocumented SQLite query/write state transitions on the mail hot path

The point of this inventory is to make deletion and simplification safe:
undocumented implied behavior is exactly the behavior most likely to leak old
compatibility assumptions back into the daemon line.
