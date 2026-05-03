# ATM CLI Requirements

## 1. Product Definition

Product requirement ID:
- `REQ-P-PRODUCT-001` The retained ATM product surface consists of
  `send`, `read`, `ack`, `clear`, `log`, `doctor`, `teams`, and `members`,
  backed by a singleton daemon runtime and SQLite source-of-truth for mail and
  roster state in the Phase Q architecture.

Satisfied by:
- intentionally undecomposed product requirement; this governs overall retained
  product scope rather than a single crate-local obligation

The product is a local command-line tool named `atm`.

The current target architecture no longer treats daemon removal as a product
goal. Phase Q reintroduces a tightly-bounded singleton daemon runtime because
mail routing, native agent notification, and cross-host transport need one
coordinating process, while ATM command behavior remains the user-facing
surface.

The retained product surface is:
- `atm send`
- `atm read`
- `atm ack`
- `atm clear`
- `atm log`
- `atm doctor`
- `atm teams`
- `atm members`

The system must preserve the retained command behavior unless these
requirements explicitly retire or change it.

The system uses structured logging through `sc-observability`.

Schema ownership references:

- Claude Code-native message schema:
  [`claude-code-message-schema.md`](./claude-code-message-schema.md)
- ATM additive/interpreted message schema:
  [`atm-message-schema.md`](./atm-message-schema.md)
- legacy ATM read-compatibility schema:
  [`legacy-atm-message-schema.md`](./legacy-atm-message-schema.md)
- `sc-observability` schema ownership pointer:
  [`sc-observability-schema.md`](./sc-observability-schema.md)
- ATM-owned error-code registry:
  [`atm-error-codes.md`](./atm-error-codes.md)
- schema enforcement models:
  `tools/schema_models/claude_code_message_schema.py` and
  `tools/schema_models/atm_message_schema.py` and
  `tools/schema_models/legacy_atm_message_schema.py`

## 1.1 Documentation Structure

Documentation organization is defined in
[`documentation-guidelines.md`](./documentation-guidelines.md).

Top-level product docs in `docs/` remain the product source of truth.
Crate-local ownership docs live under:

- [`docs/atm/requirements.md`](./atm/requirements.md)
- [`docs/atm/architecture.md`](./atm/architecture.md)
- [`docs/atm-core/requirements.md`](./atm-core/requirements.md)
- [`docs/atm-core/architecture.md`](./atm-core/architecture.md)
- [`docs/atm-daemon/requirements.md`](./atm-daemon/requirements.md)
- [`docs/atm-daemon/architecture.md`](./atm-daemon/architecture.md)
- [`docs/atm-rusqlite/requirements.md`](./atm-rusqlite/requirements.md)
- [`docs/atm-rusqlite/architecture.md`](./atm-rusqlite/architecture.md)

During the cleanup/restructure phase, product requirements stay here while
crate-local ownership is moved out of this file into the crate directories.

Phase-Q supersession note:
- earlier daemon-free assumptions in this file are historical requirements from
  the prior rewrite line
- for mail/runtime architecture, the current authoritative direction is Section
  21

## 2. Scope

Product requirement ID:
- `REQ-P-SCOPE-001` The rewrite retains the documented command surface and
  migrates ATM mail/runtime ownership from filesystem JSON plus mailbox locks
  to SQLite plus a singleton daemon without intentionally removing retained
  functionality.

Satisfied by:
- intentionally undecomposed product requirement; this governs overall rewrite
  scope and is enforced across the workspace rather than by one crate-local ID

- `REQ-P-RUNTIME-001` Production ATM commands must connect to the active daemon
  and auto-start it when absent.

  Required behavior:
  - the production CLI/runtime path first attempts to connect to an
    already-running daemon
  - if the daemon is unavailable, CLI must attempt one bounded background
    auto-start for the active team daemon and then retry the connection once
  - if auto-start still fails, ATM must fail clearly with recovery guidance
  - no production path may silently bypass the daemon by talking directly to
    SQLite or inbox files

### 2.1 In Scope

- one binary: `atm`
- one primary library: `atm-core`
- SQLite-backed ATM mail source of truth
- SQLite-backed team roster source of truth
- singleton daemon runtime
- same-host daemon API over Unix domain socket
- cross-host daemon API over TCP/TLS
- Claude-compatible JSONL inbox ingress and export
- configuration resolution
- hook-based identity fallback
- file-reference policy handling for `send --file`
- origin-inbox merge / ingest compatibility for Claude-owned inbox files
- ATM-owned read/ack/clear/task state in SQLite
- structured logging through `sc-observability`
- log query and follow through `sc-observability`
- local diagnostics through `atm doctor`
- local team discovery and recovery through `atm teams`
- local roster verification through `atm members`
- native agent/plugin notification interface
- the retained local team recovery surface:
  - `atm teams`
  - `atm members`
  - `atm teams add-member`
  - `atm teams backup`
  - `atm teams restore`
- task metadata carried in the mail envelope
- JSON output mode
- human-readable output mode

### 2.2 Out Of Scope

- long-lived durable remote-delivery queueing
- broad plugin host framework beyond the ATM agent notification/runtime
  interface
- CI monitoring
- TUI and MCP features
- daemon auto-spawn from tests
- `atm status` in the initial rewrite
- separate `atm tail` command in the initial rewrite
- team lifecycle management outside the retained local recovery surface
  (`atm teams`, `atm members`, `atm teams add-member`, `atm teams backup`,
  `atm teams restore`)

### 2.3 Release Distribution Scope

Product requirement ID:
- `REQ-P-RELEASE-001` The `1.0` retained-surface release must replace the
  previously published `agent-team-mail` CLI/core distribution channels from
  this repo without requiring downstream users to adopt new crate identities.

- `REQ-P-RELEASE-002` Channel parity for the replacement release is limited to
  the historical release channels that actually existed for the old repo:
  crates.io, GitHub Releases, and Homebrew.

- `REQ-P-RELEASE-003` Crate/package identity continuity must be preserved by
  publishing the retained CLI/core replacement under the legacy package names
  `agent-team-mail` and `agent-team-mail-core` while keeping the installed CLI
  binary name `atm`.

- `REQ-P-RELEASE-004` This repo must own the release-process control surface
  needed to ship and verify the replacement release, including the release
  workflows, artifact manifest, supporting scripts, and `publisher` agent
  instructions.

- `REQ-P-RELEASE-005` Windows installation must be first-class for `1.0`
  without requiring Rust tooling or manual archive extraction; `winget` is
  therefore a required additional release channel even though it was not part
  of the historical `agent-team-mail` release system.

- `REQ-P-RELEASE-006` Release prerequisites that depend on account-level
  distribution infrastructure must be made explicit in the repo-owned release
  plan before `1.0` release automation is considered complete.

Required behavior:
- the `1.0` release must publish the retained CLI and core crates under the
  legacy crates.io package names:
  - `agent-team-mail`
  - `agent-team-mail-core`
- the `atm` binary name remains the installed CLI entrypoint
- the release channels that were already part of the historical
  `agent-team-mail` release system and must be replaced from this repo are:
  - crates.io
  - GitHub Releases
  - Homebrew
- `winget` is not a historical release channel for `agent-team-mail`, but it
  is a required new `1.0` release channel so normal Windows users can install
  ATM without Rust tooling or manual zip handling
- Homebrew release automation depends on the existing `randlee/homebrew-tap`
  tap and requires `HOMEBREW_TAP_TOKEN` to be configured in `atm-core` GitHub
  secrets before the release workflow can update formulas from this repo
- `winget` release automation uses the `randlee` namespace with package ID
  `randlee.agent-team-mail`
- the first `winget` release requires a one-time manual manifest submission to
  `microsoft/winget-pkgs`; after that initial submission, later releases may
  be automated from this repo
- `winget` release automation must not require a repo-specific secret beyond
  the default GitHub workflow token
- release readiness proof for `winget` must validate successful submission or
  manifest update dispatch; it cannot require same-day installability because
  Microsoft review introduces a normal 1-2 day publication lag

## 3. External Contracts

Product requirement ID:
- `REQ-P-CONTRACT-001` External path/config/store/observability contracts must
  match the documented retained ATM behavior for the active architecture line.

Satisfied by:
- `REQ-CORE-CONFIG-001` for home/path/config resolution aspects
- `REQ-CORE-RUNTIME-001` for durable mail/roster store ownership aspects
- `REQ-CORE-COMPAT-001` for Claude inbox compatibility-surface aspects
- `REQ-ATM-OBS-001` for CLI observability bootstrap/integration aspects
- `REQ-CORE-OBS-001` for ATM observability boundary/query-model aspects

### 3.1 Home And Path Resolution

Path resolution order:
1. `ATM_HOME` when set and non-empty
2. OS home directory

Required canonical paths:
- `{ATM_HOME}/.claude`
- `{ATM_HOME}/.claude/teams`
- `{ATM_HOME}/.claude/teams/{team}`
- `{ATM_HOME}/.claude/teams/{team}/config.json`
- `{ATM_HOME}/.claude/teams/{team}/inboxes/{agent}.json`
- `{ATM_HOME}/.config/atm/config.toml`
- `{ATM_HOME}/.config/atm/state.json`
- `{ATM_HOME}/.config/atm/share/{team}/`

### 3.1.1 Security And Durability Boundaries

Product requirement IDs:
- `REQ-SEC-001` All user-supplied team and agent name segments must be
  validated before path construction.
- `REQ-SEC-002` JSON number normalization must not allocate unbounded memory.
- `REQ-DURABILITY-001` Atomic-write temp file names must be globally unique
  within a process.

Required behavior:
- valid team/agent path-segment characters are limited to:
  - alphanumeric
  - hyphen
  - underscore
  - period
- team/agent segments must reject:
  - empty strings
  - path separators
  - `..` sequences
  - consecutive periods
  - leading periods
  - platform-specific path escapes that could break out of the intended ATM
    home subtree
- validation must happen before any path construction in address parsing or
  home/path helpers
- JSON number normalization must cap exponent-driven string expansion at 64
  characters
- if exponent expansion would exceed 64 characters, ATM must:
  - return the original raw numeric string unchanged
  - emit a structured warning using
    `AtmErrorCode::WarningMalformedAtmFieldIgnored`
- atomic persistence helpers must use temp-file names that are unique for each
  write attempt targeting the same destination path from the same process
- timestamp-only temp-file suffixes are not sufficient for the durability
  contract because rapid same-process writes can collide

### 3.2 Team Mail Store

Per-team layout:
- `{ATM_HOME}/.claude/teams/{team}/config.json`
- `{ATM_HOME}/.claude/teams/{team}/inboxes/{agent}.json`
- optional origin inbox files:
  - `{ATM_HOME}/.claude/teams/{team}/inboxes/{agent}.{origin}.json`

The rewrite retains origin-file merge behavior for read and wait paths because it is part of the current file-based mail surface and does not require the daemon.

### 3.2.1 Message Schema Ownership And Compatibility

Product requirement ID:
- `REQ-P-SCHEMA-001` ATM must preserve explicit ownership boundaries between
  Claude Code-native message schema, legacy ATM compatibility schema, and
  forward ATM metadata schema.

Satisfied by:
- `REQ-CORE-MAILBOX-001` for persisted inbox read/write compatibility
- `REQ-CORE-WORKFLOW-001` for ATM workflow semantics layered onto compatible
  message representations

Required rules:

- Claude Code-native message schema is owned by Claude Code
- ATM must not redefine Claude-native fields as if ATM owned them
- ATM read must accept:
  - Claude Code-native messages
  - legacy ATM top-level additive messages
  - future ATM metadata-based messages
- new ATM-only machine-readable fields must not be added as new top-level inbox
  fields
- forward ATM machine-readable fields must live in `metadata.atm`
- forward ATM-authored alert and repair metadata, including legacy
  `atmAlertKind` and `missingConfigPath`, must migrate to `metadata.atm`
  fields such as `metadata.atm.alertKind` and
  `metadata.atm.missingConfigPath`
- ATM may enrich a Claude-native message in place by adding ATM-owned metadata
  without rewriting native Claude fields except for the explicitly documented
  cross-team alias projection carve-out on `from`, which also requires
  `metadata.atm.fromIdentity`
- locally owned schema enforcement must distinguish legacy top-level UUID-based
  ATM identifiers from forward metadata-based ULID identifiers
- write-path validation may reject wrong-format ATM-owned identifiers with
  descriptive errors
- read-path validation failure for ATM-owned fields must trigger warning +
  degradation logic rather than failing the overall message read
- a separate ATM-native inbox is explicitly deferred and must not be assumed by
  the current live design

Current compatibility rule:

- existing runtime write/read behavior for legacy top-level alert fields
  remains stable until a later compatibility-migration implementation changes
  that persisted shape
`REQ-P-SCHEMA-001` is owned by:

- [`claude-code-message-schema.md`](./claude-code-message-schema.md)
- [`atm-message-schema.md`](./atm-message-schema.md)
- [`legacy-atm-message-schema.md`](./legacy-atm-message-schema.md)
- [`atm-core/design/dedup-metadata-schema.md`](./atm-core/design/dedup-metadata-schema.md)
  §2.2 and §3.3 for forward ATM alert-field placement and sender-side dedup
  semantics

### 3.2.2 Shared File Ownership And Mutation Classes

Product requirement ID:
- `REQ-P-FILEIO-001` Every live file operation must declare file ownership,
  mutation class, and the single commit path used for persistence.

Required rules:

- every live file path must be classified as one of:
  - Claude-owned
  - ATM-owned
  - shared/de-facto interoperable
- ownership determines whether ATM is allowed to treat the file as writable
  source-of-truth state
- ATM-owned machine state must have one documented write path per file family
- ad hoc write logic at leaf call sites is prohibited for live shared state

Operation classes:

- `read_only`
  - no lock acquisition
  - no temp-file write
  - no persistence side effect
- `read_possible_write`
  - initial unlocked read is allowed
  - if the read determines no change is needed, return without locking
  - if the read determines a change is needed, the operation must enter the
    shared write-commit path before persisting anything
- `read_modify_write`
  - mutation is expected
  - persistence must still flow through the shared write-commit path

Shared write-commit path requirements:

- the mutation plan must be computed from a concrete input snapshot
- before replacing the live file, ATM must prove source freshness by either:
  - compare-and-swap against the exact snapshot identity/content that was read,
    or
  - lock, reread current state, recompute the mutation from the fresh state,
    then commit
- `read -> mutate -> lock -> blind rename` is not a valid write path
- every successful commit of shared mutable structured state must use the
  documented atomic replacement helper family

Source-of-truth guardrails:

- ATM must not rely on full-file rewrite of Claude-owned files as the long-term
  source of truth for ATM-local workflow state
- if ATM-local semantics need durability independent of Claude’s native writes,
  that state must move to ATM-owned sidecars or an equivalent ATM-owned store
- when a legacy compatibility path still rewrites a non-ATM-owned shared file,
  the requirements and architecture docs must call out the limitation

### 3.3 Configuration Resolution

Configuration resolution order:
1. CLI flags
2. environment variables
3. repo-local `.atm.toml`
4. global `{ATM_HOME}/.config/atm/config.toml`
5. defaults

Required config fields:
- default team

Supported optional config fields:
- `[atm].team_members`
- `[atm].aliases`
- `[[atm.post_send_hooks]]`

Runtime identity rules:
- repo-local `.atm.toml` `[atm].identity` is not a valid runtime identity
  fallback for the retained multi-agent ATM model
- runtime identity must come from:
  - explicit command override when supported
  - hook-file identity
  - `ATM_IDENTITY`
- an obsolete config `[atm].identity` field may remain temporarily for
  migration, but ATM must ignore it for runtime identity resolution and
  `atm doctor` must flag it for removal
- `.atm.toml` may define `[atm].team_members` as the baseline team roster that
  should always be present in `config.json`
- `.atm.toml` may define `[atm].aliases` for ATM-owned shorthand addressing of
  canonical member identities
- `.atm.toml` may define one or more `[[atm.post_send_hooks]]` rules for
  best-effort recipient-scoped post-send automation
- retired `[atm].post_send_hook`, `[atm].post_send_hook_senders`,
  `[atm].post_send_hook_recipients`, and `[atm].post_send_hook_members` keys
  must be rejected with migration guidance directing operators to
  `[[atm.post_send_hooks]]`
- config sections outside ATM-owned config, such as `[rmux]` or future
  `[scmux]`, are not ATM runtime config and must be ignored by `atm-core`

### 3.3.1 Config And Schema Recovery

Product requirement ID:
- `REQ-P-CONFIG-HEALTH-001` Persisted ATM config and team JSON loading must
  recover at the narrowest safe scope and report precise diagnostics when
  recovery is not safe.

Satisfied by:
- `REQ-CORE-CONFIG-003` for config/team schema recovery and diagnostic policy
- `REQ-CORE-SEND-001` for send-time missing-config fallback and repair
  notification policy
- `REQ-CORE-MAILBOX-001` for mailbox record skip behavior

Required persisted-data classes:
- `compatibility-recoverable`
- `record-invalid`
- `document-invalid`
- `missing-document`

Required handling policy:
- compatibility-only schema drift may be recovered with documented,
  deterministic defaults
- malformed records inside a larger persisted collection should be skipped or
  quarantined individually when the rest of the document remains trustworthy
- malformed root documents or invalid root structure must fail with structured
  errors rather than guessed repairs
- missing persisted team config is a distinct `missing-document` condition and
  must not be collapsed into generic parse corruption
- identity and routing semantics must never be fabricated to keep a command
  running

Required diagnostics:
- failure class when known
- file path
- entity scope when known, such as member name or collection entry
- field name when known
- parser detail, including line and column when available
- recovery guidance when operator action is required

Operator examples and safe repair guidance live in
[`persisted-data-repair.md`](./persisted-data-repair.md).

### 3.4 Claude Settings Resolution

The system must resolve Claude settings for file-reference policy checks.

Resolution order:
1. explicit settings path override when provided internally
2. repo-local `.claude/settings.local.json`
3. repo-local `.claude/settings.json`
4. global `{ATM_HOME}/.claude/settings.json`

### 3.5 Observability Shared Integration Baseline

ATM depends on `sc-observability` as the shared logging/query/health substrate.

The shared surface ATM integrates against must support:
- structured log emission
- historical query of retained records
- follow/tail of new matching records
- filtering by severity
- filtering by structured key/value fields
- filtering by time window
- limit/order controls
- health reporting for the logging runtime

The current shared repo now exposes those generic capabilities. ATM must
integrate with them directly rather than preserving a local tracing-only
adapter.

Required integration rules:

- ATM must not implement a parallel ad hoc log-query engine when shared
  `sc-observability` APIs can own the behavior
- `atm-core` must keep the shared crates behind an ATM-owned injected boundary
- `atm` owns the concrete shared-crate bootstrap and dependency wiring
- the active release baseline uses the published
  `sc-observability = "1.0.0"` crates.io dependency
- the same pinned Rust toolchain must be used locally and in CI across ATM and
  `sc-*` repos
- the concrete integration work is planned in Phase K of
  [`project-plan.md`](./project-plan.md)

Historical note:
- `OBS-GAP-1` is complete as a historical planning artifact and does not remain
  the gating item for retained observability delivery

## 4. Identity Resolution

Product requirement ID:
- `REQ-P-IDENTITY-001` Identity resolution must follow the documented command
  precedence rules.

Satisfied by:
- `REQ-CORE-CONFIG-001` for identity resolution policy

### 4.1 Send Identity Resolution Order

1. `--from`
2. hook-file identity
3. `ATM_IDENTITY`

### 4.2 Read Identity Resolution Order

1. `--as`
2. hook-file identity
3. `ATM_IDENTITY`

### 4.3 Doctor Identity Resolution

`atm doctor` uses the same config and hook-resolution paths as the retained mail commands, but it must not fail immediately only because hook identity is absent. Missing hook identity is a diagnostic finding unless identity resolution is explicitly required for a requested check.

If command identity cannot be determined where required, the command must fail with a structured recovery-oriented error. An obsolete config `identity` field may be reported as a diagnostic, but it does not count as command identity.

## 5. Address Resolution

Product requirement ID:
- `REQ-P-ADDRESS-001` Address resolution must support the documented
  `agent`/`agent@team` forms and precedence rules.

Satisfied by:
- `REQ-CORE-CONFIG-002` for address parsing, alias rewrite, and
  team/member validation policy

Supported address forms:
- `agent`
- `agent@team`

Resolution order:
1. explicit `agent@team`
2. bare `agent` plus `--team`
3. bare `agent` plus configured default team

An explicit `@team` suffix takes precedence over `--team`.

Aliases are resolved after splitting `agent@team`, so only the agent token is
rewritten.

Alias rules:
- aliases are accepted as ATM-owned input shorthand only
- recipient aliases must resolve to canonical member names before validation,
  self-send checks, and mailbox lookup
- sender aliases may be accepted on input, but canonical sender identity
  remains the routing and validation identity
- same-team messages keep current canonical sender projection behavior
- cross-team messages may project an alias-oriented sender in the persisted
  `from` field only when ATM also stores canonical sender identity in
  `metadata.atm.fromIdentity`

Post-send-hook rules:
- `[[atm.post_send_hooks]]` is the only supported post-send hook shape in this
  release line
- each rule binds exactly one `recipient` selector and one `command` argv
- `recipient` must be either one concrete team member name or `*`
- multiple matching rules may run for a single send, in config order
- retired `[atm].post_send_hook`, `[atm].post_send_hook_senders`,
  `[atm].post_send_hook_recipients`, and `[atm].post_send_hook_members` keys
  must fail with migration-oriented guidance to `[[atm.post_send_hooks]]`
- a relative hook path must resolve from the directory containing the
  discovered `.atm.toml`
- bare executable names such as `bash`, `python3`, or `tmux` must use normal
  `PATH` resolution
- the hook must execute with the config-root directory as its working directory
- recipient non-match is expected behavior and must be silent
- the hook inherits the process environment and also receives one ATM-owned
  JSON payload in `ATM_POST_SEND`
- the `ATM_POST_SEND` payload must contain:
  - `from`
  - `to`
  - `sender`
  - `recipient`
  - `team`
  - `message_id`
  - `requires_ack`
  - `is_ack`
  - optional `task_id` when present
  - optional `recipient_pane_id` when ATM has an authoritative pane mapping for
    the recipient
- example payload:
  ```json
  {
    "from": "arch-ctm@atm-dev",
    "to": "recipient@atm-dev",
    "sender": "arch-ctm",
    "recipient": "recipient",
    "team": "atm-dev",
    "message_id": "...",
    "requires_ack": false,
    "is_ack": false,
    "recipient_pane_id": "%1"
  }
  ```
- the hook may optionally emit one structured result object on stdout for ATM
  to parse as post-send diagnostics
- the structured hook-result object must support:
  - `level`
  - `message`
  - optional `fields`
- supported hook-result levels are:
  - `debug`
  - `info`
  - `warn`
  - `error`
- missing stdout, empty stdout, oversized stdout, or invalid hook-result schema
  must not fail the send or convert a successful hook execution into a command
  error
- when a valid hook-result object is returned, ATM must log it with the
  declared level and preserve any structured fields
- when a hook is configured, ATM must emit enough diagnostics to explain
  whether the hook ran or failed, including the sender, recipient, and matched
  hook recipient selector
- Phase Q supersession note:
  - the retained release-line hook rules above describe the current
    command-triggered contract
  - the Phase Q target runtime supersedes this with one daemon-owned
    post-store hook trigger for eligible outbound messages, as defined in
    Section 21

## 6. `atm send`

Product requirement ID:
- `REQ-P-SEND-001` `atm send` must satisfy the documented send contract.

Satisfied by:
- `REQ-ATM-CMD-001` for CLI entry, parsing, and dispatch aspects
- `REQ-ATM-OUT-001` for human-readable and JSON output aspects
- `REQ-CORE-CONFIG-002` for address resolution and target-validation aspects
- `REQ-CORE-SEND-001` for send-time missing-config fallback and repair
  notification behavior
- `REQ-CORE-MAILBOX-001` for message creation, duplicate suppression, and
  atomic mailbox mutation aspects

### 6.1 Purpose

Write one message into one target inbox.

### 6.2 Required Flags And Inputs

- positional target: `agent` or `agent@team`
- optional positional message text
- `--team <name>`
- `--file <path>`
- `--stdin`
- `--summary <text>`
- `--json`
- `--dry-run`
- `--from <name>`
- `--requires-ack`
- `--task-id <id>`

Retired from the current implementation:
- `--offline-action`
  - this flag exists only to cooperate with daemon-backed liveness checks and is not retained

### 6.3 Required Behavior

- resolve sender identity using the defined precedence
- resolve recipient address using the defined precedence
- resolve aliases before mailbox lookup
- when a cross-team alias-oriented sender is projected into `from`, also
  persist canonical sender identity in `metadata.atm.fromIdentity` and use the
  canonical sender identity for validation, self-send checks, routing, and
  audit behavior
- verify target team existence and target agent membership as part of address
  resolution before mailbox path selection, except for the documented
  `missing-document` fallback in §6.3.1
- generate summary when not explicitly provided
- enter the atomic append boundary before final inbox mutation
- validate message text inside the atomic append boundary
- generate message id for ATM-authored messages inside the atomic append boundary
- create inbox file if absent inside the atomic append boundary
- preserve duplicate-suppression behavior for message ids inside the atomic append boundary
- append atomically to the inbox file
- support dry-run without mutation
- support sender-controlled ack-required messages
- support optional task metadata on sent messages
- reject retired `post_send_hook_members` config with actionable migration
  guidance before send execution proceeds
- reject retired flat post-send-hook config keys with actionable migration
  guidance before send execution proceeds
- run matching `[[atm.post_send_hooks]]` rules only after successful
  non-`dry-run` sends
- match rules only by resolved recipient identity
- support `recipient = "*"` wildcard matching for all recipients
- execute all matching post-send-hook rules in config order
- support an optional structured hook result on stdout so hook scripts can
  report post-send outcomes such as nudges, no-op conditions, and operator
  errors without relying on stderr scraping
- emit structured diagnostics for hook-rule evaluation and actionable warnings
  only when a configured hook execution fails
- treat `post_send_hook` failure or timeout as best-effort diagnostics only; it
  must not roll back or fail an already-successful send
- write a non-null `message_id` on every ATM-authored message
- current live write compatibility may generate top-level `message_id` values
  using UUID while the metadata-based schema is not yet implemented

Forward schema requirements:

- once ATM writes `messageId` under `metadata.atm`, it must use ULID rather
  than UUID for newly-authored values
- ATM must generate the ULID first and derive the persisted Claude-native
  `timestamp` from that ULID creation instant
- legacy UUID `message_id` remains read-compatible

`message_id` is required on every message written by `atm send`.

`message_id` is optional in the persisted schema (§14.1) only to support
legacy messages written by older clients, but `atm send` never omits it.

Recipients use `message_id` for:
- duplicate suppression
- read-time duplicate collapse
- acknowledgement targeting

### 6.3.1 Missing Team Config Fallback

When team `config.json` is missing, `atm send` may still proceed only when:
- the resolved team directory exists
- the target inbox path already exists
- no team, agent, or routing identity must be guessed

When `atm send` uses this fallback, it must:
- surface an actionable warning to the sender that delivery used inbox fallback
  because team config is missing
- keep the original delivery path best-effort and non-interactive
- send a best-effort repair notification to `team-lead` when that recipient can
  be resolved without guesswork
- deduplicate repeated repair notifications for the same unresolved missing-team
  config condition so inboxes do not accumulate hundreds of identical messages

When team `config.json` is malformed rather than missing:
- `atm send` must fail with a structured configuration error
- malformed config must not silently degrade into missing-config fallback

### 6.4 Message Source Semantics

Exactly one message source must be used:
- positional message text
- `--stdin`
- `--file`

`--file` behavior:
- verify the file exists
- apply the file-access policy
- if allowed, send a file-reference message body
- if not allowed, copy the file into the team share directory and rewrite the message body to reference the share copy

If positional message text is combined with `--file`, preserve the current two-part body shape:

```text
<message text>

File reference: <path or share copy>
```

### 6.5 Ack-Required And Task Metadata

`--requires-ack` means the message must enter the pending-ack queue at write time.

Required behavior:
- write the message with `read = false`
- set `pendingAckAt` to the send timestamp inside the atomic append boundary
- do not wait for a later read to create the ack obligation

`--task-id <id>` attaches task metadata to the message envelope.

Required behavior:
- persist `taskId`
- require acknowledgement for any task-linked message
- reject blank task ids

If `--task-id` is present:
- treat the message as task-linked mail
- imply `--requires-ack`

### 6.6 Output Contract

Human output must include:
- recipient
- sender
- delivery result

JSON output must include:
- `action = "send"`
- `team`
- `agent`
- `outcome`
- `message_id`
- `requires_ack`
- `task_id`

Dry-run JSON output must include:
- `action = "send"`
- `agent`
- `team`
- `message`
- `dry_run = true`
- `requires_ack`
- `task_id`

## 7. `atm read`

Product requirement ID:
- `REQ-P-READ-001` `atm read` must satisfy the documented read/selection/wait
  contract.

Satisfied by:
- `REQ-ATM-CMD-001` for CLI entry, parsing, and dispatch aspects
- `REQ-ATM-OUT-001` for human-readable and JSON output aspects
- `REQ-CORE-CONFIG-002` for target-validation aspects
- `REQ-CORE-MAILBOX-001` for merged inbox load/persist aspects
- `REQ-CORE-WORKFLOW-001` for classification, queue selection, and legal
  transition aspects

### 7.1 Purpose

Read messages from one inbox.

### 7.2 Supported Flags

- optional target: `agent` or `agent@team`
- `--team <name>`
- `--all`
- `--unread-only`
- `--pending-ack-only`
- `--history`
- `--since-last-seen`
- `--no-since-last-seen`
- `--no-mark`
- `--no-update-seen`
- `--limit <n>`
- `--since <iso8601>`
- `--from <name>`
- `--json`
- `--timeout <seconds>`
- `--as <name>`

### 7.3 Required Behavior

- default to the caller’s own inbox when no target agent is provided
- resolve identity and target address using the defined precedence
- verify target team exists
- verify explicit target agent exists in team config
- load messages from the merged inbox surface
- deduplicate entries by `message_id` before bucket selection and output rendering
- classify each message into the read axis, the ack axis, and a derived message class
- map the derived message class into display buckets
- support filtering by sender and timestamp
- support selection by queue mode
- preserve origin-inbox visibility when bridge remotes are configured
- sort newest-first before limiting
- write displayed messages back through the read-axis mutation rules
- persist read-triggered state changes back to the physical inbox file that owns each displayed message when origin inbox files are present in the merged surface
- support optional wait mode with timeout
- support optional seen-state filtering and updates

When multiple inbox entries share the same non-null `message_id`, `atm read`
must display only the most recent entry. Earlier duplicates are silently
suppressed.

Deduplication order:
- compare entries by `message_id`
- keep the newest entry by message timestamp
- when timestamps are equal, keep the later record encountered in inbox order
- do not emit suppressed duplicates in either human or JSON output

`--timeout` preserves the current queue-first behavior: if the requested read selection already contains unread or pending-ack messages at command start, the command returns immediately with those messages. It blocks only when the requested selection is empty at command start.

### 7.4 Display Buckets

The CLI exposes three display buckets:
- `unread`
- `pending_ack`
- `history`

Bucket mapping from the derived message class:
- `Unread` -> `unread`
- `PendingAck` -> `pending_ack`
- `Read` -> `history`
- `Acknowledged` -> `history`

The display buckets are a presentation contract. They are not the canonical two-axis model.

### 7.5 Selection Modes

Default selection is the actionable queue:
- unread
- pending-ack

Explicit selection modes:
- default => actionable queue only
- `--unread-only` => unread bucket only
- `--pending-ack-only` => pending-ack bucket only
- `--history` => actionable queue plus history bucket
- `--all` => all buckets and bypass seen-state filtering

Mutual exclusion:
- `--all`
- `--unread-only`
- `--pending-ack-only`
- `--history`

### 7.6 Seen-State Rules

Seen-state is enabled by default unless `--no-since-last-seen` is set.

`--since-last-seen` explicitly enables the default watermark filter. When set explicitly, it behaves the same as the default. If both `--since-last-seen` and `--no-since-last-seen` appear, `--no-since-last-seen` wins.

When seen-state is enabled and a watermark exists:
- unread messages remain eligible even when older than the watermark
- pending-ack messages remain eligible even when older than the watermark
- history messages are filtered by the watermark

On a true first run with no stored watermark:
- the default read view still shows only actionable messages
- historical messages remain hidden unless `--history` or `--all` is used

`--all` bypasses seen-state filtering entirely.

If seen-state updates are enabled:
- update the watermark using the latest displayed message timestamp
- do not use non-displayed messages when computing the watermark

`--no-update-seen`: when this flag is set, messages are read and displayed normally but the seen-state watermark is not updated after the operation. The watermark is left unchanged regardless of which messages were displayed.

`--since <iso8601>`: filters to messages whose `timestamp` field is greater than or equal to the given ISO 8601 datetime. It filters by message timestamp, not by the seen-state watermark. It may be combined with seen-state filtering; both constraints apply independently.

`--from <name>` in read context is a sender filter: it restricts displayed messages to those sent by the named agent. It does not override the caller's identity.

### 7.7 Wait Mode Rules

When `--timeout <seconds>` is set:
- establish the read selection baseline after actor resolution, inbox loading, workflow classification, and filter application
- if the requested selection already contains eligible messages at wait start, return immediately without blocking
- otherwise block until a newly arrived message becomes eligible for the requested read selection, or until the timeout expires
- re-run the normal read selection over the updated merged inbox surface once a new eligible message arrives
- preserve the same sender, timestamp, seen-state, and selection filters during the wait

Timeout success condition:
- either the initial selection is already non-empty, or at least one message that was not eligible at wait start becomes eligible before the timeout expires

Timeout failure condition:
- the initial selection is empty and no newly eligible message arrives before the timeout expires

### 7.8 Mutation Rules

Base display mutation:
- any displayed message is written back with `read = true`

Ack-axis activation on display happens only when:
- the caller is reading their own inbox
- `--no-mark` is not set
- the message is displayed
- the message is currently `Unread`
- the message does not already require acknowledgement

Required transition on read of a normal unread message:
- `(Unread, NoAckRequired) -> (Read, PendingAck)`

Required transition on read of an ack-required unread message:
- `(Unread, PendingAck) -> (Read, PendingAck)`

Required transition on read with `--no-mark` or when reading another inbox:
- `(Unread, NoAckRequired) -> (Read, NoAckRequired)`

No additional ack-axis mutation happens when:
- the message is already `PendingAck`
- the message is already `Acknowledged`
- the message is already `Read`

### 7.9 Processing Order

1. resolve actor and target inbox
2. build the hostname registry for configured origin inboxes
3. load messages from the merged inbox surface
4. classify canonical state
5. apply sender and timestamp filters (`--from`, `--since`)
6. apply seen-state filter when enabled and selection is not `--all`
7. map canonical state to display buckets and apply selection mode
8. if `--timeout` is set and the current selection is empty, block until a newly eligible message arrives or the timeout expires
9. sort newest-first and apply limit
10. apply read-axis and ack-axis transitions to displayed messages
11. persist read-triggered state changes atomically
12. update seen-state when enabled
13. render output

### 7.10 Output Contract

Human output must preserve the current queue-oriented shape:
- queue heading
- bucket counts line
- bucketed message output
- hidden-history summary when history is collapsed

JSON output must include:
- `action = "read"`
- `team`
- `agent`
- `messages`
- `count`
- `bucket_counts`
- `history_collapsed`

`bucket_counts` fields:
- `unread`
- `pending_ack`
- `history`

## 8. `atm ack`

Product requirement ID:
- `REQ-P-ACK-001` `atm ack` must satisfy the documented acknowledgement
  contract.

Satisfied by:
- `REQ-ATM-CMD-001` for CLI entry, parsing, and dispatch aspects
- `REQ-ATM-OUT-001` for human-readable and JSON output aspects
- `REQ-CORE-MAILBOX-001` for atomic ack persistence and reply append aspects
- `REQ-CORE-WORKFLOW-001` for pending-ack eligibility and acknowledgement
  transition aspects

### 8.1 Purpose

Acknowledge a pending-ack message in the caller's own inbox and send a visible reply to the original sender.

### 8.2 Supported Flags And Inputs

- positional `message-id`
- positional reply text
- `--team <name>`
- `--as <name>`
- `--json`

### 8.3 Required Behavior

- resolve the caller's own inbox using the retained identity rules
- locate the target message in the merged inbox surface
- require the target message to be in the pending-ack ack state
- persist the ack transition back to the physical inbox file that owns the source message when the merged inbox surface includes origin inbox files
- atomically:
  - set `read = true`
  - remove `pendingAckAt`
  - set `acknowledgedAt`
  - append a reply message to the original sender's inbox
- preserve `acknowledgesMessageId` on the emitted reply
- reject duplicate acknowledgement of an already acknowledged message

### 8.4 Output Contract

JSON output must include:
- `action = "ack"`
- `team`
- `agent`
- `message_id`
- `reply_message_id` (Uuid of the reply message sent)
- `reply_text` (String body of the reply message sent)
- `task_id` (optional String, present when the source message has `taskId`)
- `reply_target`

## 9. `atm clear`

Product requirement ID:
- `REQ-P-CLEAR-001` `atm clear` must satisfy the documented clear contract and
  preserve pending-ack protection.

Satisfied by:
- `REQ-ATM-CMD-001` for CLI entry, parsing, and dispatch aspects
- `REQ-ATM-OUT-001` for human-readable and JSON output aspects
- `REQ-CORE-CONFIG-002` for target-validation aspects
- `REQ-CORE-MAILBOX-001` for clear-set persistence aspects
- `REQ-CORE-WORKFLOW-001` for clear-eligibility and pending-ack protection
  aspects

### 9.1 Purpose

Remove non-actionable messages from one inbox without touching actionable work.

### 9.2 Supported Flags

- optional target agent: `agent` or `agent@team`
- `--as <name>` override actor identity for this clear operation
- `--team <name>`
- `--older-than <duration>`
- `--idle-only`
- `--dry-run`
- `--json`

### 9.3 Required Behavior

- default to the caller's own inbox when no target agent is provided
- resolve the target inbox using the retained address and identity rules
- compute clear eligibility from the merged inbox surface
- persist removals back to the physical inbox file that owns each removed message when origin inbox files are present in the merged surface

Default clear behavior removes only clearable messages:
- `(Read, NoAckRequired)`
- `(Read, Acknowledged)`

Clear must never remove:
- `(Unread, NoAckRequired)`
- `(Unread, PendingAck)`
- `(Read, PendingAck)`

Additional rules:
- `--idle-only` narrows removal to idle-notification messages only
- `--older-than` further filters the clearable set by message timestamp age
- dry-run returns the computed removal set without mutation
- clearing must preserve unknown fields on messages that remain

### 9.4 Output Contract

JSON output must include:
- `action = "clear"`
- `team`
- `agent`
- `removed_total`
- `remaining_total`
- removal counters by class

## 10. `atm log`

Product requirement ID:
- `REQ-P-LOG-001` `atm log` must satisfy the documented shared-observability
  query/follow contract.

Satisfied by:
- `REQ-ATM-CMD-001` for CLI entry, parsing, and dispatch aspects
- `REQ-ATM-OUT-001` for record rendering/output aspects
- `REQ-ATM-OBS-001` for CLI observability bootstrap/injection aspects
- `REQ-CORE-LOG-001` for core query/follow/filter behavior aspects
- `REQ-CORE-OBS-001` for ATM event/query-model aspects

### 10.1 Purpose

Inspect ATM observability records through shared `sc-observability` query/follow APIs.

`atm log` replaces the old daemon-log viewing model. It must not depend on daemon-owned log files, daemon status, or tmux fallback behavior.

### 10.2 Supported Flags

- `--tail`
- `--level <trace|debug|info|warn|error>`
- `--match <key=value>` repeatable
- `--since <iso8601|duration>`
- `--limit <n>`
- `--json`

Deferred from the current source repo:
- direct `--file` selection of arbitrary ATM log files
- separate `atm tail` command

### 10.3 Required Behavior

- query existing ATM records through the injected observability port over `sc-observability`
- support follow mode through the same adapter
- support filtering by level
- support filtering by structured key/value fields
- support filtering by time window
- support limit/order controls for non-tail mode
- default to snapshot mode when `--tail` is not set
- return snapshot results newest-first before applying output limits
- return followed records in arrival order while `--tail` is active
- use the built-in shared file-backed retained log store as the authoritative
  query/follow source

### 10.4 ATM Log Fields

The retained ATM event vocabulary must include enough structure to filter on:
- command
- team
- actor
- target
- outcome
- error class

This ATM field set is ATM-owned even when the underlying query/follow/filter mechanics are shared in `sc-observability`.

### 10.5 Output Contract

Human output must show one record per line with enough information to understand:
- timestamp
- severity
- source/service
- event name or message
- important ATM fields when present

JSON output must emit structured records suitable for machine filtering and test assertions.

Each JSON record must expose at least:
- timestamp
- severity
- source or service
- event name
- ATM structured fields map

## 11. `atm doctor`

Product requirement ID:
- `REQ-P-DOCTOR-001` `atm doctor` must satisfy the documented local diagnostics
  contract.

Satisfied by:
- `REQ-ATM-CMD-001` for CLI entry, parsing, and dispatch aspects
- `REQ-ATM-OUT-001` for report rendering/output aspects
- `REQ-ATM-OBS-001` for CLI observability bootstrap/injection aspects
- `REQ-CORE-CONFIG-001` for config and identity inspection aspects
- `REQ-CORE-DOCTOR-001` for diagnostic evaluation aspects

### 11.1 Purpose

Run local ATM diagnostics for the retained ATM runtime.

`atm doctor` remains a local diagnostics command, but in the Phase Q target
architecture it must also report daemon/runtime availability because normal ATM
mail behavior depends on the singleton daemon being present.

### 11.2 Supported Flags

- `--team <name>`
- `--json`

### 11.3 Required Checks

The initial doctor implementation must cover:
- config file discovery and parse health
- effective team resolution
- identity resolution inputs and fallbacks
- obsolete `[atm].identity` configuration drift detection
- daemon control-socket existence and reachability
- singleton daemon ownership health
- SQLite mail-store path visibility and openability when Phase Q runtime is
  active
- baseline `[atm].team_members` coverage against `config.json.members`
- team directory existence
- team config existence and parse health
- inbox directory existence and writability
- stale mailbox lock detection across `~/.claude/teams/*/inboxes/*.lock` using
  start-of-run and end-of-run snapshots; a lock present in both snapshots is
  stale and must be reported with `ATM_WARNING_STALE_MAILBOX_LOCK` as a
  transitional compatibility finding rather than a normal mail-correctness
  dependency in the Phase Q target architecture
- hook identity availability
- `ATM_HOME`, `ATM_TEAM`, and `ATM_IDENTITY` override visibility
- `sc-observability` initialization health
- active shared log path visibility
- `sc-observability` query-health readiness for `atm log`

### 11.4 Output Contract

Human output must provide:
- overall status summary
- findings grouped by severity
- full current member roster from `config.json`, with baseline
  `[atm].team_members` shown first and `team-lead` first among that baseline
- concrete remediation guidance when the user can act

JSON output must provide:
- summary
- findings
- recommendations
- environment override visibility
- member roster
- observability health snapshot

Each doctor finding must expose at least:
- severity
- code
- message
- remediation when available

The obsolete config-identity finding must use:
- `ATM_WARNING_IDENTITY_DRIFT`

Critical findings must cause a non-zero exit status.

## 12. `atm teams`

Product requirement ID:
- `REQ-P-TEAMS-001` `atm teams` must satisfy the documented retained local
  team recovery contract.

Satisfied by:
- `REQ-ATM-CMD-001` for CLI entry, parsing, and dispatch aspects
- `REQ-ATM-OUT-001` for human-readable and JSON output aspects
- `REQ-CORE-TEAM-001` for local team discovery, roster mutation, and
  backup/restore behavior

### 12.1 Purpose

Provide the minimum retained local team-recovery surface required for initial
release and the documented backup/restore workflow.

### 12.2 Retained Surface

The retained `teams` surface for initial release is:
- `atm teams`
- `atm teams add-member`
- `atm teams backup`
- `atm teams restore`

The retained surface explicitly does not include broader historical team
orchestration commands such as:
- `spawn`
- `join`
- `resume`
- `update-member`
- `remove-member`
- `cleanup`

### 12.3 Required Behavior

Bare `atm teams` must:
- list discovered teams under ATM home deterministically
- expose at least team name plus enough summary information, such as member
  count, to pick a target team for restore or repair work

`atm teams add-member` must:
- validate that the target team exists
- reject duplicate member names
- persist the new member entry deterministically in team config
- create any required local inbox state atomically with the roster update

`atm teams backup` must:
- create a timestamped snapshot under the ATM team backup area
- capture the current `config.json`
- capture team inbox files, excluding transient `*.lock` sentinels, dotfiles,
  and restore markers
- capture the ATM team task bucket
- report the created backup path in human and JSON output
- not claim to back up the separate Claude Code project task list

`atm teams restore` must:
- restore from the newest snapshot by default or from an explicit backup path
- support a dry-run mode that reports members, inboxes, and tasks that would
  be restored
- preserve the current team-lead entry and current `leadSessionId`
- add only missing non-lead members from the snapshot
- clear runtime-only restored-member fields such as session, activity, and
  pane state before persisting them
- restore non-lead inbox files from the chosen snapshot deterministically
- sweep stale inbox `*.lock` sentinels before copying restored inbox files as a
  self-heal step
- restore the ATM team task bucket and recompute `.highwatermark` from the
  maximum restored task id
- fail with a structured error when backup material is missing or malformed
- avoid partial restore on validation or snapshot-load failure

### 12.4 Output Contract

Human output must make the performed action and target team clear.

JSON output must include:
- `action`
- `team`

`add-member` JSON output must additionally include:
- `member`

`backup` JSON output must additionally include:
- `backup_path`

`restore` JSON output must additionally include:
- `backup_path`
- `members_restored`
- `inboxes_restored`
- `tasks_restored`

Dry-run `restore` JSON output must additionally include:
- `dry_run = true`
- `would_restore_members`
- `would_restore_inboxes`
- `would_restore_tasks`

## 13. `atm members`

Product requirement ID:
- `REQ-P-MEMBERS-001` `atm members` must satisfy the documented local roster
  inspection contract.

Satisfied by:
- `REQ-ATM-CMD-001` for CLI entry, parsing, and dispatch aspects
- `REQ-ATM-OUT-001` for human-readable and JSON output aspects
- `REQ-CORE-TEAM-001` for local roster loading and deterministic projection

### 13.1 Purpose

List the current local team roster for verification, recovery, and restore
follow-up without depending on daemon-only or hook-only state.

### 13.2 Supported Flags

- `--team <name>`
- `--json`

### 13.3 Required Behavior

`atm members` must:
- resolve the effective team using the retained team-resolution rules
- load the local team roster from `config.json`
- return a structured error when the team or team config is missing
- show all configured members deterministically, with `team-lead` first when
  present and remaining members in stable local order
- expose currently persisted member metadata that ATM already knows locally,
  such as type, model, cwd, or pane id when present in config
- remain useful without daemon or hook state

Richer runtime state, such as live session or activity data, may be layered on
later, but it is not required for the retained local release surface.

### 13.4 Output Contract

Human output must show:
- team name
- one row per member
- enough persisted member detail to verify roster repair or restore outcomes

JSON output must include:
- `team`
- `members`

Each member object must expose at least:
- `name`
- persisted local member metadata when present

## 14. Message And Workflow Model

Product requirement ID:
- `REQ-P-WORKFLOW-001` The message/workflow model must satisfy the documented
  persisted-field, two-axis, and legal-transition rules.

Satisfied by:
- `REQ-CORE-WORKFLOW-001` for the canonical two-axis model and legal
  transitions

### 14.1 Persisted Message Fields

Required fields:
- `from`
- `text`
- `timestamp`
- `read`

Optional fields:
- `source_team`
- `summary`
- `message_id`
- `taskId`
- `pendingAckAt`
- `acknowledgedAt`
- `acknowledgesMessageId`
- `metadata`

Unknown fields must be preserved.

For ATM-authored messages:
- ATM machine-readable identity is mandatory
- current legacy top-level `message_id` values may be UUID
- forward metadata `messageId` values must be ULID
- ATM-authored machine identifiers must not be null or blank

Legacy or externally imported records may still omit `message_id`; the rewrite
must preserve such records without inventing synthetic ids during read.

### 14.2 Two-Axis Canonical Model

The canonical model has two independent axes.

Read axis:
- `Unread`
- `Read`

Ack axis:
- `NoAckRequired`
- `PendingAck`
- `Acknowledged`

Persisted-field classification:
- read axis:
  - `read = false` => `Unread`
  - `read = true` => `Read`
- ack axis:
  - `acknowledgedAt` present => `Acknowledged`
  - else `pendingAckAt` present => `PendingAck`
  - else => `NoAckRequired`

Derived message class for queue logic:
1. ack axis `PendingAck` => `PendingAck`
2. else ack axis `Acknowledged` => `Acknowledged`
3. else read axis `Unread` => `Unread`
4. else => `Read`

The canonical two-axis model is distinct from the read command’s display buckets.

### 14.3 Required State Transitions

```text
Send normal message
  -> (Unread, NoAckRequired)

Send ack-required message
  -> (Unread, PendingAck)

Send task-linked message
  -> persist taskId
  -> (Unread, PendingAck)

Read own inbox with marking enabled, normal unread message
  (Unread, NoAckRequired) -> (Read, PendingAck)

Read own inbox with marking enabled, ack-required unread message
  (Unread, PendingAck) -> (Read, PendingAck)

Read own inbox with --no-mark
  (Unread, NoAckRequired) -> (Read, NoAckRequired)
  (Unread, PendingAck) -> (Read, PendingAck)

Read another inbox
  (Unread, NoAckRequired) -> (Read, NoAckRequired)
  (Unread, PendingAck) -> (Read, PendingAck)
  (Read, PendingAck) -> (Read, PendingAck)
  (Read, Acknowledged) -> (Read, Acknowledged)
  (Read, NoAckRequired) -> (Read, NoAckRequired)

Ack workflow
  (Read, PendingAck) -> (Read, Acknowledged)
  and emit a reply message that references the original message id

Clear workflow
  remove only (Read, NoAckRequired) and (Read, Acknowledged)
```

Disallowed transitions:
- any transition that makes the read axis move from `Read` back to `Unread`
- `Acknowledged -> PendingAck`
- `Acknowledged -> NoAckRequired`
- clearing a message in `PendingAck`
- clearing a message with read axis `Unread`

The implementation must encode legal transitions in code structure, not only in comments or tests.

### 14.4 Task Metadata Rule

Messages with `taskId` are task-linked messages.

Required rules:
- every task-linked message must require acknowledgement
- a task-linked message remains actionable until acknowledged
- a task-linked message must continue to appear in `atm read` until acknowledged
- a task-linked message must never be removed by `atm clear` before acknowledgement

## 15. Observability Requirements

Product requirement ID:
- `REQ-P-OBS-001` ATM observability must satisfy the documented best-effort
  emit behavior and shared query/follow/health expectations.

Satisfied by:
- `REQ-ATM-OBS-001` for CLI bootstrap/injection aspects
- `REQ-CORE-LOG-001` for ATM log query/follow service aspects
- `REQ-CORE-DOCTOR-001` for observability health reporting aspects
- `REQ-CORE-OBS-001` for ATM event and query-model boundary aspects

ATM must emit structured records through `sc-observability`.

Initial shared integration scope:
- `sc-observability-types`
- `sc-observability`

Deferred from the initial retained observability integration:
- `sc-observe`
- `sc-observability-otlp`

Required ATM event classes:
- command started
- command succeeded
- command failed
- mailbox record skipped

Required ATM event fields:
- command name
- team when known
- actor identity when known
- target identity when known
- task id when known
- result
- error class on failure
- count when applicable
- transition count when applicable

Emission is best-effort:
- logging failures must never block retained command behavior
- command correctness takes priority over observability delivery

Sink policy:
- the shared file sink is required for retained ATM observability
- the shared console sink is optional and must remain off by default for normal
  ATM CLI command execution so command output stays stable
- console logging may be enabled later for explicit local debugging or
  integration testing

Diagnostic logging rules:
- command failures must emit structured failure diagnostics before the CLI
  exits, even when the command fails before reaching a core service
- degraded recovery paths that intentionally continue, such as malformed-record
  skips or missing-config fallback warnings, must also emit structured warning
  diagnostics
- every ATM warning/error diagnostic must carry a stable ATM-owned error code in
  addition to human-readable text
- command lifecycle failure events must include the stable error code when one
  is available

`atm log` and `atm doctor` are not best-effort features in the same sense:
- they are explicit observability consumers
- if shared query/health APIs are unavailable, they must fail with clear structured errors

## 16. Error Requirements

Product requirement ID:
- `REQ-P-ERROR-001` Public command failures must satisfy the documented
  structured error requirements.

Satisfied by:
- intentionally undecomposed product requirement; crate-local error ownership
  remains derived from command and service requirements rather than a dedicated
  crate requirement ID in this pass

All user-visible failures must use structured errors with recovery guidance.

Persisted-data failures must preserve parser and entity context when available.

Stable error-code rule:
- every public `AtmError` must map to a stable ATM-owned error code
- ATM warning and error logs must include that code
- CLI bootstrap and argument-validation failures must also be logged with a
  stable error code before process exit
- the single source of truth for ATM-owned error codes is
  [`atm-error-codes.md`](./atm-error-codes.md)

Minimum error categories:
- configuration
- missing document
- address
- identity resolution
- team not found
- agent not found
- mailbox read
- mailbox write
- message validation
- store
- ingest
- export
- transport
- daemon runtime
- daemon singleton
- daemon client

Phase Q required families:
- store:
  - SQLite bootstrap/open
  - schema/transaction
  - busy-timeout / saturation
- ingest:
  - replay/import failure
  - backpressure/degraded ingest
- export:
  - Claude compatibility export failure
  - re-export/replay failure
- transport:
  - local daemon request failure
  - remote connect/timeout/protocol failure
- daemon runtime:
  - shutdown timeout
  - signal/reload failure
  - runtime over-capacity
- daemon singleton:
  - already-running daemon
  - stale-artifact cleanup/release failure
- daemon client:
  - daemon unavailable
  - daemon health-query timeout
- serialization
- file policy
- wait timeout
- observability emit
- observability query
- observability health

Mutation failures must be fail-safe:
- no partial send writes
- no partial read-mark updates
- no illegal state transitions after failed persistence

## 17. Reliability Requirements

Product requirement ID:
- `REQ-P-RELIABILITY-001` The retained command surface must satisfy the
  documented durability and consistency constraints.

Satisfied by:
- `REQ-CORE-MAILBOX-001` for atomicity, duplicate suppression, and mailbox
  consistency aspects

- mailbox writes must be atomic
- concurrent appends must not silently lose messages
- duplicate message ids must not be appended twice
- read-time duplicate message ids collapse to the newest visible entry
- corrupt records should be skipped individually when possible
- persisted config/team schema drift should recover with deterministic defaults
  when safe
- missing team config may use only the explicitly documented send fallback
  behavior
- persisted config/team records with missing identity or routing-critical fields
  must fail or be isolated rather than guessed
- missing inbox files are treated as empty inboxes
- seen-state races must not corrupt mailbox data
- observability emission failures must not corrupt command behavior

## 18. Testing Requirements

Product requirement ID:
- `REQ-P-TEST-001` The rewrite must satisfy the documented testing obligations.

Satisfied by:
- intentionally undecomposed product requirement; this governs workspace-level
  test coverage expectations rather than a single crate-local requirement ID

Because `sc-observability` is newly introduced into ATM, the rewrite must add explicit test coverage for:
- ATM event emission through the observability port boundary
- best-effort emission failure behavior
- two-axis state classification
- two-axis state transition enforcement
- task-linked ack-required transition behavior
- log query by severity
- log query by structured field match
- log follow/tail behavior
- doctor observability-health reporting
- teams list behavior over the local ATM home
- members list behavior over local team config
- add-member duplicate validation and inbox creation
- backup snapshot completeness
- restore dry-run reporting
- restore preservation of team-lead / `leadSessionId`
- restore recomputation of `.highwatermark` to the maximum restored task id
- retained mail-command correctness when observability emission fails
- clear eligibility behavior

The implementation must include:
- `atm-core` tests for observability port behavior using test doubles
- CLI integration tests for `atm log`
- CLI integration tests for `atm doctor`
- CLI integration tests for `atm ack`
- CLI integration tests for `atm clear`
- CLI integration tests for `atm teams`
- CLI integration tests for `atm members`

## 19. Acceptance Criteria

Product requirement ID:
- `REQ-P-ACCEPTANCE-001` The rewrite is complete only when the documented
  acceptance criteria are met.

Satisfied by:
- intentionally undecomposed product requirement; this defines overall product
  completion gates rather than a single crate-local obligation

The rewrite is ready when:
- `atm send` works through the documented production runtime path
- `atm read` works through the documented production runtime path
- `atm ack` works through the documented production runtime path
- `atm clear` works through the documented production runtime path
- `atm log` works through shared `sc-observability` APIs
- `atm doctor` works as a local diagnostics command with daemon/runtime
  visibility in the Phase Q target architecture
- `atm teams` provides the retained local team recovery surface
- `atm members` provides the retained local roster verification surface
- retained commands preserve documented behavior, and any Phase Q runtime-shape
  changes are explicit in the requirements and architecture
- workflow-axis classification is correct
- workflow-axis transitions are encoded in implementation structure
- display buckets are derived consistently from the two-axis model
- task-linked messages remain pending until acknowledged unless the operator
  explicitly acknowledges them through `atm ack`
- observability integration is exercised by automated tests
- the file-by-file migration plan is complete enough to implement directly

Cross-document invariants that must remain true:
- `taskId` implies ack-required behavior at send time
- displayed messages always persist `read = true`
- pending-ack messages remain actionable until acknowledged
- `atm clear` never removes unread messages
- `atm clear` never removes pending-ack messages
- `atm read --timeout` returns immediately when the requested selection is already non-empty


## 20. Phase M: Mailbox Concurrency And Restore Atomicity

Phase M addresses blocking and important findings from the Phase L code review
(ARCH-CR-001 through ARCH-CR-004 and associated QA findings) that must be
closed before the 1.0 release.

### 20.1 Mailbox Concurrency Safety

- `REQ-CORE-MAILBOX-LOCK-001` All mailbox read-modify-write operations must
  hold an exclusive advisory file lock for the duration of the operation.

  Rationale: `append_message` in `mailbox/mod.rs` currently reads the full
  inbox, appends one record in memory, then calls `atomic::write_messages` to
  replace the file. Two concurrent writers can both read the same snapshot and
  the later rename silently drops the earlier writer's append. This is ARCH-CR-001.

  Required behavior:
  - before entering any read-modify-write section on an inbox file, ATM must
    acquire an exclusive advisory lock on a well-known lock sentinel derived from
    the inbox path
  - the lock must be held for the full duration of read + modify + atomic
    replacement, including any durability sync that is part of the shared
    atomic-write helper boundary
  - lock release must happen automatically when the lock guard is dropped (RAII)
  - lock acquisition must use a bounded timeout (default 5 seconds) and fail
    with a structured `AtmError` carrying `AtmErrorCode::MailboxLockTimeout`
    when the timeout expires
  - the lock sentinel path is a transient runtime artifact: ATM writes the
    owner pid while the lock is held, unlinks the sentinel on guard drop, and
    must tolerate stale pid-bearing sentinels from crashed processes
  - advisory locking is cooperative: only concurrent ATM processes coordinate
  - the sentinel lock must not block Claude Code's native inbox appends because
    Claude does not participate in ATM's cooperative lock protocol

- `REQ-CORE-MAILBOX-LOCK-002` Mailbox locking must work on macOS, Linux, and
  Windows without platform-specific feature flags in consuming code.

  Required behavior:
  - on Unix: use `flock(2)` exclusive lock on the lock sentinel file descriptor
  - on Windows: use `LockFileEx` exclusive lock on the lock sentinel file handle
  - the public API must present a single `MailboxLockGuard` type that is
    platform-uniform; platform branching is internal to `lock.rs`
  - the `fs2` crate is the preferred implementation

- `REQ-CORE-MAILBOX-LOCK-003` Locks must be per-inbox-file, not per-team or global.

  Required behavior:
  - locking is scoped to a single inbox file path
  - two concurrent `atm send` commands to different recipients must not block each other
  - the lock sentinel path is `{inbox_path}.lock`

- `REQ-CORE-MAILBOX-LOCK-004` Every mailbox mutation path must acquire the lock.

  Required coverage:
  - `append_message` for both normal send and the missing-config team-lead notice path
  - workflow state writeback in read, ack, and clear paths
  - any future mutation path added to the mailbox layer

  Read-only `read_messages` calls with no following writeback do not require locking.

- `REQ-CORE-MAILBOX-LOCK-005` Multi-source mailbox commands must acquire their
  final required lock set before any mutating source reread, and must do so in
  deterministic path order.

  Rationale: `read`, `ack`, and `clear` do not operate on a single inbox file.
  The executed Phase P design permits unlocked observational snapshots when no
  mutation is committed from that snapshot, but any state-changing path must
  reacquire the full deterministic lock set, reload fresh source files under
  that lock set, recompute the mutation, and then persist. Locking only during
  the final write step would still allow stale reads and lost updates.

  Required behavior:
  - `read` is a `read_possible_write` path: it may take an unlocked
    observational snapshot of the source inbox set,
    but if display-state mutation is needed it must re-discover the current
    source-file set, dedupe duplicate paths, sort the resulting paths
    deterministically by canonical path string, acquire the full lock set, then
    reload and recompute under that lock set before persisting
  - `ack` uses an unlocked preflight plus one final superset lock: it may
    resolve the reply target and candidate source message from an unlocked
    preflight, but it must acquire the final sorted superset lock plan before
    the mutating source reread, then re-read and re-validate the pending
    acknowledgement state under that final lock set before writing either the
    source or reply mailbox state
  - mutating `clear` is a full-lock-through-persist path: it must acquire the
    deterministic lock set before its
    mutating source reread and must hold that lock set through removal
    computation, mailbox replacement, and workflow-sidecar updates; `clear
    --dry-run` remains observational and lock-free
  - final source-file discovery for a mutating path must use the command's
    existing requested-inbox plus origin-inbox resolution logic
  - legitimately absent inbox paths at discovery time are excluded from the
    lock set rather than locked speculatively
  - source enumeration faults are not treated as absent paths; if origin inbox
    discovery cannot enumerate the candidate directory completely, the command
    must fail closed instead of continuing with a partial source set
  - for any mutating path, those locks must remain held through the fresh
    surface computation, state transition, and final writeback
  - deterministic ordering must prevent deadlock when two commands contend on the
    same pair of inbox files in opposite discovery order
  - lock acquisition uses one total timeout budget for the full lock set, not a
    fresh timeout per file
  - if any lock in the set cannot be acquired, every previously acquired lock in
    that attempt must be released immediately and the command must fail without
    mutating any source inbox from a partially locked snapshot
  - partial lock acquisition must never degrade into a best-effort state-changing
    command result for `read`, `ack`, or `clear`
  - the unlocked observational snapshot used by `read`, `ack`, or dry-run
    `clear` must never be the snapshot from which a later mutating commit is
    persisted
  - source discovery for mutating commands must fail closed: if directory
    enumeration itself fails or if any directory entry in the candidate inbox
    directory cannot be enumerated reliably, the command must abort before the
    mutating reread instead of warning and continuing with a partial source set
  - if a discovered file disappears or becomes unreadable after lock planning
    but before or during the under-lock source-file load, the command must fail
    as a normal operator-actionable file-read error and must not persist any
    partial state

- `REQ-CORE-MAILBOX-LOCK-006` Single-process single-threaded usage must not
  regress measurably due to lock acquisition.

  Required behavior:
  - uncontended `flock` is a single syscall returning immediately; no background
    threads or polling loops
  - lock sentinel created lazily on first lock attempt

- `REQ-CORE-MAILBOX-LOCK-007` Lock acquisition must distinguish true lock
  contention from other lock-path I/O failures.

  Required behavior:
  - only retry errors that actually mean "lock currently held by another
    process" for the current platform/API surface
  - if the sentinel file cannot be opened, locked, or queried because of a
    non-contention I/O or OS error, fail immediately with `MailboxLockFailed`
    rather than sleeping until the timeout budget expires
  - `MailboxLockTimeout` is reserved for genuine contention or equivalent
    lock-busy conditions
  - operator recovery guidance must distinguish "wait and retry" from
    "repair filesystem/permissions state"

- `REQ-CORE-MAILBOX-LOCK-008` Stale-lock sweeping must identify rotated lock
  sentinels conservatively and must evict only verifiable orphaned candidates.

  Required behavior:
  - candidate matching is based on the basename, not the full path
  - the accepted sentinel predicate is:
    `file_name.ends_with(".lock") || file_name.contains(".lock.")`
  - the sweep must not use `path.extension() == "lock"` because that misses
    rotated sentinels such as `inbox.json.lock.old`
  - the sweep must not broaden to arbitrary substring matching such as
    `contains("lock")`; non-sentinel files like `locksmith.txt` must not be
    considered
  - a matched candidate is evictable only when its contents parse as the
    documented `pid[:token]` owner record format and `process_is_alive(pid)`
    returns false
  - malformed or unreadable candidate contents are treated as non-evictable and
    must be left in place for explicit operator cleanup instead of speculative
    deletion
  - the sweep is a best-effort stale-artifact cleanup path, not a second lock
    authority; it must not claim ownership without the existing advisory-lock
    acquisition succeeding afterward
  - Windows rename semantics must not be assumed to match Unix for a live held
    lock handle; rotated-name sweeping exists to clean up post-crash or
    externally renamed artifacts, not to coordinate live-lock handoff

  Acceptance Criteria:
  - positive predicate cases: `inbox.json.lock`, `inbox.json.lock.old`, and
    `inbox.json.lock.replaced` are all treated as stale-sentinel candidates
  - negative predicate cases: malformed or unrelated names such as
    `inbox.json.lockold`, `locksmith.txt`, and `inbox.locksmith.json` are not
    treated as stale-sentinel candidates
  - malformed rotated candidates that do match the filename predicate but do
    not contain a parseable `pid[:token]` owner record remain in place and are
    not deleted speculatively

- `REQ-CORE-MAILBOX-LOCK-009` Read-only filesystem failures on the mailbox-lock
  path must surface as a dedicated non-contention diagnostic.

  Required behavior:
  - ATM must classify read-only filesystem errors by raw OS error code rather
    than treating them as generic permission failures
  - the required platform mappings are:
    - Linux: `EROFS` (`30`)
    - macOS: `EROFS` (`30`)
    - Windows: `ERROR_WRITE_PROTECT` (`19`)
  - the same classification helper must be used for lock-path open/create,
    owner-record truncate/write, and sentinel removal so retry behavior and
    operator guidance stay consistent
  - read-only filesystem errors must not participate in the lock-contention
    retry loop and must not be retried by sentinel-removal backoff logic
  - on every lock-acquisition retry iteration, read-only-filesystem
    classification must run before any timeout-budget decision; a classified
    `EROFS` / `ERROR_WRITE_PROTECT` failure must never fall through to
    `MailboxLockTimeout`
  - mutation-path failures caused by a read-only filesystem must return
    `MailboxLockReadOnlyFilesystem`
    / `ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM`, not `MailboxLockFailed` or
    `MailboxLockTimeout`
  - the structured error message and recovery guidance must include the lock
    path plus the specific attempted operation (`open`, `write owner record`,
    or `remove stale sentinel`) so operators can distinguish remount/media
    failures from ACL or contention issues
  - other non-contention lock-path filesystem failures, including `ENOSPC`,
    `EMFILE`, and `ESTALE`, remain `MailboxLockFailed` and are not retried
  - best-effort drop-time cleanup remains warn-only because the command has
    already completed, but public sweep or acquisition paths must surface the
    read-only diagnosis instead of silently suppressing it

  Acceptance Criteria:
  - `ATM_TEST_FORCE_LOCK_READONLY_FS=open` injects a synthetic platform-correct
    read-only-filesystem error into the lock open/create path only; owner-record
    write and sentinel-removal paths continue to run normally
  - `ATM_TEST_FORCE_LOCK_READONLY_FS=write_owner` injects a synthetic
    read-only-filesystem error into the owner-record truncate/write path only
  - `ATM_TEST_FORCE_LOCK_READONLY_FS=remove` injects a synthetic
    read-only-filesystem error into the stale-sentinel removal path only
  - when the seam is unset or set to any other value, no synthetic read-only
    filesystem failure is injected
  - read-only failures injected through any of the three seam values surface as
    `MailboxLockReadOnlyFilesystem`
    / `ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM`, never as `MailboxLockTimeout`

### 20.2 Shared Mutable File Atomicity

- `REQ-CORE-PERSIST-ATOMIC-001` Every shared mutable ATM-owned structured state
  file must be persisted atomically.

  Scope:
  - live inbox files under `.claude/teams/<team>/inboxes/*.json`
  - team `config.json`
  - ATM-owned task-bucket JSON/state files written during backup/restore flows
  - `.highwatermark` and any equivalent ATM-owned monotonic task-state file
  - send-alert / restore-progress / similar ATM-owned persisted coordination
    state when that state is shared across processes or operators
  - any future ATM-owned JSON or JSONL file that can be rewritten by more than
    one ATM process, agent, or operator workflow

  Required behavior:
  - live-file replacement must use a temp-file + fsync + rename pattern or an
    equivalent same-filesystem atomic-replacement mechanism
  - for files replaced via rename, the helper must fsync the parent directory
    after the rename whenever the platform allows directory-sync semantics, so
    successful return means both file contents and name publication are durably
    committed as far as the host platform can provide
  - no live shared structured file may be truncated and rewritten in place
  - mailbox locking does not replace atomic persistence; both are required for
    mailbox files
  - temp-file + rename atomicity alone is not a source-unchanged compare-and-swap
    against non-cooperating writers, so ATM must not claim mailbox rewrite
    safety for concurrent Claude Code appends

- `REQ-CORE-PERSIST-ATOMIC-001A` Shared mutable file commits must use one of
  the documented mutation classes and the shared commit protocol.

  Required behavior:
  - `read_only` paths must not acquire mailbox/file locks
  - `read_possible_write` paths may do an initial unlocked read, but any actual
    commit must prove source freshness before replacing the live file
  - `read_modify_write` paths must also prove source freshness before replacing
    the live file
  - acceptable freshness proofs are limited to:
    - compare-and-swap against the exact earlier snapshot, or
    - lock, reread current state, recompute, and then commit
  - a stale-snapshot rename after late lock acquisition is forbidden even if
    the rename itself is atomic

  Open hardening gap — `P.6` send-side workflow freshness:
  - mailbox read/ack/clear paths satisfy this through
    `mailbox::store::with_locked_source_files(...)`
  - workflow-sidecar writes performed during `send` and the missing-config
    team-lead notice path are already atomic and owner-routed, but they do not
    yet provide a dedicated freshness proof across concurrent same-recipient
    sends; P.6 is the tracked hardening item for that gap

- `REQ-CORE-PERSIST-ATOMIC-001B` Every shared mutable file family must have one
  documented write path and one owning helper boundary.

  Required behavior:
  - mailbox file replacement must go through the mailbox atomic helper family
  - shared generic state replacement must go through the shared persistence
    helper family
  - new live structured files must not introduce bespoke `fs::write`,
    truncate-and-rewrite, or ad hoc temp-file logic at individual call sites
  - if a file family needs special preconditions such as lock ordering or
    freshness validation, those preconditions must be enforced at the shared
    helper boundary or a single owner-layer wrapper around it
  - the current owner-layer set is:
    - mailbox compatibility surface:
      `mailbox::store::observe_source_files(...)` for lock-free snapshots,
      `mailbox::store::with_locked_source_files(...)` for shared read/ack/clear
      lock+reload orchestration, and `mailbox::store::commit_mailbox_state(...)`
      / `mailbox::store::commit_source_files(...)` as the persistence leaf
    - workflow-state sidecar:
      `workflow::{load_workflow_state(...), save_workflow_state(...),
      project_envelope(...), remember_initial_state(...),
      apply_projected_state(...), remove_message_state(...)}`
    - seen-state watermark:
      `read::seen_state::save_seen_watermark(...)`
    - send-alert state:
      `send::alert_state::{register_missing_team_config_alert(...),
      clear_missing_team_config_alert(...), save(...), acquire_lock(...)}`
    - team config:
      `team_admin::write_team_config(...)`
    - task bucket and `.highwatermark`:
      `team_admin::restore::restore_task_state_from_backup(...)`
    - restore marker and restore staging:
      `team_admin::restore::write_restore_marker(...)`,
      `team_admin::restore::clear_restore_marker(...)`,
      `team_admin::restore::prepare_restore_workspace(...)`, and
      `team_admin::restore::cleanup_restore_workspace(...)`
  - send-side workflow seeding must not continue indefinitely as an open-coded
    `load -> mutate -> save` sequence in command-layer logic; P.6 exists to
    converge that path onto a dedicated owner-layer freshness boundary

- `REQ-CORE-PERSIST-ATOMIC-001C` ATM must not claim rewrite safety for
  non-cooperating external writers.

  Required behavior:
  - if a live file can be concurrently changed by a writer outside ATM’s lock
    protocol, ATM must document whether that file is:
    - read-only from ATM’s perspective, or
    - a legacy compatibility surface with known overwrite risk, or
    - protected by real freshness validation/CAS
  - for Claude-owned inbox files, advisory lock correctness applies only to
    concurrent ATM writers
  - ATM-local workflow state that requires stronger guarantees must move to an
    ATM-owned source-of-truth path rather than relying on full-file rewrite of
    the Claude-owned inbox surface

- `REQ-CORE-PERSIST-ATOMIC-002` Phase M must treat atomic persistence as a
  cross-cutting invariant, not a mailbox-only or restore-only rule.

  Required behavior:
  - when Phase M touches a shared mutable structured file path, the
    implementation must either route that path through an existing atomic write
    helper or add one before modifying the file
  - new shared mutable JSON/JSONL/state files introduced during Phase M must
    adopt the same atomic persistence contract immediately rather than deferring
    to a follow-on cleanup sprint

- `REQ-CORE-PERSIST-ATOMIC-003` Atomic persistence helpers must be centralized
  and reused instead of duplicated ad hoc at call sites.

  Required behavior:
  - `atm-core` must own the shared atomic persistence primitive used by mailbox,
    config, task-bucket, highwatermark, and shared coordination writers
  - mailbox writes continue using the mailbox atomic helper
  - team-config writes continue using `write_team_config(...)`
  - task-bucket / highwatermark / shared state writes added or touched by Phase M
    must use a documented helper with the same temp-file + rename semantics
  - the Phase M audit must grep for direct `fs::write`, `File::create`, or
    equivalent in-place rewrites of live shared mutable structured files and
    either remove them or document why the path is not in scope

### 20.2.1 Shared Commit And Freshness Validation

The required shared commit protocol is:

1. classify the operation as `read_only`, `read_possible_write`, or
   `read_modify_write`
2. perform any unlocked observational read allowed by that class
3. compute whether a write is necessary
4. if no write is needed, return without locking
5. if a write is needed, enter the owning write path for that file family
6. prove source freshness by CAS or by lock + reread + recompute
7. write the temp file, fsync, rename, and perform any required directory sync

The intentionally forbidden shape is:

- read old snapshot
- compute mutation from old snapshot
- acquire late lock
- rename blindly over a newer live file

### 20.2.2 Locking Failure-Path Test Contract

- `REQ-CORE-MAILBOX-TEST-001` Phase M follow-up coverage must include
  deterministic failure-path locking tests in addition to success-path
  no-deadlock tests.

  Required behavior:
  - add bounded tests for lock contention timeout on the mutation commands that
    use mailbox locking; for the follow-up sprint the explicit command coverage
    list is `send` for contention timeout, `clear` for fail-closed discovery,
    and `send` for non-contention lock-error classification
  - add deterministic coverage for fail-closed source discovery when an origin
    inbox directory entry cannot be enumerated successfully
  - add deterministic coverage for non-contention lock-path failures so they do
    not regress into `MailboxLockTimeout`

- `REQ-CORE-MAILBOX-TEST-002` Locking tests must use bounded, non-flaky
  construction that cannot hang indefinitely.

  Required behavior:
  - use explicit timeout-based synchronization (`recv_timeout`,
    `wait_timeout`, elapsed-time assertions with bounded slack) rather than
    open-ended thread joins or sleeps waiting for success
  - tests for directory-entry enumeration failure must use a deterministic seam
    or injected enumerator/fault source rather than permission tricks, racing
    deletes, or environment-sensitive filesystem behavior
  - tests for non-contention lock errors must use a deterministic seam or
    injectable failure source rather than depending on platform-specific errno
    behavior
  - tests that intentionally hold a lock must guarantee teardown via scoped
    guards/channels even when the assertion path fails
  - crash-durability helper tests should verify sequencing and error propagation
    through deterministic seams; they must not rely on real crash simulation
  - forbidden test patterns:
    - open-ended polling waiting for "eventual" success
    - indefinite `join()` or blocking wait with no timeout
    - sleeps used as the primary correctness mechanism
    - race-dependent stress loops expected to pass only "most of the time"

### 20.3 Restore Transaction Atomicity

- `REQ-CORE-RESTORE-ATOMIC-001` `teams restore` must write `config.json` as
  the last mutation step, only after all other restore mutations succeed.

  Rationale: ARCH-CR-002 — `team_admin.rs:372-400` copies inboxes, restores
  tasks, recomputes highwatermark, then writes config. If the process dies
  between inbox copy and config write, the team has partially restored inbox
  files that do not match the config roster.

  Required behavior:
  - restore planning and backup validation happen before the marker is written
  - config.json is written last, after all inbox copies and task restores succeed
  - a `.restore-in-progress` marker file is written to the team directory before
    mutation begins and removed after config is successfully fsynced
  - the config-last step must continue using the existing `write_team_config(...)`
    atomic temp-file + rename pattern instead of introducing a second config
    persistence path
  - on next `atm teams restore`, if a `.restore-in-progress` marker exists, warn
    the operator and recommend re-running the restore
  - `atm doctor` must check for stale `.restore-in-progress` markers and report
    them as findings with recovery guidance

- `REQ-CORE-RESTORE-ATOMIC-002` Restored inbox files must be staged before
  being placed in the live inbox directory.

  Required behavior:
  - inbox files from the backup must first be copied to `.restore-staging/inboxes/`
  - after all staging copies succeed, move staged files to the live inboxes
    directory using `fs::rename` where possible
  - on staging or move failure, clean up the staging directory and fail without
    writing config
  - if stale staging already exists at restore start, the command must either
    clean it first or fail with a recovery message; it must never merge old and
    new staging contents implicitly

- `REQ-CORE-RESTORE-ATOMIC-003` Stale restore-progress markers must have a fixed
  diagnostics contract.

  Required behavior:
  - `atm doctor` must report stale `.restore-in-progress` markers as warnings
  - the finding must not become a blocking error by default
  - the finding must include recovery guidance telling the operator to rerun
    `atm teams restore` or remove the marker after manual verification

### 20.4 Error Display And Diagnostics

- `REQ-CORE-ERROR-DISPLAY-001` `AtmError::Display` must remain concise and
  must not emit multi-KB backtrace output.

  Required behavior:
  - `Display` renders the human-readable message and recovery text only
  - captured backtraces remain available via Debug output and a dedicated
    accessor on `AtmError`

- `REQ-CORE-ERROR-DOC-001` Every public function returning `AtmResult` or
  `Result<_, AtmError>` in the explicit Phase M audit inventory must have a
  `# Errors` documentation section.

  Required behavior:
  - the Phase M audit inventory must explicitly include:
    - `mailbox/mod.rs`
    - `mailbox/lock.rs`
    - `read/mod.rs`
    - `ack/mod.rs`
    - `clear/mod.rs`
    - `team_admin.rs`
    - `doctor/mod.rs`
    - `error.rs`
    - `config/mod.rs`
    - `home.rs`
    - `send/mod.rs`
    - `send/input.rs`
    - `send/file_policy.rs`
    - `identity/mod.rs` if the consolidation lands there
    - any new public atomic/state helper introduced by Phase M
  - each `# Errors` section must list the `AtmErrorCode` variants the function
    can return
  - the implementation must audit the current public API surface instead of
    relying on a stale hard-coded function count

- `REQ-CORE-ERROR-RECOVERY-001` Every `AtmError` construction site in the
  explicit Phase M audit inventory that represents an operator-actionable
  failure must use `.with_recovery()`.

  Required behavior:
  - Phase M must perform a grep-driven audit of remaining bare
    `AtmError::new(...)`, `AtmError::mailbox_*`, `AtmError::file_policy(...)`,
    and similar operator-actionable construction sites in the explicit Phase M
    audit inventory
  - the audit must explicitly include bare operator-actionable sites in:
    - `mailbox/mod.rs`
    - `mailbox/lock.rs`
    - `read/mod.rs`
    - `ack/mod.rs`
    - `clear/mod.rs`
    - `team_admin.rs`
    - `doctor/mod.rs`
    - `config/mod.rs`
    - `home.rs`
    - `address.rs`
    - `send/mod.rs`
    - `send/input.rs`
    - `send/file_policy.rs`
    - `identity/mod.rs` if new operator-facing errors are introduced there
    - any new M.1/M.2 helper that constructs `AtmError`
  - permission, timeout, missing-file, malformed-input, lock-contention, and
    operator-remediable configuration failures are always considered
    operator-actionable for this audit
  - sites already covered by L.7/L.8 recovery work do not need duplicate edits
  - internal invariant violations do not require recovery guidance

### 20.5 Code Consolidation And Documentation

- `REQ-CORE-IDENTITY-CONSOLIDATE-001` The duplicated `resolve_actor_identity`
  function must be consolidated into a single shared implementation.

  Required behavior:
  - the identical helper currently present in `ack/mod.rs`, `clear/mod.rs`, and
    `read/mod.rs` must be moved to `identity/mod.rs` as `pub(crate)`

- `REQ-CORE-CONFIG-DOC-001` The deprecated `[atm].identity` config key must be
  documented in a `# Deprecated` section in the config module documentation.

  Required behavior:
  - migration guidance: use `ATM_IDENTITY` environment variable instead
  - reference `ATM_WARNING_IDENTITY_DRIFT` error code

- `REQ-CORE-PANIC-DOC-001` The panic path in `normalize_json_number` must be
  eliminated and documented.

  Required behavior:
  - `normalize_json_number(...)` must return the raw input string on exponent
    parse failure or unsupported exponent range instead of panicking
  - a library function must not panic on potentially untrusted input

## 21. Phase Q: SQLite Mail SSOT, Runtime Boundaries, And Lock Elimination

Phase Q supersedes the mailbox-lock line as the target architecture for ATM
mail correctness. The `REQ-CORE-MAILBOX-LOCK-*` requirements remain
transitional compatibility constraints only for the interim file-based line.
The release-complete target is elimination of mailbox-lock dependence from ATM
mail correctness.

### 21.1 SQLite Mail And Roster Ownership

- `REQ-CORE-RUNTIME-001` ATM mail and team roster state must move to SQLite as
  the authoritative source of truth.

  Required behavior:
  - SQLite is the durable source of truth for:
    - message records
    - read/unread state
    - ack-required / acknowledged state
    - clear/visibility state
    - task linkage and task metadata
    - team roster
    - current per-member pid
  - Claude-owned inbox JSONL files are compatibility ingress/export surfaces,
    not ATM's authoritative durable mail store
  - `config.json` becomes a roster-ingress source, not the durable roster truth

- `REQ-CORE-STORE-001` The Phase Q SQLite store must use one documented schema
  contract with stable keys, constraints, and indexes.

  Required behavior:
  - the authoritative schema must define at least:
    - `messages`
    - `ack_state`
    - `message_visibility`
    - `tasks`
    - `team_roster`
    - `inbox_ingest`
  - `message_key` is the canonical ATM durable message identity
  - `message_key` format must be deterministic and typed by source family:
    - `atm:<ulid>` for ATM-authored durable rows
    - `ext:<fingerprint>` for imported external rows without ATM ids
  - schema constraints must forbid duplicate authoritative identities
  - the schema must document the required lookup indexes for message lookup,
    task lookup, visibility projection, and ingest dedupe
  - ATM-authored message identity is immutable; the same ATM-authored
    `message_id` / `message_key` must never be reused for a different durable
    message
  - an attempted durable insert of an already-known immutable ATM message must
    return a typed duplicate result/error such as `DuplicateEntry`

- `REQ-CORE-STORE-002` The Phase Q SQLite store must enforce WAL and explicit
  transaction policy.

  Required behavior:
  - `journal_mode = WAL`
  - `foreign_keys = ON`
  - mutating ATM command flows must use explicit transactions
  - no production mutating path may rely on implicit per-statement autocommit
    as the normal correctness model

- `REQ-CORE-INGEST-001` Inbox/config ingest must use one owned contract for
  replay, backpressure, and degradation.

  Required behavior:
  - ingest must be idempotent
  - parseable external rows must not be silently dropped
  - malformed external rows must emit structured diagnostics rather than panic
  - backlog/slow-ingest conditions must surface through structured diagnostics
    or health findings rather than dropping records silently
  - roster/config ingest must apply one deterministic last-write-wins policy
    for replacing roster truth in SQLite

- `REQ-CORE-RUNTIME-003` Crash recovery and replay must preserve the durable
  ordering rule for daemon-managed export work.

  Required behavior:
  - the ordering rule is `SQLite commit -> export / remote handoff`
  - re-export/replay must be keyed by durable `message_key`
  - if daemon-managed retry/re-export state survives crash, it must be stored
    durably with a bounded expiry/deadline
  - persisted retry state must not become a long-lived remote outbox

- `REQ-CORE-RUNTIME-002` Live agent status must not use SQLite as its
  authoritative live truth.

  Required behavior:
  - live status is runtime-owned daemon state
  - SQLite stores the current durable `pid` for each member, and daemon memory
    caches it as the primary liveness field
  - daemon runtime state must include `last_active_at` for each known active
    agent/member entry
  - SQLite must not own live `last_active_at`; it remains daemon-memory-only
    runtime state
  - the runtime-managed member fields (`pid`, `last_active_at`, `state`) must
    update only through one documented heartbeat socket handler, as defined in
    `docs/team-member-state.md`
  - for Claude hook-driven sessions, the current pid capture mechanism must use
    the stable parent agent PID (`current-thread.parent.pid` / hook
    `os.getppid()` equivalent), not the short-lived hook subprocess PID
  - for Codex-style sessions without Claude hooks, the current pid capture
    mechanism must use the agent process PID itself
  - until `schooks 1.0` is released, the installed Python hooks from
    `../agent-team-mail` are the supported interim mechanism for sourcing the
    Claude-side pid/activity heartbeat inputs
  - once `schooks 1.0` is released, `schooks` becomes the controlled hook
    environment layer and must report pid/activity updates to `atm-daemon`
  - `atm-daemon` remains the owner of live `last_active_at` and state truth
    even after `schooks` takes over environment control
  - if a heartbeat reports a new pid while the stored old pid is still alive,
    the daemon must reject the new pid unless the explicit admin takeover path
    documented in `docs/team-member-state.md` is active
  - a successful pid replacement must update SQLite and emit
    `AgentPidChanged`
  - runtime member-state transitions must follow the minimal transition model
    in `docs/team-member-state.md`
  - illegal runtime member-state transitions must be prevented by one closed
    transition API and encoded with typestate or equivalent compile-time
    structure where practical (`RBP-002`)
  - `atm read` may consume daemon-owned live-status overlays such as
    `active 3 seconds ago` or `idle for 30 minutes`, but this does not make
    daemon-owned inbox-read logic mandatory
  - SQLite may store a diagnostic or last-observed snapshot only
  - roster truth and live-status truth must remain distinct
  - all allowed update paths for durable roster fields and runtime-only
    team-member fields must be listed in `docs/team-member-state.md`; no other
    update paths are permitted

- `REQ-CORE-RUNTIME-004` Canonical system events must be emitted only on the
  daemon side of the durable store boundary.

  Required behavior:
  - CLI, transport, watcher/reconcile, ingress adapters, and SQLite/store code
    must not fire system events or external post-send hooks directly
  - those layers may only submit work, return typed results, or emit
    diagnostics
  - one daemon-owned core service boundary interprets durable write results and
    determines whether an event-producing transition occurred
  - internal daemon-local hook sites may exist for logging, testing, and
    notifier fanout, but the external post-send hook remains a single canonical
    downstream effect of the daemon-owned event boundary

### 21.2 Singleton Daemon Runtime

- `REQ-CORE-DAEMON-001` ATM must run exactly one daemon per host in the Phase Q
  runtime.

  Required behavior:
  - it must be impossible for two active ATM daemons to run on one host at the
    same time
  - daemon startup must fail deterministically when a live daemon already owns
    the host runtime
  - stale daemon ownership artifacts may be cleaned up only when they are
    proven stale
  - stale cleanup must never allow two live daemons

- `REQ-CORE-DAEMON-002` The daemon must be a thin runtime wrapper rather than a
  unique business-logic layer.

  Required behavior:
  - daemon responsibility is limited to runtime orchestration such as:
    - transport listeners
    - route selection
    - live-status cache
    - watch/reconcile loop
  - the daemon must not become the only place where ATM mail semantics are
    implemented

- `REQ-CORE-DAEMON-003` Production ATM commands must connect to the active
  daemon, auto-start it when absent, and fail clearly only when connect/start
  cannot succeed.

  Required behavior:
  - production CLI/runtime calls must first attempt to connect to the active
    daemon
  - if the daemon is unavailable, ATM must attempt one bounded background
    daemon start for the active team and then retry the connection once
  - if connect/start still fails, ATM must fail with a clear recovery message
    rather than silently falling back to direct SQLite or inbox-file access
  - in-process test harnesses may bypass the daemon only inside explicit test
    wiring, not in the production path
  - `atm send` must route through the daemon in the Phase Q production path
  - `atm ack` must route through the daemon in the Phase Q production path
  - `atm clear` must route through the daemon in the Phase Q production path
  - `atm read` may request daemon-supplied live overlays, but correct mailbox
    projection must not depend on daemon-owned inbox-read logic

  Satisfies:
  - `REQ-P-RUNTIME-001`

- `REQ-CORE-DAEMON-004` The daemon must implement one documented graceful
  shutdown and signal-handling contract.

  Required behavior:
  - required config must validate before listeners bind; invalid config fails
    deterministically with typed startup diagnostics
  - `SIGINT` and `SIGTERM` begin graceful shutdown
  - `SIGHUP` triggers bounded runtime rescan/reload without releasing singleton
    ownership
  - signal handlers install before listeners begin accepting
  - graceful shutdown must stop accepts, drain inflight work, checkpoint WAL,
    and release singleton ownership in order

### 21.3 Strict I/O Ownership Boundaries

- `REQ-CORE-BOUNDARY-001` Every subsystem must be behind a strict trait
  boundary for all external I/O.

  Required behavior:
  - only the owning store subsystem may touch SQLite
  - only the owning inbox ingress/export subsystem may parse or write inbox
    JSONL
  - only the owning config-ingress subsystem may parse team `config.json`
  - only the owning watcher/reconcile subsystem may consume filesystem watch
    events or drive watch-triggered rescan/reconcile logic
  - only the owning transport subsystem may touch sockets
  - only the owning notifier/plugin subsystem may talk to agent processes
  - no business logic may live in I/O adapter code
  - no "just this one call site" bypasses are allowed
  - I/O-owning boundary traits are sealed by default; opening a boundary for
    external implementation requires explicit architectural approval
  - concrete I/O adapter types and constructors remain private unless a
    documented boundary contract requires wider visibility
  - violation of any ownership rule above is a direct QA failure for the Phase
    Q implementation line

### 21.3.1 Structured Error Boundaries

- `REQ-CORE-BOUNDARY-002` Production Phase Q code must model fallible runtime
  behavior with discriminated error unions and explicit `Result` propagation.

  Required behavior:
  - fallible production paths must prefer typed error enums/unions over panic,
    `unwrap`, or `expect`
  - compile-time-visible error types must remain the primary enforcement
    mechanism for runtime failure handling
  - panic is reserved for invariant corruption or explicitly unreachable code
    paths, not routine I/O, parse, transport, or store failures
  - CLI, daemon, and core service layers must preserve structured error
    identity when translating between boundaries
  - the `AtmErrorCode` registry must not use wildcard or catch-all variants in
    place of specific codes
  - every public `AtmErrorCode` must document one recoverability class
  - every public `AtmErrorCode` must document one docs link and stable
    cause/recovery contract in the central registry
  - the `AtmErrorCode` registry is centralized and read-only from the
    perspective of feature/service code; subsystems consume codes from the
    registry and do not mint local alternatives
  - violation of these structured-error rules is a direct QA failure for the
    Phase Q implementation line

### 21.4 Transport And Routing Model

- `REQ-CORE-TRANSPORT-001` ATM must use one logical daemon API with two
  production transport implementations and one test transport.

  Required behavior:
  - same-host transport: Unix domain socket
  - cross-host transport: TCP/TLS
  - test transport: in-process `test-socket` implementation of the same
    protocol/interface for subsystem and daemon-boundary tests
  - these are implementations of one protocol/interface, not separate systems
  - socket receive logic must remain a small framed-message loop that:
    - reads one request frame
    - parses it into a qualified request enum/value
    - dispatches immediately to the owning handler boundary
    - returns a typed response
  - request-kind routing must live behind one dispatcher boundary with
    injectable typed handlers for request families
  - adding a new request family must not require embedding business logic into
    Unix-domain or TCP/TLS transport adapters
  - socket receive logic must not perform SQL, watcher, or notification
    business logic inline
  - any violation of this transport isolation rule is a direct QA failure for
    the Phase Q implementation line
  - subsystem and runtime tests must be able to replace Unix/TCP transport
    adapters with the `test-socket` transport without changing business logic

- `REQ-CORE-TRANSPORT-001B` Request routing must live behind one explicit
  dispatcher boundary with injectable typed handlers.

  Required behavior:
  - transport adapters hand parsed qualified requests to the dispatcher
  - the dispatcher owns request-kind routing only
  - concrete request-family behavior lives in injectable handlers behind the
    dispatcher
  - adding a new request family must not require transport-adapter logic
    growth beyond decode + dispatch
  - any violation of this dispatcher/handler rule is a direct QA failure for
    the Phase Q implementation line

- `REQ-CORE-TRANSPORT-001A` Filesystem watch/reconcile logic must remain a
  separate owned subsystem from transport, store, and notifier logic.

  Required behavior:
  - watch event ingestion, debounce, and reconcile triggering must stay behind
    one owned watcher/reconcile boundary
  - the watcher boundary may request work from ingress/store/notifier
    handlers, but it must not inline SQL, socket, or notification delivery
    logic
  - the transport boundary must not absorb watcher responsibilities
  - any violation of this watcher isolation rule is a direct QA failure for
    the Phase Q implementation line

- `REQ-CORE-TRANSPORT-002` Cross-host traffic must be daemon-to-daemon only.

  Required behavior:
  - native agent/plugin code talks only to the local daemon
  - cross-host delivery happens only between daemons
  - remote routing uses an address form equivalent to `agent@team.host`
  - sender-side daemons must not write remote host inbox JSONL directly

- `REQ-CORE-TRANSPORT-003` Remote delivery must not leave durable long-lived
  pending messages behind when a host is unreachable.

  Required behavior:
  - bounded transient retry is allowed for short intermittent failures
  - after the bounded retry window expires, the send fails
  - ATM must not keep a durable remote outbox that can leave stale messages
    queued for days

- `REQ-CORE-TRANSPORT-004` Remote send success must require remote daemon
  acceptance within the bounded retry window.

  Required behavior:
  - sender-side daemons may record observability/audit information locally
    while attempting remote delivery
  - a remote send must not be reported as successfully delivered until the
    remote daemon accepts it
  - if the bounded retry window expires without remote acceptance, the send
    fails and must not leave durable delivered-message state behind

- `REQ-CORE-TRANSPORT-005` The daemon runtime must use concrete timeout and
  capacity limits for transport/store/health operations.

  Required behavior:
  - same-host daemon request deadline: `3s`
  - per-leg TCP/TLS connect deadline: `5s`
  - per-leg TCP/TLS read/write deadline: `5s`
  - total remote retry budget: `30s`
  - SQLite `busy_timeout`: `1500ms`
  - ingest batch processing slice: `2s`
  - doctor health query deadline: `3s`
  - max concurrent accepts: `64`
  - max per-connection inflight requests: `32`
  - ingest queue depth: `1024`
  - retry queue depth: `256`
  - SQLite handle budget: `1..=4`
  - live status-cache cap: `4096`
  - accept-cap saturation sheds new accepts with typed over-capacity error
  - per-connection inflight saturation sheds excess requests for that
    connection with typed over-capacity error
  - ingest-queue saturation degrades with structured backlog/health reporting
    and fails the enqueue; it never silently drops
  - retry-queue saturation sheds the retry attempt with typed
    remote-delivery saturation error rather than growing unboundedly
  - no async dispatcher, accept loop, watcher, notifier, or health-query path
    may perform blocking SQLite calls inline; direct SQLite work must stay
    behind `spawn_blocking` or a dedicated blocking pool owned by the store
    adapter

### 21.5 Claude Compatibility And Native Agent Path

- `REQ-CORE-COMPAT-001` Claude inbox JSONL remains the required compatibility
  path for Claude context injection.

  Required behavior:
  - ATM-authored Claude inbox exports must remain Claude-native at the top
    level with ATM machine fields under `metadata.atm`
  - Claude-native external writes must be importable into SQLite through one
    owned ingress boundary
  - once team roster and pane mapping truth move to SQLite, ATM-owned
    post-send-hook payloads must carry the authoritative `recipient_pane_id`
    from roster truth when known
  - post-send hooks must be able to rely on that payload field instead of
    rediscovering pane mappings from local files once the Phase Q migration is
    complete

- `REQ-CORE-COMPAT-002` Native agent/plugin traffic must not use JSONL.

  Required behavior:
  - native agent/plugin delivery and notification uses the daemon API instead
    of JSONL
  - the later agent plugin crate must align to this daemon API rather than
    introducing a parallel message transport

- `REQ-CORE-COMPAT-003` The post-send hook must execute on the daemon side of
  the durable store boundary in the Phase Q production runtime.

  Required behavior:
  - the external post-send hook fires only after a successful new SSOT insert
    for a hook-eligible locally-originated outbound message
  - post-send-hook execution is daemon-owned downstream behavior, not store
    behavior and not CLI behavior
  - hook execution must never roll back a durable message commit
  - hook failure remains a typed best-effort warning/diagnostic outcome
  - the daemon-owned hook payload must include whether the outbound message is
    an ack reply (`is_ack`)

- `REQ-CORE-COMPAT-004` Hook eligibility must be explicit and origin-based.

  Required behavior:
  - hook-eligible origins are:
    - locally-originated `atm send`
    - locally-originated `atm ack`
    - later locally-originated daemon/plugin outbound send operations
  - non-hook-eligible origins are:
    - imported inbound Claude/legacy JSONL rows
    - remote inbound daemon deliveries
    - replay/re-export/reconcile paths
    - duplicate durable insert attempts rejected as `DuplicateEntry`
  - one successful eligible durable insert may fire the external post-send hook
    at most once

### 21.6 Lock Elimination Target

- `REQ-CORE-LOCK-RETIRE-001` ATM mail correctness must stop depending on
  mailbox lock artifacts.

  Required behavior:
  - mailbox locks may remain only as transitional compatibility machinery for
    the interim file-based line
  - the Phase Q target architecture must eliminate mailbox-lock dependence from
    normal ATM mail correctness
  - Phase Q completion requires that stale lock artifacts can no longer wedge
    normal ATM mail flows

### 21.7 Test Strategy Constraints

- `REQ-CORE-TEST-RUNTIME-001` Core Phase Q behavior must be testable without
  daemon process spawning.

  Required behavior:
  - daemon spawning is not part of the core test strategy
  - core service behavior must be testable in-process
  - transport/watch/runtime logic must be testable with fakes or in-process
    harnesses
  - no default test path may depend on daemon process lifecycle to validate ATM
    mail correctness

### 21.8 Observability Requirements

- `REQ-CORE-OBS-002` Phase Q must keep structured observability first-class at
  both CLI and daemon boundaries.

  Required behavior:
  - CLI entry, daemon runtime, transport, ingest/export, and service
    orchestration must emit structured events through the shared
    `sc-observability` boundary
  - observability wiring must remain layered:
    - `atm` owns CLI bootstrap and presentation concerns
    - `atm-daemon` owns daemon/runtime event emission
    - `atm-core` owns ATM event and error models above the shared boundary
    - native plugins may emit plugin-local diagnostics, but daemon-owned
      runtime/transport/store/ingest events must be emitted by the daemon and
      not delegated to plugin code
  - observability must not be implemented as ad hoc println/debug output in
    production paths

### 21.8.1 Doctor Health Interface

- `REQ-CORE-DOCTOR-002` The Phase Q runtime must expose a daemon health query
  interface consumable by `atm doctor`.

  Required behavior:
  - `atm doctor` remains a CLI command
  - daemon/runtime health information must be obtained through an explicit
    daemon-facing interface rather than direct CLI inspection of private daemon
    state
  - daemon reachability (liveness) and daemon readiness must remain distinct
    health answers
  - the health interface must be able to report at least:
    - daemon reachability
    - daemon readiness
    - singleton ownership status
    - live status-cache summary
    - ingest backlog / degraded-ingest state when present
    - queue-depth / backlog metrics needed to diagnose readiness pressure
    - SQLite open/readiness state

### 21.9 QA Invariants

- `REQ-CORE-QA-RUNTIME-001` Every QA pass for Phase Q must verify the daemon
  and boundary invariants.

  Required behavior:
  - impossible to run two active ATM daemons on one host
  - daemon unavailability uses only the documented connect/start path and fails
    clearly if that path cannot succeed, with no hidden direct I/O fallback
  - every subsystem performs external I/O only through its owning trait
    boundary
  - production error handling uses typed `Result`/error-enum boundaries instead
    of panic/unwrap for fallible runtime paths
  - daemon/runtime code remains thin and does not accumulate business logic
  - daemon spawning is not the test strategy
  - `atm send`, `atm ack`, and `atm clear` use the daemon production path
  - `atm read` uses daemon participation only for explicitly requested live
    overlays or other documented runtime-only data
  - canonical system events fire only from the daemon-owned post-store
    transition boundary
  - duplicate durable insert attempts do not create duplicate events or
    post-send-hook execution
  - no async dispatcher, accept loop, watcher, notifier, or health-query path
    performs blocking SQLite calls inline
  - SQLite remains the source of truth for mail and roster
  - live agent status remains runtime-owned state
  - daemon health reporting distinguishes liveness from readiness and exposes
    queue-depth/backlog metrics
  - structured `sc-observability` coverage remains present at both CLI and
    daemon layers
  - Claude compatibility export remains Claude-native top-level plus
    `metadata.atm`
