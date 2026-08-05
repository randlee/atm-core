# ATM-Core Crate Architecture

## 1. Purpose

This document defines the `atm-core` crate architectural boundary.

It complements the product architecture in
[`../architecture.md`](../architecture.md) and owns crate-local structure and
service boundaries.

The crate-local machine-readable boundary inventory lives in:
- [`./boundaries.md`](./boundaries.md)

## 1.1 ADRs

## Shared ATM protocol lives in atm-core

```yaml
adr_id: ADR-ATM-CORE-001
crate: atm-core
title: Shared ATM protocol lives in atm-core
status: accepted
date: 2026-05-03
deciders:
  - team-lead
  - arch-ctm
tags:
  - protocol
  - contracts
related_boundaries:
  - BOUNDARY-AtmProtocol
  - BOUNDARY-ClientTransport
  - BOUNDARY-ServerTransport
  - BOUNDARY-RequestDispatcher
code_references:
  - docs/atm-core/boundaries.md
  - docs/atm-daemon/boundaries.md
  - docs/atm/boundaries.md
```

Context:
- The protocol is shared by CLI clients, daemon server/runtime, in-process
  transport tests, and thin extension crates.

Decision:
- `AtmProtocol` is owned by `atm-core`, not by `atm-daemon`.
- `atm-core` also owns the public transport and dispatcher contracts that
  operate over that protocol.

Consequences:
- Thin callers do not need daemon-shaped API types.
- Client and server transports share one contract family.
- The `atm-graft` crate is allowed only as a thin consumer of that shared
  contract family; it must not introduce a second daemon-specific client API.
- Thin plugin crates must stay on the shared contract family for unary
  command/request behavior and must not introduce receiver-private stream or
  session APIs into the shared daemon contract.

Alternatives considered:
- Keep the protocol modeled as daemon API types.
- Move the protocol into a dedicated transport crate first.

Follow-up work:
- Keep crate-local boundary records aligned with this ownership rule.
- Enforce daemon-shaped protocol naming as a lint failure.

Convention note:
- crate-local `atm-core` ADRs may remain embedded in this architecture document
  until they are extracted into standalone `docs/adr/` files

## Ack is folded into send-shaped thin-client requests

```yaml
adr_id: ADR-ATM-CORE-002
crate: atm-core
title: Ack is folded into send-shaped thin-client requests
status: accepted
date: 2026-05-03
deciders:
  - team-lead
  - arch-ctm
tags:
  - workflow
  - protocol
related_boundaries:
  - BOUNDARY-AtmProtocol
code_references:
  - docs/atm-core/boundaries.md
  - docs/requirements.md
```

Context:
- Thin client surfaces should expose as few methods as possible while still
  preserving ATM workflow semantics.

Decision:
- `ack` remains a retained user workflow, but thin-client protocol surfaces
  carry acknowledgement through send-shaped request data rather than a separate
  top-level protocol family.

Consequences:
- Thin extensions expose a smaller public surface.
- Task-state rules still remain explicit in store and workflow boundaries.
- The reply emitted by that workflow must hardcode `requires_ack = false`.
- Shared core request and query surfaces must resolve ULID text only to
  `AtmMessageId`.
- Send-shaped request data may carry `parentMessageId` and `threadMode` when
  the caller is creating a successor-thread message.
- Update/correction threads are modeled as one linear successor chain whose
  terminal node is the effective current instruction.
- The effective current instruction is mode-aware:
  - terminal `add-details` keeps the terminal id but composes the still-valid
    predecessor context into the current body
  - terminal `supersede` exposes only the replacement body
- Only the original sender may append chain successors.
- Ack is evaluated at the chain level rather than per-node toggle churn, with
  the root message establishing whether the thread is ack-required.

Alternatives considered:
- Preserve a first-class top-level `ack` protocol method family.

Follow-up work:
- Align thin-client docs with the `send` / `receive` shape.
- Align successor-chain and ephemeral-retention rules with the retained
  product requirements before the SQLite sprint closes.

## 2. Architectural Rules

- `atm-core` exposes request/result/service boundaries, not clap surfaces.
- `atm-core` owns workflow/state transitions and must enforce them by code
  structure.
- `atm-core` owns observability as an injected boundary, not as a concrete
  dependency on `sc-observability`.
- `atm-core` must keep mailbox/config/workflow/log/doctor/team-recovery logic
  reusable across CLI contexts.
- `atm-core` owns persisted config/team loading policy, including compatibility
  defaults, recovery boundaries, and precise parse diagnostics.
- `atm-core` must keep all external I/O behind explicit boundary traits or
  façade interfaces with hidden implementations.
- `atm-core` must keep production failure handling structured with typed
  `Result`/error-enum boundaries rather than routine panic/unwrap paths.
- retained mailbox runtime selection must be fail-closed and store-backed only;
  `atm-core` must not preserve a file-backed mailbox fallback once the Phase X
  cutover line lands
- Claude inbox-append runtime behavior and the former
  `crates/atm-storage-claude` backend are retired from the accepted line;
  retained command/runtime logic must not treat mailbox JSON append as a
  second durable or governing runtime backend, and the shared backend
  contract remains the required seam for future backend implementations

Observability release boundary rules:
- raw `serde_json::Value` / `serde_json::Map` remain internal translation types
  only; they are not part of the public observability contract
- the public L.4 field model uses:
  - `LogFieldKey`
  - `AtmJsonNumber`
  - `LogFieldValue`
  - `LogFieldMap`
- CLI JSON output remains wire-compatible with the current retained-log output
  shape after the boundary cleanup

## 2.1 Phase R Boundary Model

Phase R makes `atm-core` the owner of the service-layer boundaries while the
daemon remains a runtime wrapper only.

Historical-through-AI.5 subsystem boundaries (not accepted Phase AI targets):
- `AtmProtocol` boundary
- `ClientTransport` boundary
- `ServerTransport` boundary
- `RequestDispatcher` boundary
- `MailStore` boundary
- `MailStoreDoctor` boundary
- `RosterStore` boundary
- `RosterStoreDoctor` boundary
- config-ingress boundary
- `ConfigDoctor` boundary
- `MessageReceivedHookEmitter` boundary
- `StatusSource` boundary

Phase AA shared runtime-composition contracts:
- `RuntimeDoctorPorts` is the `atm-core` DTO that groups the storage-neutral
  doctor handles consumed by daemon and direct-doctor callers
- `DoctorFinding` is the shared subsystem diagnostic DTO used by the doctor
  trait family
- ADR-038's bounded canonical-record query crosses through a storage trait;
  no replay persistence boundary exists

Required architectural rules:
- business logic must live in service modules, not in concrete adapters
- concrete I/O implementations stay private behind the owning boundary
- module privacy and hidden constructors are the first enforcement tool even
  before crate extraction
- if a boundary proves fragile, the next step is crate extraction rather than
  boundary bypass
- typed error translation happens at the boundary layer, but must preserve
  discriminated error identity across store/ingress/export/service calls
- `atm-core` owns the service-facing error façade and ATM event models used by
  both CLI and daemon `sc-observability` emitters; the dependency-light
  `atm-error` crate owns the shared stable error-code vocabulary

Sealing posture per boundary:
- `MailStore`: sealed by default
- `MailStoreDoctor`: sealed by default
- `RosterStore`: sealed by default
- `RosterStoreDoctor`: sealed by default
- `SourceIngress`: sealed by default
- `ProjectionExport`: sealed by default
- `ConfigIngress`: sealed by default
- `ConfigDoctor`: sealed by default
- `MessageReceivedHookEmitter` adapters: sealed by default unless an ADR explicitly
  opens the boundary
- `ObservabilityPort`: sealed

Privacy rule:
- concrete adapter types and their constructors remain private or
  tightly-scoped `pub(crate)` implementation details
- public callers depend on traits, façade structs, or request/result APIs
  rather than concrete I/O adapter types
- widening any boundary to public concrete adapter access requires explicit
  architecture review

`atm-core` does not own:
- daemon lifecycle
- socket listener loops
- live runtime status cache
- singleton enforcement

Those belong to the `atm-daemon` crate.

Phase R redesign notes:
- `atm-core` owns the shared `AtmProtocol` contract
- `atm-core` owns the public boundary contracts for transport, dispatch,
  store, config ingress, post-send emission, and notification/status surfaces
- `atm-core` owns the transport-neutral HTTP application schema used by both
  same-host local IPC and cross-host daemon transport; the retired ATM frame
  schema is historical only and must not be extended
- `atm-core` owns the immutable public runtime roster projection
  `ProjectionRoster`; that surface is derived from canonical ATM roster
  truth rather than from direct `config.json` reads
- `atm-core` owns the shared subsystem doctor DTO family:
  - `DoctorFinding`
  - `MailStoreDoctorReport`
  - `RosterStoreDoctorReport`
  - `ConfigDoctorReport`
  - `DaemonRuntimeDoctorReport`
- the daemon may aggregate those subsystem reports and compare them for drift,
  but it must not reimplement backend-specific diagnosis logic

Phase AC supersession note:
- `AC.2` moved the concrete Claude inbox storage backend into the now-retired
  `crates/atm-storage-claude`
- `ADR-019` later retired that concrete backend from the accepted line because
  Claude Code no longer uses it
- `atm-core` still owns generic source/projection boundary traits and helper
  request/response shapes during the cutover window, but it no longer owns the
  concrete Claude file-backed backend implementation
- `atm-core` team-admin surfaces must treat ATM roster rows as canonical team
  and member truth; retained Claude `config.json` remains projection/output
  state plus explicit `doctor` comparison input, not a second team-admin
  authority
- the `Z.6` Claude send warning path must build `ProjectionRoster` from
  store-backed ATM roster rows after the durable write succeeds; it must not
  reopen a direct `config.json` membership lookup seam
- `atm-core` owns the queue-query semantics shared by `atm list` and
  single-message `atm read`
- selector-driven queue inspection operates on logical terminal-node messages,
  not raw superseded predecessors
- the canonical ICD owns the exact frame constants, packet-kind assignments,
  and payload DTO mapping that `atm-core` must encode and decode
- `atm-core` owns the current daemon packet family for:
  - send compose
  - send acknowledge
  - receive
  - clear
  - doctor
  - heartbeat
- thin-client workflow surfaces should center on `send` and `receive`
- `ack` remains a workflow/state concern, but thin-client protocol shape
  should carry it inside send-shaped requests rather than a separate top-level
  method family

## 2.2 AI.23 Shared-Write Convergence Point

AI.23 establishes one convergence point for every authenticated HTTP write.
Local CLI HTTP, same-host advertised-IP HTTP, and remote peer HTTPS all decode
the same transport-neutral `WriteRequest` and enter `ApiRouter::route`. The
transport adapter owns authentication and ingress provenance only; it must not
create a second persistence, acknowledgement, or notification path.

`ApiRouter::route` delegates write handling to the daemon's
`DaemonRequestDispatcher::route_write`, which invokes the canonical
`MessageWriter::write` persistence operation before
`PostWriteRouter::dispatch` selects the local nudge or host-qualified peer
delivery. This ordering is the crate-level shared-write-resource invariant:
every ingress uses the same dispatcher, persistence boundary, and post-write
router. See [`../atm-daemon/http-api.md`](../atm-daemon/http-api.md),
[`../adr/ADR-033-http-endpoint-contract.md`](../adr/ADR-033-http-endpoint-contract.md),
and [`boundaries.md`](./boundaries.md) for the corresponding transport and
boundary contracts.
- queue inspection must not remain one "read many full messages" surface once
  SQLite-backed mailbox history becomes the ordinary durable source of truth

Config-ingress ownership rules:
- `ConfigIngress` must not remain a generic retained-command/runtime roster
  lookup surface
- normal retained runtime membership decisions belong to ATM roster truth and
  `ProjectionRoster`
- the only approved retained send-path file-state exception before `Z.8`
  watcher ownership is the post-write missing-config existing-inbox fallback
  warning; that exception does not restore generic file-backed membership
  checks
- before `ADR-019`, `ConfigIngress` was reserved for watcher-owned external
  ingest plus approved comparison/preservation callers such as `doctor` and
  recreated-shell restore preservation
- after `ADR-019`, no accepted runtime path keeps watcher/reconcile as a live
  `ConfigIngress` owner; any retained `ConfigIngress` use is explicit
  comparison/admin tooling only
- repository-local lint / `sc-lint`-candidate gates should make direct
  production `config.json` roster reads and generic `load_team_config(...)`
  helper use mechanically detectable
- later `Phase Z` boundary-cleanup gates should also make the following
  mechanically detectable:
  - `SCB-RETAINED-001`: direct command-entry
    `service_runtime_store::default_runtime()` misuse in `atm teams`,
    `atm members`, or `atm teams add-member`
  - `SCB-WORKSPACE-001`: direct command/team-admin ambient `.atm.toml` /
    `load_config(...)` reads outside the approved seam
  - `SCB-SINGLETON-001`: public ambient singleton/runtime-factory exposure
    such as broad crate-root re-exports
- any surviving pre-existing match for those rule families must live in an
  explicit TOML allowlist with owner and sunset-sprint metadata; new matches
  are lint failures rather than review-time warnings

Required HTTP direction:
- HTTP request framing must not depend on EOF or socket half-close semantics
- adapters validate HTTP method/resource/body limits before JSON decode and
  call the shared `ApiRouter`
- UDS, loopback TCP, and HTTPS implementations vary only in socket and ingress
  authentication; they share one HTTP resource contract and `WriteRequest`
- the canonical daemon wire contract is documented in
  [`../atm-daemon/http-api.md`](../atm-daemon/http-api.md)
- the same HTTP API governs same-host local IPC and cross-host daemon-to-daemon
  transport

## 2.2 Phase R Semantic Wrapper Policy

Phase R should keep durable identifiers and runtime-cap settings typed across
the service boundary.

Required wrappers:
- `MessageKey`
- `ConnectionCap`
- `QueueDepth`
- `RetryBudget`
- `BusyTimeout`
- `RequestDeadline`

Architectural rule:
- these values must not flow through the service/store boundary as raw
  `String`, `usize`, or integer timeout primitives in the current SQLite/daemon
  implementation line

Store-family rule:
- `MailStore` owns message lifecycle state
- `RosterStore` owns durable team/member roster state only
- task storage is currently out of scope; any future task storage line starts
  from canonical Claude-code schema rather than from preserved transition
  scaffolding
- daemon-owned live `pid` state and other session-transient runtime data stay
  outside `RosterStore`
- `TeamConfig` / `config.json` stays a config-ingress document, not the durable
  roster contract or the daemon runtime team-discovery surface
- `MailStore` must not become the catch-all owner for unrelated future domains
  such as orchestration or daemon-live-status state

## 3. Config Loading Boundary

Persisted config and team-document handling belongs at the `atm-core` loading
boundary rather than in scattered command call sites.

Required loading policy:
- classify persisted-data failures as compatibility-only schema drift,
  record-level invalid data, document-level invalid data, or missing-document
- apply defaults only for deterministic compatibility recovery
- keep identity and routing-critical fields required unless the product docs
  explicitly define a safe fallback
- preserve file, entity, and parser context when converting loader failures
  into `AtmError`

This keeps tolerant parsing centralized and prevents commands from inventing
ad hoc recovery behavior.

ATM-owned `.atm.toml` semantics for the retained multi-agent model:
- `atm-core` consumes the `[atm]` section only
- `[atm].default_team` remains the shared team default
- `[atm].team_members` is the baseline roster used for doctor and future
  orchestration-safety checks
- `[atm].aliases` is an ATM-owned shorthand map for canonical agent names
- `[[atm.post_send_hooks]]` is the ATM-owned best-effort post-send automation
  surface
- each rule binds one recipient selector and one command argv
- retired flat hook keys and `[atm].post_send_hook_members` are configuration
  errors with migration guidance, not compatibility aliases
- `[atm].identity` is obsolete and ignored by runtime identity resolution
- launcher-owned sections such as `[rmux]` and future `[scmux]` are outside the
  `atm-core` runtime boundary and are intentionally ignored
- `config.json` remains an ingress surface for roster updates, but it is not
  the durable source of truth for roster state in the current architecture

Send-specific policy remains layered above the loader:
- send may use a narrowly defined missing-document fallback when the product
  docs explicitly allow it
- malformed documents remain loader errors and do not automatically degrade into
  send fallback
- deduplicated repair notifications belong to the send orchestration boundary,
  not to generic config parsing

Identity-specific policy:
- caller-owned command identity must come from explicit override when supported
  or invoking-shell `ATM_IDENTITY`
- caller-owned command entry points must reject unresolved identity before any
  daemon dispatch
- downstream caller-owned request DTOs must carry resolved caller identity as a
  required field
- `atm-core` owns the service-layer mailbox split:
  - `peek` and `list` are inspection-only queries
  - `send`, `read`, `ack`, and `clear` are owner-only mutating operations
- mutating mailbox/message service operations must not expose caller
  impersonation
- `atm-core` must not treat hook files, repo-local config, or daemon ambient
  `ATM_IDENTITY` as fallback caller identity
- `atm-core` must not derive a normal sender/actor identity from repo-local
  config in the shared multi-agent checkout model
- aliases must resolve to canonical member names before membership validation,
  self-send checks, and mailbox lookup
- client-specific runtime logic is owned by the client crate, not by
  `atm-core` adapters or the daemon composition layer
- same-team messages keep current canonical sender projection behavior
- cross-team messages may project an alias-oriented `from` field only when the
  canonical sender identity is also persisted in SQLite-owned state for
  validation, routing, and audit use
- the shared send-context builder rejects canonical same-team self-addressed
  sends only when the destination has no host, before any message persistence
  or `dry-run` success outcome is built; host-qualified destinations proceed
  to ordinary host routing without DNS or local-interface inspection
- post-send-hook execution is outside the atomic mailbox mutation boundary
- the hook runs only after a successful non-`dry-run` send
- hook matching is recipient-scoped only
- `recipient = "*"` matches all recipients
- multiple matching rules execute in config order
- a relative hook path resolves from the discovered `.atm.toml` directory and
  executes with that same directory as working directory
- bare executable names use normal `PATH` lookup
- the hook inherits process environment and receives one ATM-owned JSON
  payload in `ATM_POST_SEND`
- the `ATM_POST_SEND` payload contains:
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
- hook stdout may optionally carry one structured result object that ATM parses
  on a best-effort basis for post-send diagnostics
- supported structured hook-result levels are `debug`, `info`, `warn`, and
  `error`
- hook configuration lookup must come from authoritative sender roster home
  `home_dir` metadata
- recipient non-match is silent
- hook-decision evaluation must preserve sender, recipient, matched rule
  selector, and execution outcome for troubleshooting
- hook failure or timeout is best-effort only and must not convert a
  successful send into a command failure
- the hook fires for successful outbound mailbox writes from `atm send` and
  `atm ack`; `is_ack = false` for send and `is_ack = true` for ack
- historical self-addressed pending-ack cleanup is handled in the shared ack
  path by suppressing reply emission while still completing the source
  acknowledgement state transition
- suppressed self-ack completion emits no ack hook because no outbound reply
  write exists
- after roster migration, the send path should populate
  `ATM_POST_SEND.recipient_pane_id` from the authoritative roster/store record
  so hook scripts do not need to rediscover pane mappings from file state
- any retained built-in helper does not consume `ATM_POST_SEND`; it consumes
  one resolved `ATM_INTERNAL_NUDGE` envelope carrying:
  - the canonical post-send event
  - the concrete sink target
  - the resolved template kind
  - the resolved template body or explicit disabled state
- committed `.atm.toml` pane ids are not live routing truth; any retained
  compatibility helper must consume authoritative roster/payload pane metadata
  or an explicit operator-provided pane id
- the reserved diagnostic sender `atm-identity-missing@<team>` is for
  ATM-generated repair/diagnostic notices only
- doctor should compare the live `config.json` roster against canonical ATM
  roster truth and surface drift in both directions
- doctor may still project the live `config.json` roster in a deterministic
  order: baseline `[atm].team_members` first, `team-lead` first among that
  baseline, then extra runtime members
- doctor should snapshot `~/.claude/teams/*/inboxes/*.lock` at start and end;
  any lock path present in both snapshots is stale and should surface as
  `ATM_WARNING_STALE_MAILBOX_LOCK` with `rm -f <path>` recovery guidance

Approved canonical roster-member schema direction:
- `team_name`
- `agent_name`
- `member_kind` — `permanent` or `ephemeral`
- `harness` — behavioral enum; approved values: `claude-code`, `codex-cli`,
  `gemini-cli`, `opencode`, `hermes`, `python-graft`
  - `hermes` identifies the Hermes Python gateway integration
  - `python-graft` identifies any other Python host using the `atm-graft`
    interface
- `agent_type`
- `model`
- `metadata_json`
- `recipient_pane_id` when known, only if the U.7 roster review keeps a
  runtime/routing field on canonical member rows

`pid` is not part of the canonical roster-member schema. It is transient
daemon-owned runtime state and must not be treated as a roster-member identity,
harness field, or durable SQLite roster field in the U.7 canonical member
model.

Observability boundary note:
- `AgentMember.extra` is intentionally out of scope for the L.4 observability
  field-model cleanup
- L.4 only replaces raw JSON types on observability-facing public types such as
  `AtmLogRecord.fields` and `LogFieldMatch`
- `AgentMember.extra` remains a round-trip preservation mechanism for
  Claude Code config fields rather than part of the retained-log API surface

Sealed-trait note:
- the sealed `ObservabilityPort` boundary prevents arbitrary external crates
  from implementing ATM's injected observability contract and bypassing the
  intended adapter split between `atm-core` and `atm`
- this decision should be revisited only if a concrete alternative materially
  simplifies first-party construction or testing without weakening those
  crate-boundary guarantees

## 3.1 Send Alert Metadata Boundary

ATM-authored alert metadata belongs to the send/schema boundary in `atm-core`.

Architectural rule:
- ATM-owned repair/alert machine state belongs in SQLite-owned state and typed
  diagnostics, not in shared inbox metadata namespaces
- legacy top-level alert fields such as `atmAlertKind` and
  `missingConfigPath` remain read-compatible only until removed

## 3.2 Retained Team Recovery Boundary

`atm-core` owns the retained local team recovery boundary needed for initial
release.

Architectural rules:
- the retained team surface is limited to:
  - team discovery
  - member listing
  - `add-member`
  - `update-member`
  - local team backup
  - local team restore
- historical orchestration-heavy team commands remain outside the retained
  `atm-core` boundary for initial release
- `add-member` remains create-only
- `add-member` persists the member's durable `home_dir` on the canonical ATM
  roster row and projects that same `home_dir` into compatibility
  `config.json.members`
- `update-member` is the accepted repair path for mutable existing roster
  metadata such as `home_dir`, `recipient_pane_id`, `harness`, `agent_type`,
  and `model`
- accepted terminology must distinguish:
  - `home_dir` = durable SQL-backed agent-home directory for the member; for
    worktree-backed members it preserves the worktree home and the canonical
    association back to the owning main repo
  - `live_cwd` = runtime-only working-directory overlay for the invoking ATM
    member when the active CLI/doctor process can bind `ATM_IDENTITY` to that
    displayed member; it is not durable roster metadata
  - `launch_cwd` = startup-only current-directory snapshot emitted to ATM CLI
    startup logs; it is not durable roster metadata
- `live_cwd` is runtime-only caller-member state, not operator-settable or
  durable roster metadata
- `launch_cwd` is log-only startup context and must not become durable roster
  metadata
- accepted implementations must prefer direct roster-row and runtime-roster
  fields over new directory-state coordinator structs
- backup excludes transient mailbox `*.lock` sentinels, dotfiles, and restore
  markers from the inbox copy set
- restore preserves the current team-lead record and current `leadSessionId`
  rather than replaying stale lead-session state from backup
- restored non-lead members must have runtime-only state cleared before they
  are written back to local config
- restore sweeps stale mailbox `*.lock` sentinels before restored inbox files
  are copied back into place
- restored ATM task buckets must recompute `.highwatermark` from the maximum
  restored task id
- the local `members` view is config-first; richer hook/session state may be
  layered later without changing the base recovery contract

## 3.3 Current Mail And Roster Ownership

`atm-core` must structure the mail system around these ownership rules:

- SQLite is the durable source of truth for:
  - messages
  - ack/task state
  - read/clear visibility state
  - team roster
- daemon memory is the live source of truth for agent status
- durable store state is the primary forward-write contract for ATM 1.2
- Claude inbox-append runtime behavior is retired from the accepted governing
  runtime and must not be the live forward-write contract
- if a retained Claude mailbox compatibility export helper survives
  temporarily, it is explicit obsolete-only scaffolding rather than the
  governing delivery contract
- write-affecting mail events persist first, then emit direct message-received
  behavior only when the recipient exposes that capability
- `atm-core` owns the direct message-received seam through
  `MessageReceivedHookEmitter`, not through `DeliveryPlan` or `NotificationSink`
- `atm-core` owns one canonical post-send event model carrying sender/team,
  message id, description, task id, ack flags, and authoritative
  `recipient_pane_id` when known
- any team-scoped built-in template override lookup must cross a
  storage-neutral `NudgeTemplateOverrideStore` contract upstream of
  `MessageReceivedHookEmitter`; the emitter itself receives resolved text or absence
  only and must not grow SQLite lookup behavior
- any retained built-in CLI helper receives the already-resolved template
  through `InternalNudgeEnvelope`; the live production path stays in-process,
  and the helper must not reopen runtime/store lookup
- that boundary returns an explicit row lifecycle, not hidden control strings:
  no row => product default, override row => stored text, disabled row => no
  emission, clear/reset => row deletion
- `atm-core` owns the shared resolved-template helper for built-in nudges, but
  it does not own built-in XML template bodies, template override storage,
  tmux injection, or graft host-wakeup mechanics
- the concrete receiver sinks behind that seam are:
  - `TmuxNudgeSink` for local tmux-backed recipients
  - `GraftReceiveHook` for graft-backed recipients
- `atm-core` owns the plan types and machine outputs; it must not allow outer
  send/ack/persistence modules to reintroduce harness policy after plan
  creation

Migration implication:
- current mailbox/workflow-sidecar logic is transitional and must converge onto
  the store boundary instead of remaining long-term source-of-truth logic

## 4. ADR Namespace

The `atm-core` crate uses the `ADR-ATM-CORE-*` namespace.

Initial use cases:

- typestate and workflow decisions
- mailbox boundary decisions
- config/loading decisions
- observability port decisions
- service/module boundary decisions

## 5. `sc-observability` Integration Boundary

The retained `atm-core` observability surface is a full
emit/query/follow/health boundary.

Architectural rules:

- `atm-core` owns the ATM-facing request/result models needed for ATM messaging
  workflows, log query/tail, and doctor health
- `atm-core` must not expose shared `sc-observability` types in its public API
- follow/tail behavior must remain synchronous and ATM-owned at the
  `atm-core` boundary even though it is backed by shared follow support
- the concrete adapter implementation remains owned by `atm`
- this boundary is intentionally ATM-local for the initial release; it does not
  attempt to pre-own future hook- or `schooks`-orchestrated observability
  concerns
- the initial-release health contract remains intentionally closed at:
  - `Healthy`
  - `Degraded`
  - `Unavailable`
- public observability models must use ATM-owned value/container types rather
  than exposing raw `serde_json::Value` / `Map<String, Value>` directly

Required ATM-owned projected surfaces:

- `AtmLogQuery`
- `AtmLogRecord`
- `AtmLogSnapshot`
- `AtmObservabilityHealth`
- `LogTailSession`

The exact design is owned by:
- [`design/sc-observability-integration.md`](./design/sc-observability-integration.md)

## 6. Error-Code Registry Boundary

`atm-error` owns the dependency-light source registry of ATM-owned error codes;
`atm-storage` and `atm-core` re-export the same type for their respective
consumers.

Architectural rules:

- the source registry must live in `crates/atm-error/src/error_codes.rs`
- `AtmError` must carry an `AtmErrorCode`
- coarse `AtmErrorKind` classification must not replace the stable code
- warning diagnostics emitted by `atm-core` must also select a registry code
- the source registry must stay aligned with
  [`../atm-error-codes.md`](../atm-error-codes.md)
