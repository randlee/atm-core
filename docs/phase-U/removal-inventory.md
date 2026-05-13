# Phase U Removal Inventory

This file is the authoritative Phase U removal inventory. File paths and line
numbers below identify the current develop-branch surfaces that the Phase U
sprints remove, rename, or restack.

## U.0 — Remove Old `atm-graft` Implementation Line

Status:
- completed by `team-lead`

Inventory note:
- the old graft implementation line is removed on `integrate/phase-U` by
  `team-lead`, but current `develop @ b6506ef` still carries the pre-U.0 line
- the later graft sprints (`U.8` through `U.10`) use the current `develop`
  surfaces below as removal/restack targets rather than preserving them

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
  - `expires_at`
  - `ack_state`
  - `mail_visibility_states`
- `crates/atm-rusqlite/src/lib.rs:291-369`
  - split write path for `expires_at`, `ack_state`, `recorded_at`
- `crates/atm-rusqlite/src/lib.rs:442-456`
  - split visibility/ack updates
- `crates/atm-rusqlite/src/lib.rs:486,581,609`
  - split state reads and health derivation
- `crates/atm-rusqlite/src/mailbox_metadata.rs:45-60,164-183`
  - multi-table mailbox projection joins
- `crates/atm-core/src/schema/inbox_message.rs:219,304,408-433,568-596`
  - `expires_at` compatibility-envelope handling
- `crates/atm-core/src/threading.rs:20-24`
  - expiration semantics currently rooted in `expires_at`

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
- `crates/atm-rusqlite/src/shared_db.rs`
  - `rosters`
  - `roster_json`
  - durable roster `pid`
  - whole-roster JSON as SQLite truth
- `crates/atm-rusqlite/src/roster_store.rs`
  - whole-roster snapshot write/load logic
  - per-member JSON projection readback
  - durable heartbeat persistence through roster storage
- `crates/atm-daemon/src/runtime_status_cache.rs`
  - direct `config.json` content reads for runtime roster truth
- `crates/atm-daemon/src/runtime_health.rs`
  - durable roster `pid` continuity assumptions
- `crates/atm-rusqlite/src/lib.rs`
  - schema test anchored to the canonical `team_roster` shape

## U.8 — Shared Thin-Client ICD For CLI And Graft

Primary current-develop code/doc targets:
- `crates/atm-core/src/graft.rs:25-296`
  - `AtmGraftClient`
  - `GraftSessionPort`
  - `GraftSessionState`
  - `GraftSessionId`
  - `NudgeEvent`
  - `GraftNudgeFetchRequest`
  - `GraftNudgeDrainRequest`
- `crates/atm-core/src/protocol.rs:54-57,69-72,219-233,532-553`
  - graft-specific request/response envelope variants and packet kinds
- `crates/atm/src/composition.rs:327-443`
  - graft-specific CLI composition forwarding helpers
- `docs/atm-daemon/protocol-icd.md:247-264,292-306,351-365,493-496`
  - graft-specific protocol inventory entries

Required restack rule:
- any graft support must use the same shared ICD family as CLI traffic
- additive registration or advisory-delivery messages must be renamed
  generically rather than preserving `Graft*` packet naming

U.8 implementation note:
- the accepted U.8 line lands `crates/atm-graft` as a thin shared-ICD client
  over `ClientTransport` and the existing unary `RequestEnvelope` /
  `ResponseEnvelope` family
- `GraftSessionId` is replaced in the shared naming inventory by
  `AdvisorySessionId`
- runtime/session/advisory surfaces remain intentionally deferred to `U.9`
  and `U.10`

U.8-U.10 ownership matrix for current graft-named surfaces:

Matrix scope note:
- this matrix covers shared protocol/session-contract items only
- the `atm-graft` library-owned runtime items below (`GraftClient`,
  `GraftSessionOptions`, `HostNudgeInjector`, `GraftObservability`, and poll /
  drain receive-loop machinery) are solely U.9-owned

| Current surface | Primary cutover sprint | Reason |
| --- | --- | --- |
| `GraftSessionId` | `U.8` | replaced in the shared naming line by `AdvisorySessionId`; DTO-family ownership belongs to the shared ICD sprint |
| `GraftSessionState` | `U.9` | session lifecycle interpretation is client-runtime ownership |
| `GraftSessionPort` | `U.10` | session registration/fetch/drain contract is finalized with the generic daemon advisory surface |
| `NudgeEvent` | `U.10` | daemon-originated advisory event payload is finalized with the generic daemon advisory surface |
| `GraftNudgeFetchRequest` | `U.10` | fetch/drain remains a daemon advisory/debug surface decision |
| `GraftNudgeDrainRequest` | `U.10` | fetch/drain remains a daemon advisory/debug surface decision |

## U.9 — Client-Owned Graft Runtime

Status:
- complete

Primary current-develop code/doc targets:
- `crates/atm-graft/src/lib.rs:55-82,86-150,166-239,311-836,943-1355`
  - `HostNudgeInjector`
  - `GraftObservability`
  - `GraftSessionOptions`
  - `GraftClient`
  - `GraftSession`
  - `run_receive_loop(...)`
  - current poll/drain receive-loop tests
- `docs/atm-core/architecture.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-core/boundaries.md`

Required restack rule:
- client-specific runtime behavior belongs in `atm-graft`, not `atm-daemon`
- the production embedded runtime path is one persistent receive thread
  reading one dedicated daemon advisory-stream socket; the current poll/drain
  loop is not the target design
- U.9 is the primary cutover sprint for:
  - `GraftSessionState`
    - scope: client-owned session lifecycle only, not daemon queue semantics
  - old poll/drain receive-loop machinery in `crates/atm-graft/src/lib.rs`

## U.10 — Generic Daemon Advisory-Notification Surface

Status:
- completed on `feature/pU-u10-generic-advisory-notification`

Primary current-develop code/doc targets:
- `crates/atm-daemon/src/advisory_runtime.rs:18-244,300-393`
  - daemon graft runtime ownership
  - bounded queue semantics
  - fetch/drain behavior
  - overflow tests
- `crates/atm-daemon/src/tests.rs:404-505`
  - graft-specific dispatcher routing tests
- `crates/atm/src/commands/graft.rs:1-232`
  - companion CLI fetch/drain debug surface
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/protocol-icd.md:247-264,292-306,351-365,493-496`
- `docs/atm-core/boundaries.md`

Required restack rule:
- any daemon-owned post-commit notification surface must be generic; client
  crates such as `atm-graft` are consumers, not daemon-owned subsystems
- production embedded delivery must use a live daemon advisory stream;
  fetch/drain remains optional companion CLI/debug support only
- U.10 is the primary cutover sprint for:
  - `GraftSessionPort`
  - `NudgeEvent`
  - `GraftNudgeFetchRequest`
  - `GraftNudgeDrainRequest`
