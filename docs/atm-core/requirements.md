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
- subsystem doctor trait contracts and shared doctor report DTOs
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
- `REQ-CORE-MAILBOX-001` retains current store-backed mailbox atomicity and
  consistency ownership; only its earlier Claude mailbox JSON compatibility
  aspects are historical after `ADR-019`.
- `REQ-CORE-COMPAT-001` `atm-core` owns the direct post-send and native-agent
  compatibility contract documented in product `requirements.md` Section 22.5.
  Satisfies:
  `REQ-P-CONTRACT-001`, `REQ-P-RELIABILITY-001`.
- `REQ-CORE-WORKFLOW-001` `atm-core` owns the two-axis workflow model and legal
  transitions. Satisfies the state-classification and legal-transition aspects
  of:
  `REQ-P-LIST-001`, `REQ-P-READ-001`, `REQ-P-ACK-001`, `REQ-P-CLEAR-001`,
  `REQ-P-WORKFLOW-001`.
- `REQ-CORE-TEMPLATE-WORKFLOW-001` `atm-core` owns transport-neutral validation
  and admission mapping above the `atm-storage` leaf DTOs for an optional
  template-declared workflow snapshot.
  It must treat scope kind, state, stage, transition, iteration variable, and
  template tags as bounded opaque data, resolve only declared merged-variable
  references, and reject partial declarations or reserved-tag spoofing before
  storage mutation. It satisfies `REQ-P-TEMPLATE-WORKFLOW-001` and
  `REQ-P-TEMPLATE-TAGS-001` per ADR-046.
- `REQ-CORE-WORKFLOW-ANALYTICS-001` `atm-core` owns generic local lifecycle
  query/projection contracts over durable workflow snapshots. It must not
  define process vocabulary, join current catalog metadata to rewrite history,
  or make telemetry a routing/admission input. It satisfies
  `REQ-P-WORKFLOW-ANALYTICS-001` per ADR-046.
- `REQ-CORE-LIST-001` `atm-core` owns the metadata-first queue query contract
  shared by `atm list` and selector-driven `atm read`, including bounded
  query behavior, shared match filters, successor-chain terminal-node
  selection, and list-row shaping. Satisfies:
  `REQ-P-LIST-001`, `REQ-P-READ-001`, `REQ-P-RELIABILITY-001`.
  U.3 note: logical-current projection must remain mode-aware; terminal
  `add-details` preserves predecessor context in the effective current body,
  while terminal `supersede` does not.
- `REQ-CORE-SEND-003` `atm-core` owns send-path message construction,
  classification, and direct post-send-emission behavior above the owned
  boundaries. Satisfies the send-path service aspects of:
  `REQ-P-SEND-001`, `REQ-P-IDLE-001`.
- `REQ-CORE-LOG-001` `atm-core` owns ATM log query/follow service behavior over
  the injected observability boundary. Satisfies the core
  query/follow/filtering aspects of:
  `REQ-P-LOG-001`, `REQ-P-OBS-001`.
- `REQ-CORE-DOCTOR-001` `atm-core` owns local doctor diagnostics and readiness
  evaluation. Satisfies the diagnostic evaluation aspects of:
  `REQ-P-DOCTOR-001`, `REQ-P-OBS-001`.
  Phase-AA note:
  - `MailStore` and `RosterStore` remain the primary storage-neutral
    capability surfaces in the historical Phase-AA line
  - `MailStoreDoctor`, `RosterStoreDoctor`, and `ConfigDoctor` are the
    subsystem-owned doctor traits that freeze the aggregate-only daemon doctor
    model
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
  (`AtmMessageId`), and required lookup/dedupe constraints for the retained
  ULID `message_id` wire form.
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
  store, protocol, transport, config ingress, direct post-send emission, and
  status-source calls. Satisfies the subsystem-boundary aspects of:
  `REQ-P-CONTRACT-001`, `REQ-P-TEST-001`.
  Phase-Z follow-on note: repository-local lint must also be able to reject
  direct command-entry retained-runtime acquisition in `atm teams`,
  `atm members`, and `atm teams add-member`; direct command/team-admin ambient
  workspace-config reads; and public ambient singleton/runtime-factory
  exposure that bypasses approved wrappers. A pre-existing survivor is allowed
  only when an explicit TOML allowlist entry records its owner and sunset
  sprint; any new violation must fail lint immediately.
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
- `REQ-CORE-GRAFT-001` remains active. Phase U retired the earlier
  graft-private transport implementation, not the requirement that graft stay
  a thin daemon client and bounded host-wake transport. The shared
  `REQ-CORE-TRANSPORT-001`, `REQ-CORE-TRANSPORT-002`, and HTTP application
  contract satisfy its client operations; no graft-private lifecycle, session,
  queue, or stream identifier is reserved in shared `atm-core`. Receiver-private
  runtime state belongs in the receiver implementation unless it is proven to
  be shared ATM semantics.
- `REQ-CORE-TRANSPORT-001` `atm-core` owns the transport-neutral application
  request/response types: `AgentAddress`, `WriteRequest`, read/query inputs,
  and canonical message projections. Local UDS HTTP, normal remote HTTPS, the
  explicit daemon-only plaintext-test peer profile, and the in-process test
  adapter decode to or encode from these same types. The plaintext profile is
  untrusted provenance only and cannot create a second application path.
  Satisfies:
  `REQ-P-CONTRACT-001`, `REQ-P-TEST-001`.
- `REQ-CORE-TRANSPORT-002` `atm-core` owns the typed destination-host field
  and post-write routing contract. Exactly one post-write router invokes the
  receiver hook after a newly persisted inbound write (peer provenance or an
  empty host) and invokes no hook for an idempotent duplicate. A
  host-qualified origin write retains only the temporary peer wake-up until
  Phase AM deletion, including `localhost` and the daemon's own advertised or
  bound IP. Local CLI HTTP, same-host TCP HTTP, and remote peer HTTP decode the
  same `WriteRequest` through one HTTP write resource; adapter
  authentication/provenance cannot select another write, ACK, persistence, or
  steer-nudge path. A same-host duplicate preserves its origin metadata for a later
  canonical ACK but produces neither a second hook nor peer wake-up. Satisfies:
  `REQ-P-CONTRACT-001`, `REQ-P-RELIABILITY-001`.
- AI.23 shared-write convergence constraint: all local, same-host, and remote
  HTTP writes must enter `ApiRouter::route` with the same `WriteRequest`, then
  pass through `DaemonRequestDispatcher::route_write` and the canonical
  `MessageWriter::write` persistence boundary: persist, then emit the steer
  nudge through exactly one `PostWriteRouter::dispatch`; queue-kind nudges
  defer emission until harness readiness (ADR-054), and neither kind ever
  precedes persistence. Adapters may authenticate and label provenance but
  may not implement a parallel write, acknowledgement, or nudge path. This is
  the crate-level refinement of
  `REQ-CORE-TRANSPORT-001`/`002` and the shared-write-resource contract in
  [`../architecture.md`](../architecture.md).
- `REQ-CORE-TRANSPORT-003` cross-host delivery owns no durable or in-memory
  per-message delivery state: no replay store, outbox, retry queue, receipt,
  remote ack state, or duplicate-delivery subsystem. The sole permitted
  non-durable state is REQ-CORE-TRANSPORT-003B's per-host lease, generation,
  next-attempt time, and backoff for bounded canonical-record scans.
  Storage idempotency is by immutable message ULID. An identical
  already-delivered remote duplicate is a no-op; the narrow same-host peer
  receipt of a retained origin record logs `peer_duplicate_write_skipped`
  with its ULID, source/destination host, `database_write=skipped`, and
  `delivery=continued`; it continues the ordinary inbound steer nudge without a second record or peer
  re-delivery. Satisfies `REQ-P-RELIABILITY-001`.
- `REQ-CORE-TRANSPORT-004` remote acceptance is a normal result of the same
  canonical write. A failed remote attempt creates no remote recipient row or
  delivery state; an already-persisted local sender record remains immutable.
  Satisfies `REQ-P-CONTRACT-001`, `REQ-P-RELIABILITY-001`.
- `REQ-CORE-TRANSPORT-005` `atm-core` owns the sealed, object-safe, `Send + Sync`
  `DaemonApiClient` contract used by CLI, graft, and test clients. It exposes
  one application API; concrete UDS/HTTPS HTTP I/O is adapter-owned. A future
  Python binding consumes this contract in its owning phase and must not create
  a parallel ingress or client trait.
  Satisfies `REQ-P-TEST-001`, `REQ-P-CONTRACT-001`.
- `REQ-CORE-TRANSPORT-006` is historical only. The custom ATM wire-frame
  schema, frame codec, and `ClientTransport` framing contract are retired by
  AI.6 and must not be extended; ADR-033's HTTP/OpenAPI contract replaces them.
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
- [`../atm-daemon/http-api.md`](../atm-daemon/http-api.md)
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
  - read/clear visibility persistence
  - team roster persistence
- `atm-core` owns the trait boundaries for:
  - `MailStore`
  - `RosterStore`
  - inbox ingress
  - inbox export
  - config ingress
  - notifier-facing service integration
- `atm-core` must not retain watch/reconcile as accepted boundary traits after
  `AD.4`; any surviving watch/reconcile DTOs are historical-only protocol
  scaffolding until a later deletion sprint removes them
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
- `DaemonApiClient` is the shared request/response seam for the HTTP/UDS
  production client, HTTPS peer client, and in-process HTTP test adapter.
  It is object-safe and `Send + Sync` so clients do not restate concurrency
  bounds.
- `atm-core` owns transport-neutral request/response domain types only. HTTP
  routes, framing, socket ownership, and response translation are adapter
  concerns under ADR-033; the retired ATM frame ICD is not an accepted contract.
- canonical writes represent both send and acknowledgement. An ack differs
  only by a populated `acknowledges_message_id`, never by a second request or
  packet family.
- `atm-core` relies on durable message identity and idempotent writes; it owns
  no ingest-replay or deferred-delivery persistence contract
- `atm-core` must not let command/service code access SQLite, mailbox JSON,
  `config.json`, or sockets except through the owning boundary
- `atm-core` boundary traits are sealed by default; any boundary that must
  remain externally implementable requires an explicit ADR and crate-doc note
- `atm-core` must keep concrete adapter implementations and constructors
  private unless public exposure is required by a documented boundary contract
- `atm-core` must keep business logic testable in-process without daemon
  process spawning
- `atm-core` must model fallible runtime behavior with ADR-032's `AtmError`
  and `Result` propagation rather than panic/unwrap on routine failure paths
- `atm-core` must define ATM-owned structured event/error models that both the
  CLI and daemon layers emit through `sc-observability`
- `atm-core` store implementations must enforce WAL-mode, foreign-key, and
  explicit-transaction policy through the owning store boundary
- `atm-core` defines the store contracts; the first concrete SQLite
  implementation now lives in `atm-storage-rusqlite`

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
- shared query/follow and health failures must map to stable `AtmErrorCode`
  values through ADR-032's `AtmError` without leaking backend error enums into
  `atm-core`
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
- `[atm].default_team` remains the shared config default for ATM-owned
  config/bootstrap flows that explicitly consume config defaults; it is not a
  runtime caller-team fallback for commands governed by the caller-context
  matrix
- `[atm].team_members` defines the baseline team roster that should always be
  present in `config.json`
- `[atm].aliases` may define ATM-owned shorthand names for canonical agent
  identities
- `[[atm.post_send_hooks]]` may define ATM-owned external override commands for
  post-send behavior; they are not the only shipped post-send path
- retired `[atm].post_send_hook`, `[atm].post_send_hook_senders`,
  `[atm].post_send_hook_recipients`, and `[atm].post_send_hook_members` must
  fail with migration guidance to `[[atm.post_send_hooks]]` rather than being
  treated as compatibility aliases
- `[atm].identity` and the legacy top-level `identity` key are obsolete and
  must not participate in runtime identity resolution; doctor should report
  them as configuration drift when present

Required caller-context rules:
- the authoritative command-by-command caller-context matrix is
  `docs/requirements.md` §4.1
- `atm-core` must implement that matrix exactly; crate-local code must not
  widen accepted caller identity/team sources beyond the product matrix
- `atm-core` owns the service-layer mailbox split:
  - `peek` and `list` are inspection-only queries
  - `send`, `read`, `ack`, and `clear` are owner-only mutating operations
- only inspection-only service calls may accept an
  impersonation-equivalent caller-context override
- where a command requires caller identity, runtime identity must come from the
  documented explicit command override when supported or invoking-shell
  `ATM_IDENTITY`
- where a command requires caller team, runtime team must come from the
  documented explicit command override when supported or invoking-shell
  `ATM_TEAM`
- if no valid required caller context exists, the command must fail with a
  structured recovery-oriented error rather than inventing a normal sender
  identity or caller team
- caller-context-owned command entry points must reject unresolved context
  before daemon dispatch
- downstream caller-owned request DTOs must carry resolved required caller
  context as required fields
- `atm-core` must not treat hook files, repo-local config, roster state, or
  daemon ambient `ATM_IDENTITY` / `ATM_TEAM` as fallback caller context
- `atm doctor` is the explicit exception: it remains identity-free and
  optional-team, while still inspecting caller-context visibility
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
- canonical same-team self-addressed sends with no destination host must fail
  in the shared `atm-core` send path before persistence and before any
  `dry-run` success result; every syntactically valid host-qualified target
  proceeds to the ordinary host-routing contract
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
  - `sender`
  - `recipient`
  - `team`
  - `message_id`
  - `description`
  - `task_id` as a string; it may be empty
  - `requires_ack`
  - `is_ack`
  - optional `to`
  - optional `recipient_pane_id` when authoritative roster truth includes a
    pane mapping for the recipient
- the hook must run after successful non-`dry-run` `atm send`
- the hook must also run after successful `atm ack`, using the reply message as
  the hook subject when ack emitted a reply
- if `atm ack` suppresses an unqualified same-agent/same-team historical
  self-addressed reply, the acknowledgement still succeeds but no ack hook
  fires because no outbound reply message exists. A host-qualified source is
  never suppressed and uses the ordinary canonical ACK write.
- `is_ack` must be `false` for `atm send` and `true` for `atm ack`
- hook configuration lookup must resolve from the sender's authoritative ATM
  roster `home_dir` metadata
- if no matching external rule is configured, `atm-core` must still hand off
  the canonical post-send event to the shipped built-in in-process delivery
  path; any retained `atm internal-nudge` helper uses the same resolved event
  shape through `InternalNudgeEnvelope`
- the hook may optionally emit one structured stdout result with `level`,
  `message`, and optional `fields`; ATM logs it on a best-effort basis and
  ignores absent or invalid output
- hook-rule evaluation and execution outcomes must remain observable through
  structured diagnostics without creating caller-visible warnings for expected
  recipient non-match
- once roster truth is stored in SQLite, `atm-core` must source
  `recipient_pane_id` from the authoritative roster/store boundary rather than
  forcing hooks to rediscover it from local files
- repo-tracked `.atm.toml` is dogfood/bootstrap config only; it must not carry
  live post-send pane-routing authority through committed
  `[[rmux.windows.panes]].tmux_pane_id` values
- `atm-core` owns canonical post-send event construction plus the shared
  resolved-template helper used by retained built-in nudge paths, but it must
  not own sink-local transport behavior
- any team-scoped built-in template override lookup must cross a dedicated
  storage-neutral `NudgeTemplateOverrideStore` boundary before
  `MessageReceivedHookEmitter` runs; `atm-core` must not perform direct SQLite lookup
  inside the emitter path
- the accepted built-in template lifecycle is explicit:
  - no row => product default
  - override row => stored non-empty template body
  - disabled row => no built-in nudge emission
  - clear/reset => delete the row and fall back to product default
- empty-string template bodies are invalid ATM input and must not be used as a
  hidden disable signal at any layer
- any retained built-in helper envelope is separate from the external hook
  payload:
  - external hooks receive `ATM_POST_SEND`
  - retained `atm internal-nudge` helper receives `ATM_INTERNAL_NUDGE`
  - `ATM_INTERNAL_NUDGE` carries the canonical event, sink target, resolved
    template kind, and resolved template body or explicit disabled state
- hook failure or timeout is best-effort only and must not roll back a
  successful send
- the reserved sender `atm-identity-missing@<team>` is available only for
  ATM-generated repair/diagnostic notices and must not become a general
  identity fallback

Required doctor rules:
- `atm doctor` must flag obsolete config identity fields (`[atm].identity` and
  legacy top-level `identity`) when present with `ATM_WARNING_IDENTITY_DRIFT`
- `atm doctor` must compare canonical ATM roster truth against
  `config.json.members`
- ATM roster members missing from `config.json` are findings
- Claude `config.json` members missing from ATM roster truth are findings
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
  - `update-member`
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
- `add-member` must persist the member's durable `home_dir` on the canonical
  ATM roster row and project that same `home_dir` into compatibility
  `config.json.members`
- `update-member` must validate team existence and require an existing member
  before mutating canonical ATM roster truth
- `update-member` must be the accepted repair path for mutable canonical member
  metadata including `home_dir`, `recipient_pane_id`, `harness`,
  `agent_type`, and `model`
- `update-member` must reject operator-settable `cwd`, `live_cwd`, and
  `launch_cwd`
- `atm-core` terminology must keep these meanings distinct:
  - `home_dir` = durable SQL-backed agent-home directory for the member; for
    worktree-backed members it preserves the worktree home and the canonical
    association back to the owning main repo
  - `live_cwd` = runtime-only working-directory overlay for the invoking ATM
    member when the active CLI/doctor process can bind `ATM_IDENTITY` to that
    displayed member; it is not durable roster metadata
  - `launch_cwd` = startup-only current-directory snapshot emitted to ATM CLI
    startup logs; it is not durable roster metadata
- no accepted `atm-core` surface may use bare `cwd` when `live_cwd` or
  `launch_cwd` is the real meaning
- `atm-core` must prefer extending existing roster-row and runtime-roster
  shapes over introducing new directory-tracking coordinator structs
- `add-member` remains create-only and must not be overloaded as an update path
- `atm teams` and `atm members` must source their displayed team/member state
  from canonical ATM roster rows, not from raw Claude file membership
- retained Claude compatibility member fields such as `tmux_pane_id` are
  canonical ATM roster-member metadata and must not be durably sourced from
  `.atm.toml`
- ATM-owned member pane metadata is stored on the canonical roster row as
  `recipient_pane_id` and projected back to Claude compatibility
  `AgentMember.tmux_pane_id`
- backup must snapshot:
  - `config.json`
  - team inbox files, excluding transient `*.lock` sentinels, dotfiles, and
    restore markers
  - the ATM team task bucket
  - ATM-owned durable roster/task state needed for deterministic restore
  - the canonical ATM roster audit snapshot as `atm-roster.json`
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
- restore may read the freshly recreated Claude team shell only through a
  narrow preservation helper for current `team-lead` / `leadSessionId`, not
  through a generic roster-truth config loader
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
- the immutable public runtime roster surface is `ProjectionRoster`
- retained runtime commands (`list`, `read`, `clear`, `ack`) must validate
  membership through ATM roster truth only
- Claude send must not use `config.json` as a pre-write membership gate
- the post-write Claude roster warning path must build
  `ProjectionRoster` from canonical ATM roster rows through `RosterStore` /
  SQLite rather than through a direct `config.json` read
- `doctor` may read `config.json` only as a comparison surface against
  canonical ATM roster truth
- external `config.json` edits are not an accepted production ingress path for
  canonical ATM roster truth
- watcher / reconcile is historical only and must not remain the governing
  production reader for external Claude roster edits
- if ATM later needs an external-roster import surface, it must be an explicit
  documented admin or CLI action against canonical ATM roster truth rather
  than a daemon watch/reconcile side channel
- daemon-authored Claude `config.json` projection writes must be suppressed
  once and only once through an explicit process-local journal; restart must
  clear suppression state and crash recovery must fall back to ordinary
  idempotent projection comparison behavior rather than a hidden external
  ingest lane

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

## 11. Historical Phase Yb Delivery-Plan Ownership

Requirement IDs:

- `REQ-ATM-CORE-YB-001`
- `REQ-ATM-CORE-YB-002`

Historical note:

- this section records the earlier Yb delivery-plan model only
- `ADR-019` supersedes it for the accepted runtime
- new work must use the direct post-send emitter seam instead of treating
  `DeliveryPlan`, `ReplyDeliveryPlan`, or `NotificationSink` as the governing
  send-path contract
