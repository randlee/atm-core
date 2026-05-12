# Phase U Removal Inventory

This file is the authoritative Phase U removal inventory. File paths and line
numbers below identify the current develop-branch surfaces that the Phase U
sprints remove, rename, or restack.

## U.0 — Remove Old `atm-graft` Implementation Line

Status:
- completed by `team-lead`

Inventory note:
- the old graft implementation line is already gone on `develop`
- the later graft sprints (`U.8` through `U.10`) restack ownership and shared
  interfaces rather than preserving or incrementally repairing the removed line

## U.1 — Delete `metadata.atm` Read-Path Dependence

Primary code/doc targets:
- `crates/atm-core/src/workflow.rs:198-231`
  - `atm_message_id(...)`
  - `set_atm_message_id(...)`
- `crates/atm-core/src/schema/inbox_message.rs:189-213`
  - `AtmMetadataFields`
- `crates/atm-core/src/schema/inbox_message.rs:378-590`
  - forward metadata export/import helpers
- `crates/atm-core/src/send/mod.rs:250-255`
  - forward ATM metadata write on send
- `crates/atm-core/src/send/mod.rs:427-429`
  - forward ATM metadata write on repair notice send
- `crates/atm-daemon/src/boundary_adapters.rs:397-398`
  - `AtmMessageId` to compatibility-id bridge in daemon-side tests
- `crates/atm-daemon/src/reconcile_runtime.rs:1094-1095`
  - `AtmMessageId` to compatibility-id bridge in reconcile tests

## U.2 — ADR: One Message Identity

Primary code/doc targets:
- `crates/atm-core/src/schema/inbox_message.rs:18-123`
  - `LegacyMessageId`
  - `AtmMessageId`
- `crates/atm-core/src/schema/inbox_message.rs:283-317`
  - split legacy vs forward envelope representations
- `crates/atm-core/src/schema/inbox_message.rs:506-555`
  - UUID/ULID reinterpretation on import
- `crates/atm-core/src/send/mod.rs:15,49,95,250-255,427-429`
  - dual-id generation and propagation
- `crates/atm-core/src/ack/mod.rs:14,29,39,43,198-203`
  - dual-id reply generation and public result fields
- `crates/atm-rusqlite/src/shared_db.rs:21-22,93-94,375-376`
  - `legacy_message_id` durable column and index
- `crates/atm-rusqlite/src/lib.rs:307,323-347`
  - SQLite persistence of `legacy_message_id`
- `crates/atm-rusqlite/src/mailbox_metadata.rs:4-12,39-40,106-125`
  - metadata query use of `LegacyMessageId`

## U.3 — Thread / Update / Supersede Hardening

Primary behavior targets:
- `crates/atm-core/src/send/mod.rs:518-542`
  - thread validation and mode pairing
- `crates/atm-core/src/threading.rs:56-106,130-142`
  - terminal-chain and successor behavior
- `crates/atm-core/src/ack/mod.rs:383-454`
  - terminal-node ack resolution

This sprint is primarily semantic hardening and test expansion rather than bulk
surface deletion.

## U.4 — Unified Mutable Message State

Primary code/doc targets:
- `crates/atm-rusqlite/src/shared_db.rs:24-45,100`
  - `stale_at`
  - `ack_state`
  - `mail_visibility_states`
- `crates/atm-rusqlite/src/lib.rs:291-369`
  - split write path for `stale_at`, `ack_state`, `recorded_at`
- `crates/atm-rusqlite/src/lib.rs:442-456`
  - split visibility/ack updates
- `crates/atm-rusqlite/src/lib.rs:486,581,609`
  - split state reads and health derivation
- `crates/atm-rusqlite/src/mailbox_metadata.rs:45-60,164-183`
  - multi-table mailbox projection joins
- `crates/atm-core/src/schema/inbox_message.rs:219,304,408-433,568-596`
  - `stale_at` compatibility-envelope handling
- `crates/atm-core/src/threading.rs:20-24`
  - expiration semantics currently rooted in `stale_at`

## U.5 — SQLite Query Cutover And Query Simplification

Primary code/doc targets:
- `crates/atm-core/src/service_runtime_store.rs:45-97,107-187`
  - source-file observation/commit/lock helpers
- `crates/atm-core/src/list.rs:186-193`
  - file-backed list path
- `crates/atm-core/src/read/mod.rs:245-337`
  - file-backed read path
- `crates/atm-core/src/ack/mod.rs:144-277`
  - file-backed ack path
- `crates/atm-core/src/clear/mod.rs:123-150`
  - file-backed clear path
- `crates/atm-core/src/mailbox/store.rs:41-136`
  - source-file commit/read/lock orchestration

## U.6 — Provenance / Timing Field Reduction

Primary code/doc targets:
- `crates/atm-core/src/boundary/mail.rs:57-59`
  - `imported_from`
  - `recorded_at`
- `crates/atm-rusqlite/src/shared_db.rs:25-26`
  - `imported_from`
  - `recorded_at`
- `crates/atm-rusqlite/src/lib.rs:41,323-351,386-425,587`
  - persistence, load, and health-time usage
- `crates/atm-daemon/src/peer_transport.rs:206-222`
  - `recorded_at` write on peer transport ingest

## U.7 — Roster Simplification And Explicit Member Model

Primary code/doc targets:
- `crates/atm-rusqlite/src/shared_db.rs:68-112,381-390`
  - `rosters`
  - `team_roster`
  - `roster_json`
  - `member_json`
- `crates/atm-rusqlite/src/roster_store.rs:23-61`
  - whole-roster snapshot write plus per-member projection rebuild
- `crates/atm-rusqlite/src/roster_store.rs:85-105`
  - whole-roster snapshot load
- `crates/atm-rusqlite/src/roster_store.rs:120-202`
  - per-member projection reads/updates
- `crates/atm-rusqlite/src/lib.rs:1373`
  - schema test anchored to `team_roster`

## U.8 — Shared Thin-Client ICD For CLI And Graft

Develop note:
- there is no current graft implementation line on `develop` to remove.
- this sprint restacks the abandoned earlier graft-client intent as a shared
  thin-client design rule.

Primary develop-branch design/doc targets:
- `docs/atm-core/boundaries.md`
- `docs/atm-core/architecture.md`
- `docs/atm-daemon/protocol-icd.md`

Required restack rule:
- any graft support must use shared `AtmProtocol` contracts and the same ICD
  family as CLI traffic.

## U.9 — Client-Owned Graft Runtime

Develop note:
- there is no current graft runtime implementation line on `develop` to remove.
- this sprint restacks the abandoned earlier graft-runtime intent as an
  ownership rule.

Primary develop-branch design/doc targets:
- `docs/atm-core/architecture.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-core/boundaries.md`

Required restack rule:
- client-specific runtime behavior belongs in `atm-graft`, not `atm-daemon`.

## U.10 — Generic Daemon Advisory-Notification Surface

Develop note:
- there is no current graft-specific daemon surface on `develop` to remove.
- this sprint restacks the abandoned earlier graft-notification intent as a
  generic-boundary rule.

Primary develop-branch design/doc targets:
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/protocol-icd.md`
- `docs/atm-core/boundaries.md`

Required restack rule:
- any daemon-owned post-commit notification surface must be generic; client
  crates such as `atm-graft` are consumers, not daemon-owned subsystems.
