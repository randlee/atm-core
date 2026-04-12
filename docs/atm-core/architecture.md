# ATM-Core Crate Architecture

## 1. Purpose

This document defines the `atm-core` crate architectural boundary.

It complements the product architecture in
[`../architecture.md`](../architecture.md) and owns crate-local structure and
service boundaries.

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
- `[atm].post_send_hook` is an ATM-owned helper command definition for
  best-effort post-send automation
- `[atm].post_send_hook_senders` and `[atm].post_send_hook_recipients` are the
  sender/recipient trigger lists for that helper
- retired `[atm].post_send_hook_members` is a configuration error with
  migration guidance, not a compatibility alias
- `[atm].identity` is obsolete and ignored by runtime identity resolution
- launcher-owned sections such as `[rmux]` and future `[scmux]` are outside the
  `atm-core` runtime boundary and are intentionally ignored

Send-specific policy remains layered above the loader:
- send may use a narrowly defined missing-document fallback when the product
  docs explicitly allow it
- malformed documents remain loader errors and do not automatically degrade into
  send fallback
- deduplicated repair notifications belong to the send orchestration boundary,
  not to generic config parsing

Identity-specific policy:
- runtime identity must come from explicit override, hook identity, or
  `ATM_IDENTITY`
- `atm-core` must not derive a normal sender/actor identity from repo-local
  config in the shared multi-agent checkout model
- aliases must resolve to canonical member names before membership validation,
  self-send checks, and mailbox lookup
- same-team messages keep current canonical sender projection behavior
- cross-team messages may project an alias-oriented `from` field only when
  canonical sender identity is also persisted in `metadata.atm.fromIdentity` for
  validation, routing, and audit use
- post-send-hook execution is outside the atomic mailbox mutation boundary
- the hook runs only after a successful non-`dry-run` send
- a relative hook path resolves from the discovered `.atm.toml` directory and
  executes with that same directory as working directory
- the hook inherits process environment and receives one ATM-owned JSON
  payload in `ATM_POST_SEND`
- the `ATM_POST_SEND` payload contains:
  - `from`
  - `to`
  - `message_id`
  - `requires_ack`
  - optional `task_id`
  - `hook_match.sender`
    boolean — true if the sender filter axis matched, false otherwise
  - `hook_match.recipient`
    boolean — true if the recipient filter axis matched, false otherwise
- sender matching uses `[atm].post_send_hook_senders`
- recipient matching uses `[atm].post_send_hook_recipients`
- `*` is a wildcard match on either trigger axis
- hook execution occurs once when either trigger axis matches
- hook stdout may optionally carry one structured result object that ATM parses
  on a best-effort basis for post-send diagnostics
- supported structured hook-result levels are `debug`, `info`, `warn`, and
  `error`
- hook-decision evaluation and skip reasons must be observable enough for
  troubleshooting without requiring source inspection
- hook failure or timeout is best-effort only and must not convert a
  successful send into a command failure
- the reserved diagnostic sender `atm-identity-missing@<team>` is for
  ATM-generated repair/diagnostic notices only
- doctor should project the live `config.json` roster in a deterministic order:
  baseline `[atm].team_members` first, `team-lead` first among that baseline,
  then extra runtime members

Current `AgentMember` persisted schema:
- `name: String` required for roster membership checks
- `agent_id: String` stored as `agentId`, default empty string
- `agent_type: String` stored as `agentType`, default empty string
- `model: String`, default empty string
- `joined_at: Option<u64>` stored as `joinedAt`
- `tmux_pane_id: String` stored as `tmuxPaneId`, default empty string
- `cwd: String`, default empty string
- `extra: serde_json::Map<String, serde_json::Value>` via `#[serde(flatten)]`
  for forward-compatible Claude Code fields

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
- forward ATM-authored alert metadata lives under `metadata.atm`
- legacy top-level alert fields such as `atmAlertKind` and
  `missingConfigPath` remain read-compatible only
- the current runtime send path may continue emitting the legacy top-level
  fields until the migration implementation sprint lands
- this compatibility-period carve-out is the bounded exception referenced by
  [`requirements.md` `REQ-CORE-SEND-002`](./requirements.md#6-send-alert-metadata)

## 3.2 Retained Team Recovery Boundary

`atm-core` owns the retained local team recovery boundary needed for initial
release.

Architectural rules:
- the retained team surface is limited to:
  - team discovery
  - member listing
  - `add-member`
  - local team backup
  - local team restore
- historical orchestration-heavy team commands remain outside the retained
  `atm-core` boundary for initial release
- restore preserves the current team-lead record and current `leadSessionId`
  rather than replaying stale lead-session state from backup
- restored non-lead members must have runtime-only state cleared before they
  are written back to local config
- restored ATM task buckets must recompute `.highwatermark` from the maximum
  restored task id
- the local `members` view is config-first; richer hook/session state may be
  layered later without changing the base recovery contract

## 4. ADR Namespace

The `atm-core` crate uses the `ADR-CORE-*` namespace.

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

`atm-core` owns the single source registry of ATM-owned error codes in source.

Architectural rules:

- the source registry must live in `crates/atm-core/src/error_codes.rs`
- `AtmError` must carry an `AtmErrorCode`
- coarse `AtmErrorKind` classification must not replace the stable code
- warning diagnostics emitted by `atm-core` must also select a registry code
- the source registry must stay aligned with
  [`../atm-error-codes.md`](../atm-error-codes.md)
