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
- `atm-core` must keep mailbox/config/workflow/log/doctor logic reusable across
  CLI contexts.
- `atm-core` owns persisted config/team loading policy, including compatibility
  defaults, recovery boundaries, and precise parse diagnostics.

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

Send-specific policy remains layered above the loader:
- send may use a narrowly defined missing-document fallback when the product
  docs explicitly allow it
- malformed documents remain loader errors and do not automatically degrade into
  send fallback
- deduplicated repair notifications belong to the send orchestration boundary,
  not to generic config parsing

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
