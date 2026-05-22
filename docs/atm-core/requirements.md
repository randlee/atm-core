# ATM-Core Crate Requirements

## 1. Purpose

This document defines the `atm-core` crate requirements.

The `atm-core` crate owns the reusable ATM business logic, persistent-store
contracts, and strict I/O subsystem boundaries. Product behavior remains
defined in [`../requirements.md`](../requirements.md).

The crate-local machine-readable boundary inventory lives in:
- [`./boundaries.md`](./boundaries.md)

## 2. Ownership

`atm-core` owns:

- path and config resolution policy
- address parsing and validation
- store contracts and service semantics
- inbox ingress/export contracts
- config ingress contracts
- workflow and typestate rules
- list/send/read/ack/clear service behavior
- log query/follow service behavior over the observability boundary
- doctor service behavior
- structured core errors

`atm-core` does not own:

- clap parsing
- terminal formatting
- process exit policy
- direct dependency on concrete observability crates
- daemon singleton/runtime lifecycle
- concrete socket transport
- direct agent-process notification transport

## 3. Requirement Namespace

The `atm-core` crate uses the `REQ-CORE-*` namespace.

Initial allocation:

- `REQ-CORE-CONFIG-*`
- `REQ-CORE-MAILBOX-*`
- `REQ-CORE-WORKFLOW-*`
- `REQ-CORE-LIST-*`
- `REQ-CORE-SEND-*`
- `REQ-CORE-READ-*`
- `REQ-CORE-ACK-*`
- `REQ-CORE-CLEAR-*`
- `REQ-CORE-LOG-*`
- `REQ-CORE-DOCTOR-*`
- `REQ-CORE-OBS-*`
- `REQ-CORE-TEAM-*`
- `REQ-CORE-RUNTIME-*`
- `REQ-CORE-STORE-*`
- `REQ-CORE-COMPAT-*`
- `REQ-CORE-INGEST-*`
- `REQ-CORE-BOUNDARY-*`
- `REQ-CORE-TRANSPORT-*`
- `REQ-CORE-DAEMON-*`
- `REQ-CORE-LOCK-*`
- `REQ-CORE-TEST-*`
- `REQ-CORE-QA-*`

Initial crate requirement IDs:

- `REQ-CORE-CONFIG-001` `atm-core` owns shared home/path/config/identity
  resolution policy across the CLI and daemon-backed runtime. Satisfies the
  path/config/identity aspects of:
  `REQ-P-CONTRACT-001`, `REQ-P-IDENTITY-001`, `REQ-P-DOCTOR-001`.
  It also owns parsing and validation of `[atm].claude_jsonl_body_export_max_bytes`
  for the ATM-authored JSONL compatibility envelope.
- `REQ-CORE-CONFIG-002` `atm-core` owns shared address parsing, alias rewrite,
  and team/member validation policy. Satisfies the address resolution and
  target-validation aspects of:
  `REQ-P-ADDRESS-001`, `REQ-P-SEND-001`, `REQ-P-LIST-001`, `REQ-P-READ-001`,
  `REQ-P-CLEAR-001`.
- `REQ-CORE-CONFIG-003` `atm-core` owns persisted config/team schema recovery
  and diagnostic policy. Satisfies the compatibility-recovery and
  persisted-data error aspects of:
  `REQ-P-CONFIG-HEALTH-001`, `REQ-P-ERROR-001`,
  `REQ-P-RELIABILITY-001`.
- `REQ-CORE-SEND-001` `atm-core` owns send-time missing-config fallback,
  sender-warning, and repair-notification behavior above the shared config
  loader. Satisfies the missing-config send-path aspects of:
  `REQ-P-SEND-001`, `REQ-P-CONFIG-HEALTH-001`,
  `REQ-P-RELIABILITY-001`.
- `REQ-CORE-SEND-002` `atm-core` owns ATM-authored alert metadata placement,
  compatibility reads, and degradation rules across write/read paths. Satisfies
  the alert-metadata schema and sender-side dedup aspects of:
  `REQ-P-SCHEMA-001`, `REQ-P-CONFIG-HEALTH-001`,
  `REQ-P-RELIABILITY-001`.
- `REQ-CORE-MAILBOX-001` `atm-core` owns transitional mailbox compatibility
  behavior and the file-backed import/export boundary during the migration
  line. Satisfies the persisted mailbox compatibility aspects of:
  `REQ-P-CONTRACT-001`, `REQ-P-SEND-001`, `REQ-P-LIST-001`, `REQ-P-READ-001`,
  `REQ-P-ACK-001`, `REQ-P-CLEAR-001`, `REQ-P-RELIABILITY-001`,
  `REQ-P-IDLE-001`.
  Phase-U note: active compatibility reads no longer depend on `metadata.atm`;
  inbound shared-inbox records strip that namespace through
  `strip_metadata_atm_namespace()` in
  `crates/atm-core/src/schema/inbox_message.rs`.
- `REQ-CORE-COMPAT-001` `atm-core` owns the Claude JSONL compatibility
  projection contract for ATM-authored exports and inbound compatibility
  ingestion, including the bounded export cap, retrieval-stub rule, and
  idempotent watcher/reconcile projection handling for the same logical
  message. Satisfies:
  `REQ-P-CONTRACT-001`, `REQ-P-RELIABILITY-001`.
  Phase-U note: the active compatibility contract strips inbound
  `metadata.atm` rather than preserving it as a live schema surface; the
  production enforcement point is `strip_metadata_atm_namespace()` in
  `crates/atm-core/src/schema/inbox_message.rs`.
- `REQ-CORE-WORKFLOW-001` `atm-core` owns the two-axis workflow model and legal
  transitions. Satisfies the state-classification and legal-transition aspects
  of:
  `REQ-P-LIST-001`, `REQ-P-READ-001`, `REQ-P-ACK-001`, `REQ-P-CLEAR-001`,
  `REQ-P-WORKFLOW-001`.
- `REQ-CORE-LIST-001` `atm-core` owns the metadata-first queue query contract
  shared by `atm list` and selector-driven `atm read`, including bounded
  query behavior, shared match filters, successor-chain terminal-node
  selection, and list-row shaping. Satisfies:
  `REQ-P-LIST-001`, `REQ-P-READ-001`, `REQ-P-RELIABILITY-001`.
  U.3 note: logical-current projection must remain mode-aware; terminal
  `add-details` preserves predecessor context in the effective current body,
  while terminal `supersede` does not.
- `REQ-CORE-SEND-003` `atm-core` owns send-path message construction,
  classification, and compatibility-export behavior above the owned
  ingress/export boundaries. Satisfies the send-path service aspects of:
  `REQ-P-SEND-001`, `REQ-P-IDLE-001`.
- `REQ-CORE-LOG-001` `atm-core` owns ATM log query/follow service behavior over
  the injected observability boundary. Satisfies the core
  query/follow/filtering aspects of:
  `REQ-P-LOG-001`, `REQ-P-OBS-001`.
- `REQ-CORE-DOCTOR-001` `atm-core` owns local doctor diagnostics and readiness
  evaluation. Satisfies the diagnostic evaluation aspects of:
  `REQ-P-DOCTOR-001`, `REQ-P-OBS-001`.
- `REQ-CORE-OBS-001` `atm-core` owns the abstract observability boundary and
  ATM-owned event/query models above shared crates. Satisfies the ATM event,
  query-model, and health-contract aspects of:
  `REQ-P-OBS-001`.
- `REQ-CORE-TEAM-001` `atm-core` owns the retained local team discovery,
  roster inspection, roster repair, and backup/restore behavior. Satisfies the
  local team-surface aspects of:
  `REQ-P-TEAMS-001`, `REQ-P-MEMBERS-001`.
- `REQ-CORE-RUNTIME-001` `atm-core` owns the service-layer contracts for the
  durable store family and the command semantics above those stores.
  Refines the product-level store-ownership and lock-retirement requirements
  in [`../requirements.md`](../requirements.md) Section 21.
- `REQ-CORE-RUNTIME-002` `atm-core` owns the service-layer contract that keeps
  durable roster truth separate from live daemon-status truth. Satisfies the
  state-separation and reliability aspects of:
  `REQ-P-RELIABILITY-001`.
- `REQ-CORE-STORE-001` `atm-core` owns the SQLite schema contract, canonical
  `message_key` row-key model, the one-logical-message-identity rule
  (`AtmMessageId`), and required lookup/dedupe constraints for the compatible
  `message_id` wire form.
  Satisfies:
  `REQ-P-CONTRACT-001`, `REQ-P-RELIABILITY-001`.
- `REQ-CORE-STORE-002` `atm-core` owns WAL / foreign-key / explicit
  transaction policy at the store boundary. Satisfies:
  `REQ-P-RELIABILITY-001`.
- `REQ-CORE-INGEST-001` `atm-core` owns the inbox/config ingest contract for
  replay, backpressure/degradation behavior, and no-silent-drop policy.
  Satisfies:
  `REQ-P-CONTRACT-001`, `REQ-P-RELIABILITY-001`.
- `REQ-CORE-BOUNDARY-001` `atm-core` owns the strict trait boundaries for
  store, protocol, transport, inbox ingress/export, config ingress,
  watcher/reconcile, notification sink, and status-source calls. Satisfies the subsystem-boundary aspects of:
  `REQ-P-CONTRACT-001`, `REQ-P-TEST-001`.
- `REQ-CORE-BOUNDARY-002` `atm-core` owns the typed error-model contracts used
  by service boundaries. Satisfies the structured-error aspects of:
  `REQ-P-ERROR-001`, `REQ-P-RELIABILITY-001`.
- `REQ-CORE-OBS-002` `atm-core` owns ATM event and error models above the
  shared observability boundary for both CLI and daemon callers. Satisfies:
  `REQ-P-OBS-001`, `REQ-P-DOCTOR-001`.
- `REQ-CORE-DAEMON-001` `atm-core` owns the daemon-facing singleton/runtime
  service contract that callers depend on, including no-hidden-direct-I/O
  fallback behavior. Satisfies:
  `REQ-P-RUNTIME-001`, `REQ-P-RELIABILITY-001`.
- `REQ-CORE-DAEMON-002` `atm-core` owns the contract that daemon runtime
  orchestration stays outside mail business semantics. Satisfies:
  `REQ-P-CONTRACT-001`, `REQ-P-TEST-001`.
- `REQ-CORE-DAEMON-003` `atm-core` owns the production runtime-entry contract
  that callers connect to an already-running daemon first, auto-start it once
  when absent, and fail with a typed daemon-unavailable error rather than
  silently falling back to direct SQLite or inbox-file access. Satisfies:
  `REQ-P-RUNTIME-001`, `REQ-P-RELIABILITY-001`.
- historical `REQ-CORE-GRAFT-001` is retired by the Phase U graft restack.
  Any earlier graft-specific contract intent is superseded by
  `REQ-CORE-TRANSPORT-001`, `REQ-CORE-TRANSPORT-002`, and the shared
  `AtmProtocol` / `ClientTransport` family rather than by a graft-private core
  requirement.
  The generic session identifier name reserved for the thin-client line is
  `AdvisorySessionId`; later advisory/session work must build on that
  generic core-owned name rather than reviving `GraftSessionId`.
- `REQ-CORE-TRANSPORT-001` `atm-core` owns the shared `AtmProtocol` contract
  used by client transport, server transport, and in-process test transport. Satisfies:
  `REQ-P-CONTRACT-001`, `REQ-P-TEST-001`.
- `REQ-CORE-TRANSPORT-002` `atm-core` owns the public `ClientTransport` and
  `ServerTransport` contracts plus route-selection semantics between local and
  cross-host daemon paths. Satisfies:
  `REQ-P-CONTRACT-001`, `REQ-P-RELIABILITY-001`.
- `REQ-CORE-TRANSPORT-003` `atm-core` owns the typed transport timeout and
  retry semantics exposed at service boundaries. Satisfies:
  `REQ-P-RELIABILITY-001`.
- `REQ-CORE-TRANSPORT-004` `atm-core` owns the remote-acceptance and
  no-durable-remote-outbox semantics above transport implementations. Satisfies:
  `REQ-P-RELIABILITY-001`.
- `REQ-CORE-TRANSPORT-005` `atm-core` owns the thread-safe client transport
  contract used by production, fake, and loopback transports. The shared
  `ClientTransport` boundary must remain object-safe and include `Send + Sync`
  semantics so callers do not have to restate them ad hoc. Satisfies:
  `REQ-P-TEST-001`, `REQ-P-CONTRACT-001`.
- `REQ-CORE-TRANSPORT-006` `atm-core` owns the shared ATM wire-frame schema
  and framed encode/decode helpers used by same-host local IPC, cross-host
  daemon transport, and in-process protocol tests. The canonical wire contract
  is documented in `docs/atm-daemon/protocol-icd.md`, including exact header
  constants, `message_kind` assignments, and payload DTO mapping. Satisfies:
  `REQ-P-CONTRACT-001`, `REQ-P-RELIABILITY-001`.
- `REQ-CORE-LOCK-RETIRE-001` `atm-core` owns the service-layer rule that normal
  ATM mail correctness must not depend on mailbox locks in the current
  SQLite/daemon architecture. Satisfies:
  `REQ-P-RELIABILITY-001`.
- `REQ-CORE-DOCTOR-002` `atm-core` owns the daemon-health query contract
  consumed by `atm doctor`. Satisfies:
  `REQ-P-DOCTOR-001`, `REQ-P-OBS-001`.
- `REQ-CORE-TEST-RUNTIME-001` `atm-core` owns the rule that core correctness is
  testable in process without daemon spawning. Satisfies:
  `REQ-P-TEST-001`.
- `REQ-CORE-QA-RUNTIME-001` `atm-core` owns the current runtime QA invariants for
  daemon singleton/runtime and boundary enforcement. Satisfies:
  `REQ-P-ACCEPTANCE-001`, `REQ-P-TEST-001`.

## 4. Module Ownership

Per-module documentation lives under:

- [`modules/list.md`](./modules/list.md)
- [`modules/send.md`](./modules/send.md)
- [`modules/read.md`](./modules/read.md)
- [`modules/ack.md`](./modules/ack.md)
- [`modules/clear.md`](./modules/clear.md)
- [`modules/log.md`](./modules/log.md)
- [`modules/doctor.md`](./modules/doctor.md)
- [`modules/mailbox.md`](./modules/mailbox.md)
- [`modules/config.md`](./modules/config.md)
- [`modules/observability.md`](./modules/observability.md)
- [`modules/team_admin.md`](./modules/team_admin.md)

Each module document defines:

- service responsibility
- invariants
- inputs and outputs
- references to the product requirements it implements

## 5. Required References

The `atm-core` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../project-plan.md`](../project-plan.md)
- [`../documentation-guidelines.md`](../documentation-guidelines.md)
- [`../atm-message-schema.md`](../atm-message-schema.md)
- [`../legacy-atm-message-schema.md`](../legacy-atm-message-schema.md)
  (historical only; its `metadata.atm` coverage was superseded and removed
  from the active compatibility design in Phase U)
- [`../atm-error-codes.md`](../atm-error-codes.md)
- [`../plan-phase-R.md`](../plan-phase-R.md)
- [`../plan-phase-S.md`](../plan-phase-S.md)
- [`../plan-phase-U.md`](../plan-phase-U.md)
- [`../testing-guidelines.md`](../testing-guidelines.md)
- [`../atm-daemon/protocol-icd.md`](../atm-daemon/protocol-icd.md)
- [`./boundaries.md`](./boundaries.md)
- [`./design/dedup-metadata-schema.md`](./design/dedup-metadata-schema.md)
- [`./design/sc-observability-integration.md`](./design/sc-observability-integration.md)
- [`./design/sc-obs-1.0-integration.md`](./design/sc-obs-1.0-integration.md)

## 6. Phase R Store And Boundary Requirements

Requirement IDs:
- `REQ-CORE-RUNTIME-001`
- `REQ-CORE-STORE-001`
- `REQ-CORE-STORE-002`
- `REQ-CORE-INGEST-001`
- `REQ-CORE-BOUNDARY-001`

Required `atm-core` crate rules:
- `atm-core` owns the service-layer API for:
  - message persistence
  - ack/task persistence
  - read/clear visibility persistence
  - team roster persistence
- `atm-core` owns the trait boundaries for:
  - `MailStore`
  - `TaskStore`
  - `RosterStore`
  - inbox ingress
  - inbox export
  - config ingress
  - watcher / reconcile
  - notifier-facing service integration
- `atm-core` owns the canonical durable-store contract including:
  - `messages`
  - one unified mutable message-state surface
  - one canonical roster/team/member surface
  - `inbox_ingest`
- `atm-core` must keep daemon-owned live `pid` state out of the canonical
  roster/member surface
- `atm-core` must treat `config.json` / `TeamConfig` as config-ingress input
  rather than the durable roster/store contract
- `atm-core` owns the canonical `message_key` identity format and the required
  dedupe / lookup indexes above the store boundary
- `atm-core` must model `message_key` as a semantic newtype at the service and
  store boundaries; durable identities must not remain raw `String` values
- `atm-core` must model resource-cap and timeout settings with typed wrappers
- `ClientTransport` remains the shared request/response seam for:
  - production same-host local IPC transport
  - production remote daemon peer transport
  - fake in-process transport doubles
  - loopback in-process transport
- `ClientTransport` must be strong enough for shared ownership and concurrent
  request execution without downstream callers restating `Send + Sync`
- `atm-core` owns one ATM frame contract for local IPC and remote daemon
  request/response transport, as defined by
  `docs/atm-daemon/protocol-icd.md`
- the current shared daemon packet family covers:
  - send compose
  - send acknowledge
  - receive
  - clear
  - doctor
  - heartbeat
  - advisory register
  - advisory unregister
  - advisory fetch
  - advisory drain
  - advisory stream
- `atm-core` framed transport helpers must delimit packets explicitly rather
  than relying on EOF/connection shutdown to mark request boundaries
  rather than passing raw integer literals through the service boundary
- `atm-core` owns the ingest replay/degradation contract and must not silently
  drop parseable external rows
- `atm-core` must not let command/service code access SQLite, inbox JSONL,
  `config.json`, or sockets except through the owning boundary
- `atm-core` must not let watcher/reconcile logic bypass the owned ingress or
  store boundaries
- `atm-core` boundary traits are sealed by default; any boundary that must
  remain externally implementable requires an explicit ADR and crate-doc note
- `atm-core` must keep concrete adapter implementations and constructors
  private unless public exposure is required by a documented boundary contract
- `atm-core` must keep business logic testable in-process without daemon
  process spawning
- `atm-core` must model fallible runtime behavior with typed error enums and
  `Result` propagation rather than panic/unwrap on routine failure paths
- `atm-core` must define ATM-owned structured event/error models that both the
  CLI and daemon layers emit through `sc-observability`
- `atm-core` store implementations must enforce WAL-mode, foreign-key, and
  explicit-transaction policy through the owning store boundary
- `atm-core` defines the store contracts; the first concrete SQLite
  implementation lives in `atm-rusqlite`

Phase-Q crate-local supersession note:
- earlier daemon-free phrasing in this file is historical from the prior line
- for the current SQLite/daemon architecture, the requirements in this section
  and in product `requirements.md` Section 21 are authoritative

## 7. Send Alert Metadata

Requirement ID:
- `REQ-CORE-SEND-002`

Required write-path rules:
- ATM-owned alert/repair machine state must not be introduced back into shared
  Claude JSON under any new namespace
- new ATM-only alert top-level fields must be rejected with a descriptive
  validation error on the write path
- structured alert/repair state belongs in SQLite-owned state and typed ATM
  diagnostics instead

Required read-path rules:
- ATM read must accept legacy top-level alert fields such as `atmAlertKind` and
  `missingConfigPath` while they still exist in compatibility data
- malformed additive ATM alert fields must degrade gracefully, emit warning
  diagnostics, and never cause the message to be dropped when the
  Claude-native envelope remains usable

## 8. Observability Integration Boundary

Requirement ID:
- `REQ-CORE-OBS-001`

Required boundary rules:
- `atm-core` owns the injected observability boundary used by retained command
  services
- `atm-core` must not depend on concrete `sc-observability` crate types
- the public `atm-core` observability boundary must not expose
  `serde_json::Value`, `serde_json::Map`, or other serialization-format types
  directly
- the boundary must cover emit, query, follow, and health rather than
  remaining emit-only
- ATM-owned projected request/result types must be defined in `atm-core` for:
  - log query
  - log record projection
  - tail-session projection
  - doctor health projection
- the boundary must remain synchronous and object-safe for service injection
- shared query/follow and health failures must map to stable `AtmErrorKind`
  variants without leaking shared error enums into `atm-core`
- `atm-core` command-service failures and degraded recovery warnings must expose
  stable ATM-owned error codes for the CLI observability adapter to log
- the corresponding source-of-truth code registry must live in one source file
  and match [`../atm-error-codes.md`](../atm-error-codes.md)

Required public field-model rules:
- `LogFieldKey` is the validated ATM-owned field-name type used by retained-log
  queries and projected records
- `AtmJsonNumber` is the validated ATM-owned representation for JSON numeric
  literals at the observability boundary
- `LogFieldValue` is the ATM-owned recursive value model with variants for:
  - null
  - bool
  - string
  - number (`AtmJsonNumber`)
  - array of `LogFieldValue`
  - object (`LogFieldMap`)
- `LogFieldMap` is the ATM-owned map type used by `AtmLogRecord.fields`
- `LogFieldMatch` must use `LogFieldKey` + `LogFieldValue`
- `AtmLogRecord.fields` must use `LogFieldMap`
- `AtmJsonNumber` must accept any valid RFC 8259 JSON number and reject
  non-JSON numeric values such as `NaN`, `Infinity`, and `-Infinity`
- construction of `AtmJsonNumber` must return
  `Result<AtmJsonNumber, AtmError>`
- serialization of these ATM-owned types must preserve the current CLI JSON
  wire shape for retained-log commands
- conversion to and from raw `serde_json` values must remain centralized inside
  `atm-core`

Detailed design and implementation shape is owned by:
- [`design/sc-observability-integration.md`](./design/sc-observability-integration.md)
  for the historical Phase K boundary expansion rationale
- [`design/sc-obs-1.0-integration.md`](./design/sc-obs-1.0-integration.md)
  for the active Phase L release-alignment decisions, including the L.4 public
  boundary cleanup

## 9. Config And Team Baseline Semantics

Requirement ID:
- `REQ-CORE-CONFIG-001`

Required config rules:
- `atm-core` reads ATM-owned config only from the `[atm]` section of
  `.atm.toml`
- `atm-core` ignores launcher-owned sections such as `[rmux]` and future
  `[scmux]`
- `[atm].default_team` remains the shared team default
- `[atm].team_members` defines the baseline team roster that should always be
  present in `config.json`
- `[atm].aliases` may define ATM-owned shorthand names for canonical agent
  identities
- `[[atm.post_send_hooks]]` may define ATM-owned best-effort post-send
  automation rules
- retired `[atm].post_send_hook`, `[atm].post_send_hook_senders`,
  `[atm].post_send_hook_recipients`, and `[atm].post_send_hook_members` must
  fail with migration guidance to `[[atm.post_send_hooks]]` rather than being
  treated as compatibility aliases
- `[atm].identity` is obsolete and must not participate in runtime identity
  resolution; doctor should report it as configuration drift when present

Required identity rules:
- runtime identity must come from explicit command override, hook identity, or
  `ATM_IDENTITY`
- if no valid runtime identity exists where a command requires one, the command
  must fail with a structured recovery-oriented error rather than inventing a
  normal sender identity
- aliases are input shorthand only until ATM resolves them to canonical member
  names
- recipient aliases must resolve before membership validation, self-send
  checks, and mailbox lookup
- same-team messages keep current canonical sender projection behavior
- cross-team messages may persist an alias-oriented `from` value for
  Claude-facing ergonomics only when ATM also stores canonical sender identity
  in SQLite-owned state
- canonical sender identity remains the source of truth for validation,
  self-send checks, routing, and audit behavior
- each `[[atm.post_send_hooks]]` rule binds one `recipient` selector and one
  `command` argv
- `recipient` must be one concrete recipient name or `*`
- rules with empty recipient or empty command must fail during config loading
- multiple matching rules may run for one send, in config order
- recipient non-match is expected behavior and must be silent
- a relative hook command path resolves from the discovered `.atm.toml`
  directory, and the hook executes with that same directory as its working
  directory
- bare executable names such as `bash`, `python3`, or `tmux` must use normal
  `PATH` resolution
- the hook inherits process environment and also receives one ATM-owned JSON
  payload in `ATM_POST_SEND` with:
  - `from`
  - `to`
  - `sender`
  - `recipient`
  - `team`
  - `message_id`
  - `requires_ack`
  - `is_ack`
  - optional `task_id` when present
  - optional `recipient_pane_id` when authoritative roster truth includes a
    pane mapping for the recipient
- the hook must run after successful non-`dry-run` `atm send`
- the hook must also run after successful `atm ack`, using the reply message as
  the hook subject
- `is_ack` must be `false` for `atm send` and `true` for `atm ack`
- the hook may optionally emit one structured stdout result with `level`,
  `message`, and optional `fields`; ATM logs it on a best-effort basis and
  ignores absent or invalid output
- hook-rule evaluation and execution outcomes must remain observable through
  structured diagnostics without creating caller-visible warnings for expected
  recipient non-match
- once roster truth is stored in SQLite, `atm-core` must source
  `recipient_pane_id` from the authoritative roster/store boundary rather than
  forcing hooks to rediscover it from local files
- hook failure or timeout is best-effort only and must not roll back a
  successful send
- the reserved sender `atm-identity-missing@<team>` is available only for
  ATM-generated repair/diagnostic notices and must not become a general
  identity fallback

Required doctor rules:
- `atm doctor` must flag obsolete `[atm].identity` when present with
  `ATM_WARNING_IDENTITY_DRIFT`
- `atm doctor` must compare `[atm].team_members` against `config.json.members`
- missing baseline members are findings
- extra runtime members in `config.json` are allowed
- doctor roster output must show all `config.json` members, with baseline
  members first and `team-lead` first among the baseline set
- `atm doctor` must snapshot `~/.claude/teams/*/inboxes/*.lock` at start and
  end of the run; any lock path present in both snapshots is stale and must be
  reported with `ATM_WARNING_STALE_MAILBOX_LOCK` plus `rm -f <path>` recovery guidance

## 10. Retained Team Recovery Surface

Requirement ID:
- `REQ-CORE-TEAM-001`

Required service rules:
- `atm-core` owns the retained local team recovery surface for:
  - discovered-team listing
  - local member listing
  - `add-member`
  - team backup
  - team restore
- these services remain local user-facing workflows and must not depend on
  daemon orchestration or runtime spawning, but their team/member truth is the
  canonical ATM roster rather than raw `config.json`
- `atm members` and `atm teams` must report ATM roster truth; Claude file drift
  is reported by `atm doctor`, not by treating `config.json` as the primary
  team/member surface
- `add-member` must validate team existence and reject duplicate member names
  before mutating canonical ATM roster truth
- `add-member` must project the resulting approved member set into
  `config.json`; it must not treat local `config.json` as the durable source
  of truth
- retained Claude compatibility member fields such as `tmux_pane_id` are
  canonical ATM roster-member metadata and must not be durably sourced from
  `.atm.toml`
- backup must snapshot:
  - `config.json`
  - team inbox files, excluding transient `*.lock` sentinels, dotfiles, and
    restore markers
  - the ATM team task bucket
  - ATM-owned durable roster/task state needed for deterministic restore
- restore must:
  - require the operator to recreate the Claude team shell through
    `TeamCreate` before ATM projects restored roster state back into
    `config.json`
  - preserve the current team-lead entry and current `leadSessionId`
  - restore non-lead membership from canonical ATM roster truth rather than
    from backup `config.json`
  - preserve canonical member metadata such as `tmux_pane_id`
  - restore non-lead inboxes
  - sweep stale inbox `*.lock` sentinels before copying restored inbox files
  - recompute `.highwatermark` from the maximum restored task id
  - support a dry-run path without making changes
- restore must not replay backup `config.json` as roster truth
- backup remains an audit / emergency-inspection surface rather than the source
  of roster truth during restore
- malformed or missing snapshot material must fail with structured errors
  before partial restore is committed
- `members` must remain useful as a local roster inspection command even when
  daemon or hook state is unavailable because roster truth is stored in ATM
  durable state rather than in raw Claude config alone

## 10.1 Claude Config Ingress Ownership

Requirement ID:
- `REQ-CORE-CLAUDE-ROSTER-001`

Required service rules:
- ATM roster state in SQLite is the canonical roster truth for runtime
  membership decisions
- the immutable public runtime roster surface is `ClaudeCodeTeamRoster`
- retained runtime commands (`list`, `read`, `clear`, `ack`) must validate
  membership through ATM roster truth only
- Claude send must not use `config.json` as a pre-write membership gate
- the post-write Claude roster warning path must build
  `ClaudeCodeTeamRoster` from canonical ATM roster rows through `RosterStore` /
  SQLite rather than through a direct `config.json` read
- `doctor` may read `config.json` only as a comparison surface against
  canonical ATM roster truth
- watcher / reconcile is the only approved production reader of external
  `config.json` roster changes
- any new-team ingest or external Claude roster edit must flow into canonical
  ATM roster truth through the watcher / reconcile lane
- daemon-authored Claude `config.json` projection writes must be suppressed
  once and only once through an explicit process-local journal; restart must
  clear suppression state and crash recovery must fall back to ordinary
  idempotent external ingest

## 10.2 Config Boundary Static Gates

Requirement ID:
- `REQ-CORE-CLAUDE-ROSTER-002`

Required service rules:
- repository-local lint / `sc-lint`-candidate rules must be defined for the
  `config.json` roster boundary so later regressions are mechanically
  detectable
- the first required rules are:
  - reject production direct `load_claude_team_config_document(...)` roster reads
    outside the explicit allowlist
  - reject generic runtime team-config helper use from retained command paths
  - reject Claude send paths that consult `config.json` before the durable ATM
    write has succeeded

## 11. Phase Yb Delivery-Plan Ownership

Requirement IDs:

- `REQ-ATM-CORE-YB-001`
- `REQ-ATM-CORE-YB-002`

Required service rules:

- `atm-core` must define `atm_core::delivery_plan::DeliveryPlan` and
  `atm_core::delivery_plan::ReplyDeliveryPlan`
- `atm-core` must define the shared execution entry points:
  - `atm_core::delivery_execution::execute_delivery_plan(...)`
  - `atm_core::delivery_execution::execute_reply_delivery_plan(...)`
- `atm-core` must define `atm_core::boundary::NonClaudeOutbound` as the
  first-class non-Claude payload boundary
- `NotificationSink` must remain notification-only
- `send/mod.rs`, `send/persistence.rs`, and `ack/mod.rs` must not perform
  harness policy after the machine output plan exists
