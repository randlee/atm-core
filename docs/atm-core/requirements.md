# ATM-Core Crate Requirements

## 1. Purpose

This document defines the `atm-core` crate requirements.

The `atm-core` crate owns the reusable daemon-free ATM business logic. Product
behavior remains defined in [`../requirements.md`](../requirements.md).

## 2. Ownership

`atm-core` owns:

- path and config resolution policy
- address parsing and validation
- mailbox I/O
- workflow and typestate rules
- send/read/ack/clear service behavior
- log query/follow service behavior over the observability boundary
- doctor service behavior
- structured core errors

`atm-core` does not own:

- clap parsing
- terminal formatting
- process exit policy
- direct dependency on concrete observability crates

## 3. Requirement Namespace

The `atm-core` crate uses the `REQ-CORE-*` namespace.

Initial allocation:

- `REQ-CORE-CONFIG-*`
- `REQ-CORE-MAILBOX-*`
- `REQ-CORE-WORKFLOW-*`
- `REQ-CORE-SEND-*`
- `REQ-CORE-READ-*`
- `REQ-CORE-ACK-*`
- `REQ-CORE-CLEAR-*`
- `REQ-CORE-LOG-*`
- `REQ-CORE-DOCTOR-*`
- `REQ-CORE-OBS-*`
- `REQ-CORE-TEAM-*`

Initial crate requirement IDs:

- `REQ-CORE-CONFIG-001` `atm-core` owns daemon-free home/path/config/identity
  resolution policy. Satisfies the path/config/identity aspects of:
  `REQ-P-CONTRACT-001`, `REQ-P-IDENTITY-001`, `REQ-P-DOCTOR-001`.
- `REQ-CORE-CONFIG-002` `atm-core` owns daemon-free address parsing,
  alias rewrite, and team/member validation policy. Satisfies the address
  resolution and target-validation aspects of:
  `REQ-P-ADDRESS-001`, `REQ-P-SEND-001`, `REQ-P-READ-001`,
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
- `REQ-CORE-MAILBOX-001` `atm-core` owns daemon-free mailbox/store behavior.
  Satisfies the persisted mailbox I/O and mutation aspects of:
  `REQ-P-CONTRACT-001`, `REQ-P-SEND-001`, `REQ-P-READ-001`,
  `REQ-P-ACK-001`, `REQ-P-CLEAR-001`, `REQ-P-RELIABILITY-001`,
  `REQ-P-IDLE-001`.
- `REQ-CORE-WORKFLOW-001` `atm-core` owns the two-axis workflow model and legal
  transitions. Satisfies the state-classification and legal-transition aspects
  of:
  `REQ-P-READ-001`, `REQ-P-ACK-001`, `REQ-P-CLEAR-001`,
  `REQ-P-WORKFLOW-001`.
- `REQ-CORE-SEND-003` `atm-core` owns send-path message construction,
  classification, and append-boundary behavior above the mailbox storage
  helpers. Satisfies the send-path service aspects of:
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

## 4. Module Ownership

Per-module documentation lives under:

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
- [`../atm-error-codes.md`](../atm-error-codes.md)
- [`./design/dedup-metadata-schema.md`](./design/dedup-metadata-schema.md)
- [`./design/sc-observability-integration.md`](./design/sc-observability-integration.md)
- [`./design/sc-obs-1.0-integration.md`](./design/sc-obs-1.0-integration.md)

## 6. Send Alert Metadata

Requirement ID:
- `REQ-CORE-SEND-002`

Required write-path rules:
- ATM-authored alert field writes must use ATM-owned `metadata.atm` fields
- forward alert writes must target `metadata.atm.alertKind` and
  `metadata.atm.missingConfigPath` or a later explicitly documented
  `metadata.atm` field
- new ATM-only alert top-level fields must be rejected with a descriptive
  validation error on the write path
- exception: until the alert metadata migration sprint lands, the current
  runtime send path may continue writing legacy top-level `atmAlertKind` and
  `missingConfigPath` fields; this carve-out is bounded by
  [`architecture.md` §3.1](./architecture.md)
- the write-path rejection requirement applies to new ATM-only alert fields
  introduced after Phase J

Required read-path rules:
- ATM read must accept legacy top-level alert fields such as `atmAlertKind` and
  `missingConfigPath`
- ATM read must also accept forward `metadata.atm` alert fields
- malformed ATM-owned alert metadata must degrade gracefully, emit warning
  diagnostics, and never cause the message to be dropped when the
  Claude-native envelope remains usable

Forward migration rule:
- legacy top-level `atmAlertKind` migrates to `metadata.atm.alertKind`
- legacy top-level `missingConfigPath` migrates to
  `metadata.atm.missingConfigPath`
- the forward architectural target and compatibility-period carve-out are
  documented in [`architecture.md` §3.1](./architecture.md)

## 7. Observability Integration Boundary

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

## 8. Config And Team Baseline Semantics

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
  in `metadata.atm.fromIdentity`
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
  - optional `task_id` when present
- the hook may optionally emit one structured stdout result with `level`,
  `message`, and optional `fields`; ATM logs it on a best-effort basis and
  ignores absent or invalid output
- hook-rule evaluation and execution outcomes must remain observable through
  structured diagnostics without creating caller-visible warnings for expected
  recipient non-match
- expected recipient non-match remains debug-only diagnostics and must not emit
  a caller-visible warning
- hook failure or timeout is best-effort only and must not roll back a
  successful send
- actual hook execution failures remain the only case where caller-visible hook
  warnings are appropriate
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

## 9. Retained Team Recovery Surface

Requirement ID:
- `REQ-CORE-TEAM-001`

Required service rules:
- `atm-core` owns the retained local team recovery surface for:
  - discovered-team listing
  - local member listing
  - `add-member`
  - team backup
  - team restore
- these services remain local file/config/inbox operations and must not depend
  on daemon orchestration or runtime spawning
- `add-member` must validate team existence and reject duplicate member names
  before mutating local team config
- when `add-member` receives a pane id, it must persist `tmuxPaneId` in
  canonical tmux `%<number>` form, set `backendType = "tmux"`, and mark the
  member `isActive = true`
- `add-member` must reject unsupported tmux target syntax such as
  `session:window.pane` rather than guessing a pane handle
- backup must snapshot:
  - `config.json`
  - team inbox files, excluding transient `*.lock` sentinels, dotfiles, and
    restore markers
  - the ATM team task bucket
- restore must:
  - preserve the current team-lead entry and current `leadSessionId`
  - add only missing non-lead members from the snapshot
  - clear runtime-only restored-member fields such as session or pane state
  - restore non-lead inboxes
  - sweep stale inbox `*.lock` sentinels before copying restored inbox files
  - recompute `.highwatermark` from the maximum restored task id
  - support a dry-run path without making changes
- malformed or missing snapshot material must fail with structured errors
  before partial restore is committed
- `members` must remain useful as a local roster inspection command even when
  daemon or hook state is unavailable
