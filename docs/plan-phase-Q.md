# Phase Q Plan

Branch: `plan/phase-Q`
Base: `develop` (`9d3bd4d`)

## Goal

Replace the filesystem JSON mailbox store as ATM's mail source of truth with
SQLite, while keeping the Claude inbox path as the required delivery and
context-injection surface for Claude clients.

## Motivation

Phase P improved the current file-based model enough for interim release use,
but `docs/lock-release-gate.md` concluded that the mailbox-lock architecture
still has bounded but real failure modes under contention and crash recovery.
Phase Q removes mailbox correctness from that lock model instead of hardening
it further.

Key consequences from the gate:
- file locks may remain as transitional compatibility mechanisms only
- ATM command correctness must stop depending on `.lock` sentinel cleanup
- the long-term answer is one ATM-owned transactional store

## Architecture Decision

### Decision

Use one SQLite database per team in WAL mode as the single source of truth for
ATM mail state.

Proposed path:
- `.claude/teams/<team>/.atm-state/mail.db`

SQLite mode and invariants:
- `journal_mode = WAL`
- `foreign_keys = ON`
- all ATM mutating commands use explicit transactions
- mailbox `.json` files stop being ATM's source of truth

### What Remains On Filesystem

Claude inbox JSONL files remain required for:
- Claude context injection
- interoperability with direct Claude-native `SendMessage` producers
- append-only delivery/export performed by ATM `send` and ack replies

Those files become:
- external ingress for Claude-authored messages
- compatibility/export surface for ATM-authored messages
- not the authoritative store for ATM read/ack/clear/task state

### Command Model

- `send`
  - insert authoritative message/state rows in SQLite
  - append Claude-compatible export record to recipient inbox
- `read`
  - ingest unseen inbox rows into SQLite
  - read from SQLite projection
- `ack`
  - ingest before acting
  - update authoritative ack/task state in SQLite
  - append reply export record when required
- `clear`
  - ingest before acting
  - mark hidden/cleared in SQLite
  - no correctness-critical inbox rewrite

## Schema Design

Phase Q should start with a deliberately small schema.

### `messages`

Authoritative logical message row.

Suggested columns:
- `message_key TEXT PRIMARY KEY`
- `team_name TEXT NOT NULL`
- `recipient_agent TEXT NOT NULL`
- `sender_display TEXT NOT NULL`
- `sender_canonical TEXT NULL`
- `sender_team TEXT NULL`
- `body TEXT NOT NULL`
- `summary TEXT NULL`
- `created_at TEXT NOT NULL`
- `source_kind TEXT NOT NULL`
- `legacy_message_id TEXT NULL`
- `atm_message_id TEXT NULL`
- `raw_metadata_json TEXT NULL`

Rules:
- `message_key` is the canonical ATM identity key
- preferred forms:
  - `atm:<ulid>`
  - `legacy:<uuid>`
  - `ext:<sha256>` for external Claude-native messages with no ATM id
- `legacy_message_id` and `atm_message_id` stay unique when present

### `inbox_ingest`

Tracks imported filesystem records so external inbox writes become durable in
SQLite without duplicate imports.

Suggested columns:
- `team_name TEXT NOT NULL`
- `recipient_agent TEXT NOT NULL`
- `source_path TEXT NOT NULL`
- `source_fingerprint TEXT NOT NULL`
- `message_key TEXT NOT NULL`
- `imported_at TEXT NOT NULL`

Primary key:
- `(team_name, recipient_agent, source_fingerprint)`

Rules:
- `source_fingerprint` prefers stable ids when present
- fallback is a deterministic hash of mailbox identity plus canonicalized raw
  record for external messages without ATM ids

### `ack_state`

Authoritative acknowledgement state.

Suggested columns:
- `message_key TEXT PRIMARY KEY`
- `pending_ack_at TEXT NULL`
- `acknowledged_at TEXT NULL`
- `ack_reply_message_key TEXT NULL`
- `ack_reply_team TEXT NULL`
- `ack_reply_agent TEXT NULL`

Rules:
- one row per ack-capable logical message
- absence of row means no ATM ack semantics

### `tasks`

Basic task table for task-linked mail.

Suggested columns:
- `task_id TEXT PRIMARY KEY`
- `message_key TEXT NOT NULL`
- `status TEXT NOT NULL`
- `created_at TEXT NOT NULL`
- `acknowledged_at TEXT NULL`
- `metadata_json TEXT NULL`

Rules:
- initial statuses can stay minimal:
  - `pending_ack`
  - `acknowledged`

### `message_visibility`

ATM-owned display state.

Suggested columns:
- `message_key TEXT PRIMARY KEY`
- `read_at TEXT NULL`
- `cleared_at TEXT NULL`

Rules:
- read/unread and clear state stop rewriting source inbox rows for correctness

## Migration Strategy

Recommended strategy: staged cutover, not big-bang replacement.

### Stage 1: Introduce Store Boundary

- add one `mail_store` owner boundary in `atm-core`
- keep existing file-backed behavior behind current code paths
- add SQLite implementation and schema bootstrap
- no user-visible command behavior change yet

### Stage 2: Ingest + Dual Write

- ingest mailbox JSONL into SQLite on command entry
- `send` and ack replies write to SQLite first, then export to inbox
- keep existing read behavior available for comparison/debug only
- SQLite becomes authoritative for new ATM-authored rows

### Stage 3: Read/Ack/Clear Cutover

- `read`, `ack`, and `clear` operate from SQLite after inbox ingest
- stop correctness-critical full-file inbox rewrites
- keep export-only inbox append for Claude delivery

### Stage 4: Compatibility Cleanup

- remove mailbox-lock dependence from runtime correctness
- retire stale-lock cron sweep for mail flows
- keep only any compatibility code still required for non-mail paths

## Backward Compatibility

Phase Q must preserve:
- existing `atm send`, `atm read`, `atm ack`, and `atm clear` CLI contracts
- Claude-native inbox top-level schema
- `metadata.atm` placement for ATM machine fields

Compatibility rules:
- old inbox rows with top-level ATM legacy fields remain readable
- existing workflow sidecar data can be imported during first-run migration
- external Claude-native messages with no ATM ids must still appear in `atm read`

## Sprint Breakdown

### Q.1 — SQLite Store Foundation

Scope:
- add `mail_store` abstraction
- add SQLite bootstrap, migrations, and schema
- add transaction helpers

Acceptance:
- database opens under `.atm-state/mail.db`
- schema bootstrap is deterministic and idempotent
- store-layer tests cover create/read/update transaction basics

### Q.2 — Inbox Ingest + Send Dual Write

Scope:
- ingest external inbox rows into SQLite
- move ATM `send` to SQLite-first plus inbox export
- keep exported envelope Claude-native + `metadata.atm`

Acceptance:
- `send` inserts authoritative rows in SQLite
- ATM-authored inbox export still works for Claude recipients
- repeated ingest does not duplicate imported records

### Q.3 — Ack/Task Migration

Scope:
- move ack state and task state to SQLite
- append reply exports after SQLite commit
- stop treating inbox mutation as authoritative ack state

Acceptance:
- ack-required messages are authoritative in SQLite
- task linkage and acknowledged state survive restart without inbox rewrites
- reply export still lands in Claude inbox correctly

### Q.4 — Read/Clear Cutover

Scope:
- `read` projects from SQLite after ingest
- `clear` updates SQLite visibility state
- remove correctness-critical full-file mailbox rewrites from these paths

Acceptance:
- `read` and `clear` no longer require mailbox rewrite correctness
- lock contention on inbox files does not block SQLite-owned state transitions
- existing CLI output remains compatible

### Q.5 — Lock Retirement + Ops Cleanup

Scope:
- remove mail-flow dependence on mailbox lock cron sweep
- update doctor/restore/backup docs and tooling
- remove or quarantine obsolete mailbox-lock behaviors for mail state

Acceptance:
- mail flows do not require the 5-minute stale-lock sweep
- operational docs match SQLite ownership
- Phase Q release gate proves normal mail operation without mailbox-lock
  correctness dependence

## Risk Register

### 1. External Claude Messages Bypass SQLite

Risk:
- Claude-native producers write directly to inbox JSONL

Mitigation:
- deterministic command-path ingest before `read`/`ack`/`clear`
- keep ingest idempotent and bounded

### 2. Historical Messages Without Stable ATM Identity

Risk:
- legacy Claude-native rows may lack `message_id` / `metadata.atm.messageId`

Mitigation:
- explicit fallback fingerprint for import
- isolate this in `inbox_ingest`; do not spread ad hoc identity logic

### 3. Export Compatibility Regression

Risk:
- SQLite migration breaks Claude context injection

Mitigation:
- retain raw inbox export tests
- validate exported records against Claude-native schema plus `metadata.atm`

### 4. Mixed-Version Operation During Cutover

Risk:
- older commands still expect filesystem truth

Mitigation:
- stage rollout behind clear sprint gates
- do not remove compatibility readers until cutover is complete

### 5. WAL / Backup / Restore Surprises

Risk:
- current team backup/restore flows are file-oriented

Mitigation:
- add explicit SQLite backup/restore handling in Q.5
- test restore with WAL and checkpointed states

### 6. Scope Sprawl

Risk:
- Phase Q turns into a database/platform rewrite

Mitigation:
- keep v1 schema small
- no daemon
- no generalized query engine
- no broad plugin/event redesign inside this phase

## Release Gate For Phase Q

Phase Q should be considered complete only when:
- ATM mail correctness no longer depends on mailbox `.lock` files
- SQLite is the authoritative store for read/ack/clear/task semantics
- Claude inbox files remain a compatible export/ingest surface only
- stale lock cleanup can no longer wedge normal ATM mail flows
