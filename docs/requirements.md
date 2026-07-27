# ATM CLI Requirements

## 1. Product Definition

Product requirement ID:
- `REQ-P-PRODUCT-001` The retained ATM product surface consists of
  `send`, `list`, `read`, `ack`, `clear`, `log`, `doctor`, `teams`, and
  `members`, backed by a singleton daemon runtime and SQLite source-of-truth
  for mail and roster state in the current daemon architecture.

Satisfied by:
- intentionally undecomposed product requirement; this governs overall retained
  product scope rather than a single crate-local obligation

The product is a local command-line tool named `atm`.

The current target architecture uses a tightly-bounded singleton daemon runtime
for same-host local IPC, mail routing, and native-agent notification. ATM
command behavior remains the user-facing surface. The prior custom cross-host
transport is superseded; Phase AI defines the replacement HTTPS/TCP adapter.

Phase-AA simplification direction:
- the daemon remains part of the product, but it must return to the original
  thin-router role
- concrete SQLite construction moves to a dedicated `atm-runtime` crate
- `AA.2` lands that composition root and moves production runtime/store
  assembly there; later AA sprints finish the doctor split, delete remaining
  daemon SQLite leaks, and relock the boundary permanently
- the daemon must not know that the current durable adapter is SQLite
- direct local diagnostics such as store-openability and baseline SQLite
  health may be answered without routing through the daemon
- the supported 1.2 SQLite bootstrap/migration contract is the current
  `message_id`-based durable schema only; abandoned pre-production identity
  scaffolding such as `legacy_message_id` is not an accepted runtime shape
- no repair path is offered for abandoned pre-production databases carrying
  only a `legacy_message_id` column; operators with such databases must
  discard them and allow ATM to initialize a fresh schema
- `atm doctor` now uses that direct local path for config/store diagnostics
  through `atm-runtime`; daemon routing remains only for daemon-owned runtime
  state
- each subsystem owns its own diagnostic trait and backend-specific diagnosis
- top-level doctor code aggregates subsystem findings and daemon-owned runtime
  state, but must not reimplement backend-specific diagnosis logic
- `MailStore` and `RosterStore` remain the primary storage-neutral capability
  traits during this simplification line
- `MailStoreDoctor`, `RosterStoreDoctor`, and `ConfigDoctor` are the explicit
  subsystem doctor traits used by that aggregate-only health model
- later SQLite-backed and Claude-JSON-backed implementations may satisfy that
  same behavior-named trait family rather than forcing backend-shaped parallel
  trait trees

Phase-AC supersession note:
- the Phase `AA` simplification-line store traits above are transitional only
- Phase `AC` replaces `MailStore` with canonical `MessageStore`, converges the
  roster seam into `RosterStore`, and leaves task storage out of scope for the
  initial shared `atm-storage` contract
- speculative task-store code was deleted in `AC.6` rather than preserved as a
  compatibility line
- if task storage is approved later, the first canonical implementation starts
  from Claude-code task schema plus Pydantic validation; SQLite may only sync
  to that canonical model afterward
- after Phase `AC`, `MailStore` / `RosterStore` are historical transition
  names; a later approved task-storage phase would reintroduce a fresh
  canonical design instead of reviving speculative transition scaffolding

The retained product surface is:
- `atm send`
- `atm list`
- `atm read`
- `atm ack`
- `atm clear`
- `atm log`
- `atm doctor`
- `atm teams`
- `atm members`

Approved additive CLI feature for the Phase `Y` line:
- `atm help`

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
  (historical only; Phase U removed its `metadata.atm` coverage from the
  active compatibility design)
- `sc-observability` schema ownership pointer:
  [`sc-observability-schema.md`](./sc-observability-schema.md)
- ATM-owned error-code registry:
  [`atm-error-codes.md`](./atm-error-codes.md)
- schema enforcement models:
  `tools/schema_models/claude_code_message_schema.py` and
  `tools/schema_models/atm_message_schema.py`
- historical/read-compatibility schema record only:
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
- [`docs/atm-runtime/requirements.md`](./atm-runtime/requirements.md)
- [`docs/atm-runtime/architecture.md`](./atm-runtime/architecture.md)
- [`docs/atm-rusqlite/requirements.md`](./atm-rusqlite/requirements.md)
- [`docs/atm-rusqlite/architecture.md`](./atm-rusqlite/architecture.md)
- [`docs/atm-core/boundaries.md`](./atm-core/boundaries.md)
- [`docs/atm-daemon/boundaries.md`](./atm-daemon/boundaries.md)
- [`docs/atm-runtime/boundaries.md`](./atm-runtime/boundaries.md)
- [`docs/atm-rusqlite/boundaries.md`](./atm-rusqlite/boundaries.md)
- [`docs/atm/boundaries.md`](./atm/boundaries.md)

During the cleanup/restructure phase, product requirements stay here while
crate-local ownership is moved out of this file into the crate directories.

Phase-Q supersession note:
- earlier daemon-free assumptions in this file are historical requirements from
  the prior rewrite line
- for mail/runtime architecture, the current authoritative direction is Section
  21

Phase-R redesign note:
- Phase R hardens the architecture by making crate-local boundary records part
  of the enforceable contract before new implementation work proceeds
- Phase R planning and CI may depend on `sc-lint` as an external tool
  dependency; `sc-lint` is not part of the ATM product surface even when its
  verification model constrains Phase R gates

Phase-AI portability note:
- ATM daemon functionality is first-class on Windows as well as Unix-like hosts
- the authoritative local access contract is Unix HTTP/UDS or loopback TCP and
  Windows loopback TCP; peer access is HTTPS/TCP
- the canonical daemon interface is documented in
  [`./atm-daemon/http-api.md`](./atm-daemon/http-api.md) and ADR-033
- route-specific schemas, HTTP status codes, and the OpenAPI artifact are
  owned by that contract rather than restated piecemeal across product docs
- the current daemon HTTP resource surface covers:
  - `send`
  - `ack` through the send-shaped acknowledge request
  - `read`
  - `clear`
  - `doctor`
  - daemon heartbeat/runtime liveness exchange
- S.5 planning adds `atm list` as a distinct CLI surface; S.7 owns the
  implementation line that refines the current queue-query packet mapping
  instead of assuming the old multi-message `read` response shape is still the
  final product contract
- retained `log`, `teams`, and `members` remain outside the daemon
  request/response packet family in the current Phase S line
- Phase S planning is tracked in [`plan-phase-S.md`](./plan-phase-S.md)
- durable ATM state is one host-scoped SQLite database at
  `~/.atm/db/mail.db`; the daemon is the only writer, while direct read-only
  SQLite consumers remain an allowed system integration path
- the thin-client extension surface should center on `send` and `receive`
  over the shared ATM protocol, while the retained CLI may continue to expose
  `ack` as a user-facing workflow
- Phase U removed the active `metadata.atm` namespace from the approved
  compatibility schema; the authoritative schema is
  [`atm-message-schema.md`](./atm-message-schema.md) and the planning record is
  [`plan-phase-U.md`](./plan-phase-U.md)

## 2. Scope

Product requirement ID:
- `REQ-P-SCOPE-001` The rewrite retains the documented command surface and
  migrates ATM mail/runtime ownership from filesystem JSON plus mailbox locks
  to SQLite plus a singleton daemon without intentionally removing retained
  functionality.

Satisfied by:
- intentionally undecomposed product requirement; this governs overall rewrite
  scope and is enforced across the workspace rather than by one crate-local ID

- `REQ-P-RUNTIME-001` Production ATM commands must connect to the daemon and
  auto-start it when absent.

- `REQ-P-RUNTIME-002` Daemon singleton is ATM daemon requirement `#1`:
  exactly one `atm-daemon` process may exist anywhere on the host for the
  supported runtime model, and no code path may intentionally or accidentally
  allow a second daemon to reach serving state.

- `REQ-P-RUNTIME-003` Daemon singleton enforcement must use multiple guard
  layers:
  - a pre-spawn launch gate that serializes daemon creation attempts
  - a daemon-side startup gate that refuses serving state when ownership is
    already held
  - a static lint/CI gate that rejects test-only or ad hoc daemon launch
    patterns
  No test, tool, or alternate CLI path is exempt from these guards.

  Required behavior across `REQ-P-RUNTIME-001` through
  `REQ-P-RUNTIME-003`:
  - the production CLI/runtime path first attempts to connect to an
    already-running daemon
  - if the daemon is not running, the production CLI/runtime path auto-starts
    it and retries once
  - if daemon auto-start still fails, ATM must fail clearly with recovery
    guidance
  - no production path may silently bypass the daemon by talking directly to
    SQLite or inbox files
  - every daemon launch path is subordinate to `REQ-P-RUNTIME-002` and
    `REQ-P-RUNTIME-003`

- `REQ-P-RUNTIME-004` The supported same-host topology is one OS-user-scoped
  daemon, one OS-user-scoped SQLite database, and one OS-user-scoped retained
  log root serving multiple ATM workspaces with different `ATM_HOME` values.

  Required behavior:
  - distinct workspaces on the same host may carry different `ATM_HOME`
    values while still sharing:
    - one daemon singleton
    - one durable SQLite root
    - one retained observability root
  - concurrent same-host `send`, `read`, and `ack` traffic from different
    workspaces must not leak mailbox, roster, or retained-log state across
    team/workspace boundaries
  - release evidence for this topology must include at least one shared-host
    smoke lane that proves two or more workspaces share one daemon/database/log
    root while concurrent `send` / `read` / `ack` traffic succeeds
  - any release claim that ATM is production-ready for `10+` same-host
    workspaces must cite explicit accepted evidence for that `10+` topology in
    the readiness record; absent that evidence, the release must not claim the
    broader `10+` same-host scale target

- `REQ-P-RUNTIME-005` Same-host daemon timeout handling must preserve an
  explicit retry-safety contract for side-effecting commands.

  Required behavior:
  - the daemon may continue running accepted request work after a caller-side
    local IPC deadline expires
  - read-only commands may continue to use retryable same-host timeout
    failures
  - side-effecting commands that exceed the caller-visible deadline after
    dispatch begins must not return a generic retry-safe timeout surface
  - same-host side-effecting timeout failures must return a distinct
    machine-readable error code that means "the command may have executed"
  - recovery guidance for that distinct timeout must tell callers to inspect
    mailbox or service-side effects before retrying

- `REQ-P-DAEMON-PARTITION-001` Phase R daemon cleanup work must use one
  explicit daemon-private partition map so ownership, review scope, and later
  lint enforcement do not depend on ad hoc file boundaries.

- `REQ-P-DAEMON-LIFECYCLE-001` Daemon lifecycle and singleton teardown rules
  must define a positive safe-order contract:
  - keep stable host-wide lock file paths for `launch.lock` and `owner.lock`
    on every supported operating system
  - clear or invalidate owner-visible ownership metadata while the live
    advisory lock is still held
  - release the live advisory lock only after the owner metadata is no longer
    published as current
  - if cleanup cannot complete safely, fail closed rather than publishing an
    ambiguous ownership state

- `REQ-P-DAEMON-DISPATCHER-001` Request work accepted by the daemon must remain
  tracked by runtime-owned drain accounting until it finishes or is cancelled.
  Detached untracked request execution is forbidden even when the transport
  remains single-request-per-connection.

- `REQ-P-DAEMON-LANES-001` Background daemon lanes must use rollback-safe
  startup and shutdown sequencing:
  - partial start failure must stop every lane already started
  - shutdown must attempt every lane cleanup path before final ownership
    release
  - partial lane failure must not leave the runtime in ambiguous ownership
    state

- `REQ-P-PLATFORM-001` ATM `1.0` supports macOS, Linux, and Windows as
  first-class operating systems for the retained product surface.

- `REQ-P-PLATFORM-002` Feature parity across supported operating systems is a
  release requirement, not a best-effort goal.

  Required behavior:
  - every retained ATM feature required for `1.0` must work on every supported
    operating system
  - daemon functionality must not be considered "supported" on an operating
    system when the result is compile-only support, `daemon_unavailable`
    stubs, or documentation that instructs users to switch operating systems
  - implementation differences by operating system are allowed only behind
    documented product and crate-local boundaries and must preserve the same
    observable product behavior, error semantics, and test obligations
  - a feature that lacks one supported-operating-system implementation is
    incomplete and must not be documented as production-ready

### 2.1 In Scope

- one binary: `atm`
- one primary library: `atm-core`
- SQLite-backed ATM mail source of truth
- SQLite-backed team roster source of truth
- singleton daemon runtime
- Phase AI target daemon API: Unix HTTP over local UDS and loopback TCP,
  Windows HTTP over loopback TCP, and HTTPS over TCP for remote
  peers. AI.1 intentionally retains the pre-migration local IPC baseline; the
  local HTTP target becomes live in AI.6 and the remote HTTPS target in AI.9.
- Claude-compatible JSONL inbox ingress and export
- configuration resolution
- caller identity resolution through explicit CLI override or invoking-shell
  `ATM_IDENTITY`
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
- routine daemon process spawning as a correctness test strategy
- a test-only daemon launch path
- manual daemon-start discipline as a product requirement
  - production CLI auto-start when the daemon is absent is in scope under
    `REQ-P-RUNTIME-001`
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

- `REQ-P-RELEASE-007` ATM release identifiers must be strict SemVer. The
  project supports opt-in prerelease builds such as `1.3.2-beta.1` and
  `1.3.2-alpha.1`; prereleases are never the default customer channel.

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
- automated `winget` release wiring requires a dedicated
  `WINGET_GITHUB_TOKEN` repo secret
- `WINGET_GITHUB_TOKEN` must be a PAT with permission to create branches / PRs
  against the `randlee/winget-pkgs` fork used by the release workflow
- release readiness proof for `winget` must validate successful submission or
  manifest update dispatch; it cannot require same-day installability because
  Microsoft review introduces a normal 1-2 day publication lag
- the normal Homebrew `atm` formula tracks stable releases only; prereleases
  are published, when approved, through an explicit opt-in `atm-beta` formula
  in the project-owned tap

### 2.4 HTTP Compatibility Scope

Product requirement ID:

- `REQ-P-HTTP-COMPAT-001` The daemon HTTP API has a strict, independently
  declared SemVer version. Product release versions identify builds only; they
  are never the CLI-to-daemon compatibility gate.

Required behavior:

- `/v{major}/atm` and the HTTP API's declared SemVer have the same major;
  different major versions fail before a write with a typed compatibility
  error
- the compatibility preflight compares the explicit CLI/daemon schema version
  and HTTP API major, not `atm` or `atm-daemon` product release strings
- an additive endpoint, optional JSON field, response field, or error detail
  increments the HTTP API minor version and must preserve successful
  communication for clients and servers sharing the same major; patch versions
  are corrective only and do not add or remove contract elements
- requests accept omitted additive fields with documented defaults and servers
  ignore unknown additive fields; an operation requiring a new capability must
  declare that requirement rather than relying on a minor-version mismatch
- OpenAPI, generated clients, and compatibility tests are the authoritative
  proof of this contract

## 3. External Contracts

Product requirement ID:
- `REQ-P-CONTRACT-001` External path/config/store/observability contracts must
  match the documented retained ATM behavior for the active architecture line.

Satisfied by:
- `REQ-CORE-CONFIG-001` for home/path/config resolution aspects
- `REQ-CORE-RUNTIME-001` for durable mail/roster store ownership aspects
- `REQ-CORE-INGEST-001` for config ingest and historical Claude inbox ingest
  compatibility aspects
- `REQ-CORE-MAILBOX-001` for persisted mailbox atomicity plus historical Claude
  inbox write/read compatibility aspects
- `REQ-ATM-OBS-001` for CLI observability bootstrap/integration aspects
- `REQ-CORE-OBS-001` for ATM observability boundary/query-model aspects

### 3.1 Home And Path Resolution

Workspace/config resolution order:
1. `ATM_HOME` when set and non-empty
2. OS home directory

Runtime-root rule:
- under planned ADR-026, neither the invocation directory nor `ATM_HOME` is a
  selector for daemon socket, lock, or SQLite durable-state paths; those derive
  from one OS-user `HostRuntimeScope`
- retained logs follow their host-scoped root under ADR-011 (or explicit
  `ATM_LOG_DIR`), independently of workspace `ATM_HOME`
- the invocation directory and `ATM_HOME` remain workspace/config discovery
  inputs only

Required workspace/config paths:
- `{ATM_HOME}/.claude`
- `{ATM_HOME}/.claude/teams`
- `{ATM_HOME}/.claude/teams/{team}`
- `{ATM_HOME}/.claude/teams/{team}/config.json`
- `{ATM_HOME}/.claude/teams/{team}/inboxes/{agent}.json`
- `{ATM_HOME}/.config/atm/config.toml`
- `{ATM_HOME}/.config/atm/state.json`
- `{ATM_HOME}/.config/atm/share/{team}/`

Required host-runtime paths:
- the canonical endpoint, `launch.lock`, and `owner.lock` derive only from
  `HostRuntimeScope.runtime_root`
- the one SQLite durable database derives only from
  `HostRuntimeScope.durable_state_root`
- the retained log file derives only from the ADR-011 host log root unless
  `ATM_LOG_DIR` explicitly overrides it

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

Product requirement ID:

- `REQ-CORE-IDENTITY-CHAT-001` ATM must support an optional chat-id as an
  independent component of a sender or recipient identity.

Required behavior:

- canonical address grammar is `<agent>[:<chat-id>]@<team>[.<host>]`
- `agent[:<chat-id>]` is the canonical agent-identity grammar used wherever a
  full team/host address is not required; therefore `agent:XXX` means agent
  `agent` with chat-id `XXX`
- CLI address composition is equivalent to canonical text: base agent plus
  `--team <team>` yields logical `agent@team`; adding `--chat-id XXX` yields
  logical `agent:XXX@team`, before a single normalization to the structured
  address
- caller chat-id resolution is ordered: qualified `--as <agent>:<chat-id>`,
  then `--chat-id`, then `ATM_CHAT_ID`, then a chat-id embedded in
  `ATM_IDENTITY=<agent>:<chat-id>`, then no chat-id. An explicit unqualified
  `--as <agent>` is a complete caller override and selects no chat-id.
- `agent`, `team`, and `chat-id` use the safe segment alphabet already
  required above; only the address parser interprets `:` and `.` delimiters
- storage keeps nullable source and destination chat-id columns rather than
  concatenating a chat-id into an agent-name column
- reads render a present source chat-id in `from` as `agent:chat-id`; writes,
  nudges, replies, and acknowledgements preserve the full destination address
- `agent` without a chat-id, `agent:chat-a`, and `agent:chat-b` are distinct
  identities for inbox visibility and owner-only mutations
- `atm read --agent <agent>` searches that agent across all chat IDs;
  `atm read --agent <agent> --chat <chat-id>` narrows to one chat identity
- chat-id is not a daemon session, transport-session, or message-thread field
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

Historical shared inbox file-container rule:
- the prior Claude inbox container at
  `{ATM_HOME}/.claude/teams/{team}/inboxes/{agent}.json` used one top-level
  JSON array of inbox messages
- that `.json` array shape is historical-only after `ADR-019`; it is not a
  live production compatibility path for accepted send/read behavior
- any retained repair/rebuild or salvage handling for malformed Claude inbox
  JSON exists only for historical compatibility tooling and must not redefine
  current runtime requirements

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
  - legal ATM additive derivative messages, including historical top-level
    additive compatibility records and tolerated `metadata.atm` derivatives
- no normal ATM runtime/query path may depend on ATM-owned machine-state reads
  from Claude JSON
- no `metadata.atm` namespace may survive in active compatibility output
- retained `message_id` is the ULID text form of the one logical ATM message
  identity
- ATM-owned workflow, delete/close, expiry, sender-projection, and repair
  state must live in SQLite-owned state, not in shared JSON
- write-path validation may reject wrong-format ATM-owned compatibility fields
  with descriptive errors
- read-path validation failure for additive ATM fields must trigger warning +
  degradation logic rather than failing the overall message read
- a separate ATM-native inbox remains deferred; on the earlier compatibility
  line, the shared inbox remained compatibility-only
`REQ-P-SCHEMA-001` is owned by:

- [`claude-code-message-schema.md`](./claude-code-message-schema.md)
- [`atm-message-schema.md`](./atm-message-schema.md)
- [`legacy-atm-message-schema.md`](./legacy-atm-message-schema.md)
  (historical only; its `metadata.atm` coverage was superseded and removed
  from the active compatibility design in Phase U)
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
- if ATM-local semantics need durability independent of compatibility exports,
  that state must live in the ATM-owned SQLite store
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
- default team for config/bootstrap flows that explicitly consume ATM config
  defaults; it is not a runtime caller-team fallback for commands governed by
  the caller-context matrix

Supported optional config fields:
- `[atm].team_members`
- `[atm].aliases`
- `[[atm.post_send_hooks]]`

Runtime caller-context rules:
- repo-local `.atm.toml` `[atm].identity` and the legacy top-level `identity`
  key are not valid runtime identity fallback for the retained multi-agent ATM
  model
- repo-local `.atm.toml` `[atm].default_team` is not a valid runtime caller
  team fallback for commands that require caller context
- the authoritative command-by-command caller-context matrix is
  `docs/requirements.md` §4.1
- runtime identity must come from:
  - explicit command override when supported
  - `ATM_IDENTITY`
- runtime caller chat-id must come from the caller-context precedence in
  `docs/requirements.md` §4.1; `ATM_CHAT_ID` is the environment-level source
  and a qualified `ATM_IDENTITY` is its lower-precedence fallback
- runtime caller team for commands that require it must come from:
  - explicit command override when supported
  - `ATM_TEAM`
- caller-owned CLI commands must resolve identity before daemon dispatch; if no
  valid identity exists, the CLI must fail locally and must not contact the
  daemon
- caller-owned CLI commands must resolve required caller team before daemon
  dispatch; if no valid required caller team exists, the CLI must fail locally
  and must not contact the daemon
- daemon-backed caller-owned request DTOs must carry resolved caller identity
  as required request data
- daemon-backed caller-owned request DTOs must carry resolved caller team as
  required request data when the command requires caller team
- the daemon must not consult hook files, repo-local config, roster state, or
  daemon ambient `ATM_IDENTITY` / `ATM_TEAM` to fill missing caller context
- obsolete config identity fields (`[atm].identity` and legacy top-level
  `identity`) may remain temporarily for migration, but ATM must ignore them
  for runtime identity resolution and `atm doctor` must flag them for removal
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
- malformed current-Claude root arrays must salvage segmentable valid message
  objects and emit explicit degraded warnings whenever localized recovery is
  possible
- malformed root documents or invalid root structure with no segmentable valid
  message units must fail with structured errors rather than guessed repairs
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

## 4. Caller Context Resolution

Product requirement ID:
- `REQ-P-IDENTITY-001` Caller-context resolution must follow the documented
  command precedence rules.

Satisfied by:
- `REQ-CORE-CONFIG-001` for caller-context resolution policy

Caller context means:

- caller identity when the command needs caller identity
- caller team when the command needs caller team

Global caller-context rules:

- repo-local `.atm.toml` `[atm].identity` and legacy top-level `identity` are
  not valid runtime caller identity
- repo-local `.atm.toml` `[atm].default_team` is not valid runtime caller team
  for commands that require explicit caller context
- daemon ambient `ATM_IDENTITY` / `ATM_TEAM` are not valid fallback sources
- roster state, hook files, and target-address fields are not valid caller
  context sources
- if caller context is required and cannot be resolved from the documented
  sources, the CLI must fail locally before daemon dispatch or retained
  command execution
- when both an explicit CLI caller-context override and invoking-shell env are
  present, the explicit CLI override wins
- `atm doctor` is diagnostic and is the explicit exception: it may run without
  caller identity and without caller team

### 4.1 Caller-Context Matrix

| Command | Caller identity required | Caller identity may come from | Caller team required | Caller team may come from | Notes |
| --- | --- | --- | --- | --- | --- |
| `atm send` | Yes | `--as`, else `--chat-id`, else `ATM_CHAT_ID`, else qualified `ATM_IDENTITY`, else `ATM_IDENTITY` | Yes | `--team`, else `ATM_TEAM` | `--as`/`--chat-id` select caller context; target recipient/team are not caller context |
| `atm peek` | Yes | `--as`, else `ATM_IDENTITY` | Yes | `--team`, else `ATM_TEAM` | inspection-only; `--from` remains a sender filter |
| `atm read` | Yes | `--as`, else `--chat-id`, else `ATM_CHAT_ID`, else qualified `ATM_IDENTITY`, else `ATM_IDENTITY` | Yes | `--team`, else `ATM_TEAM` | owner-only mutating read path |
| `atm ack` | Yes | `ATM_CHAT_ID`, else qualified `ATM_IDENTITY`, else `ATM_IDENTITY` | Yes | `--team`, else `ATM_TEAM` | reply target preserves the received full source address |
| `atm list` | Yes | `--as`, else `ATM_IDENTITY` | Yes | `--team`, else `ATM_TEAM` | `--from` is a sender filter, not caller identity |
| `atm clear` | Yes | `ATM_IDENTITY` | Yes | `--team`, else `ATM_TEAM` | owner-only mutating clear path |
| `atm log` | Yes | `ATM_IDENTITY` | Yes | `ATM_TEAM` | no explicit caller override surface |
| `atm members` | Yes | `ATM_IDENTITY` | Yes | `--team`, else `ATM_TEAM` | `--team` scopes the roster being inspected and may also satisfy caller-team requirement |
| `atm teams` | Yes | `ATM_IDENTITY` | Yes | `ATM_TEAM` | no explicit override surface |
| `atm teams add-member` | Yes | `ATM_IDENTITY` | Yes | `ATM_TEAM` | positional `team` is the target roster team, not caller team |
| `atm teams update-member` | Yes | `ATM_IDENTITY` | Yes | `ATM_TEAM` | positional `team` is the target roster team, not caller team |
| `atm teams backup` | Yes | `ATM_IDENTITY` | Yes | `ATM_TEAM` | positional `team` is the backup target, not caller team |
| `atm teams restore` | Yes | `ATM_IDENTITY` | Yes | `ATM_TEAM` | positional `team` is the restore target, not caller team |
| `atm doctor` | No | not required | No | optional `--team` only | diagnostic scope is optional; `ATM_IDENTITY` / `ATM_TEAM` visibility may be reported but are not required inputs |

### 4.2 Command-Specific Notes

- `atm send --as <agent>[:<chat-id>]` is the explicit caller agent/chat
  override; its caller team still comes from `--team` or `ATM_TEAM`
- with ambient `ATM_IDENTITY=<agent>`, `atm send <to> --chat-id <chat-id>` is
  equivalent to `atm send <to> --as <agent>:<chat-id>`; `--chat-id` and
  `--as` are mutually exclusive and failure to resolve the ambient base agent
  fails before daemon dispatch
- with ambient `ATM_IDENTITY=<agent>`, `atm read --chat-id <chat-id>` is
  equivalent to `atm read --as <agent>:<chat-id>`; it resolves the owner
  mailbox before the existing owner-only read path, and the two flags are
  mutually exclusive
- caller chat-id precedence is exactly: `--as` (including its explicit absence
  of chat-id), then `--chat-id`, then `ATM_CHAT_ID`, then a chat-id embedded in
  `ATM_IDENTITY`, then no chat-id. `ATM_CHAT_ID` must be a valid `ChatId` and
  requires `ATM_IDENTITY` to supply the base agent; an invalid value fails
  locally before daemon dispatch.
- on mutating `send` and owner-only `read`, `--as` must use the same base
  agent as `ATM_IDENTITY`; a different base agent is an impersonation attempt
  and fails before daemon dispatch
- a chat-qualified recipient is expressed only in canonical `<to>` syntax:
  `<agent>:<chat-id>@<team>[.<host>]`
- `--from` on `send` is not an accepted caller-identity override
- `--from` on `read` / `list` is a sender filter only
- `--as` is accepted on `send` and `read` for the caller agent/chat override,
  and on inspection-only surfaces such as `peek` and `list`
- any command without an explicit caller override surface must rely on the
  invoking shell when caller context is required
- `atm doctor` may inspect `ATM_IDENTITY` visibility and team override
  behavior, but it must not treat hook files, repo-local config identity/team,
  or daemon ambient identity/team as command caller context

If command identity cannot be determined where required, the CLI must fail with
a structured recovery-oriented error before daemon dispatch. An obsolete config
`identity` field may be reported as a diagnostic, but it does not count as
command identity.

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
  `from` field only when ATM also stores the canonical sender identity in
  SQLite-owned state for routing, validation, and audit

Post-send-hook rules:
- ATM always has one shipped default post-send path in the installed binary:
  the built-in in-process delivery path
- `[[atm.post_send_hooks]]` is the supported external override shape for
  post-send behavior
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
  - `sender`
  - `recipient`
  - `team`
  - `message_id`
  - `description`
  - `task_id` as a string; it may be empty when no task is associated
  - `requires_ack`
  - `is_ack`
  - optional `to` for compatibility
  - optional `recipient_pane_id` when ATM has an authoritative pane mapping for
    the recipient
- Current runtime addition: `is_ack` is part of the retained hook payload contract for
  the daemon-owned send/ack runtime path so hook implementations can
  distinguish `atm send` from `atm ack` without inspecting message text
- any retained built-in `atm internal-nudge` helper must not reuse
  `ATM_POST_SEND` as its control contract; it consumes a separate resolved
  `ATM_INTERNAL_NUDGE` envelope carrying the canonical event, sink target,
  resolved template kind, and resolved template body or explicit disabled
  state
- the post-send hook must run after successful non-`dry-run` `atm send`
- the post-send hook must also run after successful `atm ack`, using the
  reply message as the hook subject
- `is_ack` must be `false` for `atm send` and `true` for `atm ack`
- hook configuration lookup must use the sender's authoritative ATM roster
  `home_dir` metadata rather than the caller's live process working directory
- if no matching external `[[atm.post_send_hooks]]` rule is configured, ATM
  must still attempt the shipped built-in in-process post-send path
- the built-in shipped nudge path must support exactly six named template
  cases:
  - `delivery`
  - `delivery_ack`
  - `delivery_task`
  - `delivery_task_ack`
  - `acknowledge`
  - `acknowledge_task`
- the default built-in acknowledge nudge shapes are intentionally compact:
  - `<atm kind="ack" from="..." message-id="..."/>`
  - `<atm kind="ack" from="..." message-id="..." task-id="..."/>`
- teams may override any subset of those six built-in template bodies through
  host-scoped, team-keyed ATM-managed override rows resolved through the
  storage-neutral `NudgeTemplateOverrideStore` contract
- built-in precedence is:
  - matching external `[[atm.post_send_hooks]]` command
  - resolved team override row for the selected template kind
  - built-in product default template body for that kind when no row exists
- template lifecycle is explicit:
  - no row => product default
  - override row => stored non-empty template body
  - disabled row => no built-in nudge emission
  - clear/reset => row deletion back to product default
- empty-string template bodies are invalid and must not be used as a hidden
  disable signal
- example payload:
  ```json
  {
    "from": "arch-ctm@atm-dev",
    "sender": "arch-ctm",
    "recipient": "recipient",
    "team": "atm-dev",
    "message_id": "...",
    "description": "review failing smoke lane",
    "task_id": "",
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
- if sender identity cannot be resolved from `--from` or invoking-shell
  `ATM_IDENTITY`, fail before daemon dispatch
- resolve recipient address using the defined precedence
- resolve aliases before mailbox lookup
- when a cross-team alias-oriented sender is projected into `from`, also
  persist the canonical sender identity in SQLite-owned state and use it for
  validation, self-send checks, routing, and audit behavior
- reject canonical same-team self-addressed sends before any persistence or
  `--dry-run` success reporting only when the resolved destination has no
  host; every syntactically valid host-qualified destination continues to the
  ordinary host-routing contract
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
- if no matching external rule exists, execute the built-in in-process
  post-send path instead of silently skipping post-send emission
- support an optional structured hook result on stdout so hook scripts can
  report post-send outcomes such as nudges, no-op conditions, and operator
  errors without relying on stderr scraping
- emit structured diagnostics for hook-rule evaluation and actionable warnings
  only when a configured hook execution fails
- if a configured recipient exposes post-send behavior and no emission occurs,
  ATM must either emit the post-send effect or surface a sender-visible warning
- treat `post_send_hook` failure or timeout as best-effort diagnostics only; it
  must not roll back or fail an already-successful send
- write a non-null `message_id` on every ATM-authored message
- `message_id` is the retained ULID form of the one logical ATM message
  identity

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

## 7. Queue Inspection Surfaces (`atm list`, `atm peek`, and `atm read`)

Product requirement IDs:
- `REQ-P-LIST-001` `atm list` must satisfy the bounded queue/search contract.
- `REQ-P-READ-001` `atm read` must satisfy the documented single-message
  selection, mutation, and wait contract.

Satisfied by:
- `REQ-ATM-CMD-001` for CLI entry, parsing, and dispatch aspects
- `REQ-ATM-OUT-001` for human-readable and JSON output aspects
- `REQ-CORE-CONFIG-002` for target-validation aspects
- `REQ-CORE-LIST-001` for bounded metadata search, row shaping, and shared
  filter semantics
- `REQ-CORE-MAILBOX-001` for merged inbox load/persist aspects
- `REQ-CORE-WORKFLOW-001` for classification, queue selection, and legal
  transition aspects

### 7.1 Shared Purpose

Queue inspection is split into three commands:

- `atm list` finds messages without returning full message bodies
- `atm peek` opens one full message without mutation
- `atm read` opens one full message with owner-only mutation

The split exists so ATM can keep default queue inspection bounded even when
SQLite-backed mailbox history grows without a practical fixed upper bound.

### 7.2 Shared Selection And Filter Contract

Shared queue filters:
- optional target: `agent` or `agent@team`
- `--team <name>`
- `--from <name>`
- `--since <iso8601>`
- `--task <task-id>`
- `--contains <text>`
- `--unread`
- `--pending-ack`
- `--all`

Shared command options:
- `--json`

Inspection-only command option:
- `--as <name>` on `atm list` and `atm peek`

Shared rules:
- all three commands must default to the caller's own inbox when no target
  agent is provided
- all three commands must resolve identity and target address using the
  defined precedence
- `atm list` and `atm peek` may resolve caller identity from `--as`, else the
  invoking-shell `ATM_IDENTITY`
- `atm read` resolves caller identity from `--as`, else `--chat-id`, else
  `ATM_CHAT_ID`, else qualified invoking-shell `ATM_IDENTITY`, else invoking-
  shell `ATM_IDENTITY`; its
  existing owner-only mutation rule applies to the resolved full identity
- if required caller identity cannot be resolved from the documented source
  for that command, fail before daemon dispatch
- `--as <name>` changes caller identity resolution on `send`, owner-only
  `read`, and inspection-only commands, never message matching; mutating
  commands require the base-agent match specified in the command matrix
- `--json` changes output format only and is not a message-selection filter
- all three commands must verify target team exists
- `atm list` and `atm peek` must verify an explicit target agent exists in the
  team config before inspection proceeds
- `atm read` must reject explicit cross-agent mailbox targets on the mutating
  path and may only operate on the caller's own mailbox
- all three commands must support the same semantic message filters even when
  their output shapes differ
- `--contains` must search both summary text and full message body text
- on metadata-backed read/list/peek paths, `--contains` must stay full-body
  correct without widening the bounded metadata query into an eager full-body
  scan
- all three commands must preserve origin-inbox visibility when bridge remotes
  are configured

Legacy `atm read` flag migration:
- `--unread-only` is a deprecated alias for `--unread`
- `--pending-ack-only` is a deprecated alias for `--pending-ack`
- `--history` is a deprecated alias for `--all`
- `--since-last-seen` remains accepted as an explicit restatement of the
  default seen-state filter
- deprecation warnings must direct operators to the new flag names

### 7.3 `atm list`

Additional supported flags:
- `--limit <n>`

Required behavior:
- load the mailbox/query surface through a bounded metadata-first query path
- query logical current messages rather than superseded predecessors
- return compact rows only, not full message bodies
- when `--contains` is present, apply sender/timestamp/task and logical-current
  filtering on bounded metadata rows first, then reload durable body text only
  for surviving summary-miss candidates that still need a body check
- support the canonical row fields:
  - `message_id`
  - `summary`
  - `from`
  - `timestamp`
  - `read`
  - `pending_ack`
  - `task_id`, with `null` in JSON output when the logical message is not
    task-linked
- sort newest-first before limiting
- compute bucket/count summaries through bounded summary queries rather than by
  materializing every full message body for operator-facing response shaping
- perform no read-state or ack-state mutation
- keep default output bounded to actionable/head results rather than
  materializing full history by default

### 7.4 `atm peek`

Additional supported flags:
- `--message-id <id>`
- `--timeout <seconds>`
- `--since-last-seen`
- `--no-since-last-seen`
- `--as <name>`

Required behavior:
- return exactly one full message
- perform no read-state, seen-state, or ack-state mutation
- when `--message-id <id>` is present, resolve that exact message when present
- collapse successor/update chains to their terminal node before selector-based
  matching so superseded predecessors do not appear as separate current
  messages
- when `--task <task-id>` is present, find task-linked messages, collapse each
  successor chain to its terminal node, then select the most recent logical
  current message
- when selectors such as `--task`, `--from`, `--since`, `--contains`,
  `--unread`, or `--pending-ack` match multiple messages, return the most
  recent match
- when `--contains` is present on the metadata-backed path, selector
  correctness must be preserved by checking bounded summary text first and
  reloading durable body text only for surviving summary-miss candidates
- when multiple matches exist, include:
  - `selected_message_id`
  - `match_count`
  - `additional_match_count`
- `match_count` is the total number of logical current-message matches after
  all filters and successor-chain collapse are applied
- `additional_match_count` is `match_count - 1` for a successful peek
- when no selector is provided, return the most recent unread actionable
- when no selector is provided, prioritize pending-ack messages ahead of
  unread messages that do not require acknowledgement
- support optional wait mode with timeout

### 7.5 `atm read`

Additional supported flags:
- `--message-id <id>`
- `--timeout <seconds>`
- `--since-last-seen`
- `--no-since-last-seen`

Required behavior:
- return exactly one full message
- mutate owner-visible seen/read state when a message is selected
- when `--message-id <id>` is present, resolve that exact message when present
- collapse successor/update chains to their terminal node before selector-based
  matching so superseded predecessors do not appear as separate current
  messages
- when `--task <task-id>` is present, find task-linked messages, collapse each
  successor chain to its terminal node, then select the most recent logical
  current message
- when selectors such as `--task`, `--from`, `--since`, `--contains`,
  `--unread`, or `--pending-ack` match multiple messages, return the most
  recent match
- when `--contains` is present on the metadata-backed path, selector
  correctness must be preserved by checking bounded summary text first and
  reloading durable body text only for surviving summary-miss candidates
- when multiple matches exist, include:
  - `selected_message_id`
  - `match_count`
  - `additional_match_count`
- `match_count` is the total number of logical current-message matches after
  all filters and successor-chain collapse are applied
- `additional_match_count` is `match_count - 1` for a successful read
- when no selector is provided, return the most recent unread actionable
  message
- when no selector is provided, prioritize pending-ack messages ahead of
  unread messages that do not require acknowledgement
- support optional wait mode with timeout
- write the selected message back through the read-axis mutation rules
- persist read-triggered state changes back to the physical inbox file that
  owns the selected displayed message when origin inbox files are present in
  the merged surface
- when a read-side mutation is applied, the returned `message` payload and
  `selected_message_id` must still refer to that same mutated durable message;
  `atm read` must not mark one message read and then silently swap the output
  payload to a different unread message
- `bucket_counts` in the read outcome must describe the post-mutation mailbox
  state produced by that command execution rather than stale pre-mutation
  counts

### 7.6 Shared Message Classification And Deduplication

- load messages from the merged inbox surface
- deduplicate entries by `message_id` before bucket selection and output
  rendering
- classify each message into the read axis, the ack axis, and a derived
  message class
- map the derived message class into display buckets

When multiple inbox entries share the same non-null `message_id`, queue
inspection must display only the most recent entry. Earlier duplicates are
silently suppressed.

Deduplication order:
- compare entries by `message_id`
- keep the newest entry by message timestamp
- when timestamps are equal, keep the later record encountered in inbox order
- do not emit suppressed duplicates in either human or JSON output

### 7.7 Display Buckets

The shared queue model exposes three display buckets:
- `unread`
- `pending_ack`
- `history`

Bucket mapping from the derived message class:
- `Unread` -> `unread`
- `PendingAck` -> `pending_ack`
- `Read` -> `history`
- `Acknowledged` -> `history`

The display buckets are a presentation contract. They are not the canonical
two-axis model.

### 7.8 Default Selection And Historical Expansion

Default queue inspection behavior:
- `atm list` returns a bounded actionable/head view
- bare `atm peek` returns one selected actionable message without mutation
- bare `atm read` returns one selected actionable message
- `--all` is the explicit full-surface override and may be slower

### 7.9 Seen-State Rules

Seen-state is enabled by default unless `--no-since-last-seen` is set.

`--since-last-seen` explicitly enables the default watermark filter. When set
explicitly, it behaves the same as the default. If both `--since-last-seen`
and `--no-since-last-seen` appear, `--no-since-last-seen` wins.

When seen-state is enabled and a watermark exists:
- unread messages remain eligible even when older than the watermark
- pending-ack messages remain eligible even when older than the watermark
- history messages are filtered by the watermark

On a true first run with no stored watermark:
- the default queue view still shows only actionable messages
- historical messages remain hidden unless `--all` is used

`--all` bypasses seen-state filtering entirely.

If seen-state updates are enabled:
- update the watermark using the latest displayed message timestamp
- do not use non-displayed messages when computing the watermark

`--no-update-seen`: when this flag is set, messages are read and displayed
normally but the seen-state watermark is not updated after the operation. The
watermark is left unchanged regardless of which messages were displayed.

`--since <iso8601>` filters to messages whose `timestamp` field is greater
than or equal to the given ISO 8601 datetime. It filters by message timestamp,
not by the seen-state watermark. It may be combined with seen-state filtering;
both constraints apply independently.

`--from <name>` is a sender filter: it restricts matched messages to those
sent by the named agent. It does not override the caller's identity.

### 7.10 Wait Mode Rules

When `--timeout <seconds>` is set on `atm peek` or `atm read`:
- establish the read selection baseline after actor resolution, inbox loading,
  workflow classification, and filter application
- if the requested selection already contains an eligible message at wait
  start, return immediately without blocking
- otherwise block until a newly arrived message becomes eligible for the
  requested read selection, or until the timeout expires
- re-run the normal selection over the updated merged inbox surface once a new
  eligible message arrives
- preserve the same sender, timestamp, seen-state, and selection filters
  during the wait

Timeout success condition:
- either the initial selection is already non-empty, or at least one message
  that was not eligible at wait start becomes eligible before the timeout
  expires

Timeout failure condition:
- the initial selection is empty and no newly eligible message arrives before
  the timeout expires

### 7.11 Mutation Rules

Peek mutation rule:
- `atm peek` never mutates mailbox state

Read mutation rules:
- any selected `atm read` message is written back with `read = true`
- `atm read` must never create a new pending-ack obligation on display
- displaying a message never promotes acknowledgement state
- only sender-owned durable `requires_ack` intent may create `pending_ack_at`
- only explicit `atm ack` handling may clear pending acknowledgement into
  `acknowledged_at`
- when a selected message already requires acknowledgement, it remains
  pending-ack after display
- when a selected message does not require acknowledgement, it remains
  `NoAckRequired` after display
- required transition on read of a normal unread message:
  - `(Unread, NoAckRequired) -> (Read, NoAckRequired)`
- required transition on read of an ack-required unread message:
  - `(Unread, PendingAck) -> (Read, PendingAck)`
- no additional ack-axis mutation happens when:
  - the message is `NoAckRequired`
  - the message is already `PendingAck`
  - the message is already `Acknowledged`
  - the message is already `Read`

### 7.12 Output Contract

`atm list` human-readable output must remain metadata-only.

`atm list` JSON output must include:
- `action = "list"`
- `team`
- `agent`
- `messages`
- `count`
- `bucket_counts`

Every list row must include:
- `message_id`
- `summary`
- `from`
- `timestamp`
- `read`
- `pending_ack`
- `task_id` (`null` when the logical message is not task-linked)

`atm read` JSON output must include:
- `action = "read"`
- `team`
- `agent`
- `message`
- `selected_message_id`
- `match_count`
- `additional_match_count`
- `bucket_counts`

`atm peek` JSON output must include the same shape as `atm read`, except:
- `action = "peek"`
- `mutation_applied = false`

When `mutation_applied = true` and `message` is present:
- `message.message_id` and `selected_message_id` must identify the same
  durable message
- `bucket_counts` must reflect the mailbox state after the read-side mutation
  completes
- the read-side mutation contract is distinct from `atm ack`; read may mark a
  message `read = true`, but only ack clears `pending_ack_at` and sets
  `acknowledged_at`

Human-readable `atm peek` and `atm read` output must render one message body
only. When additional matches exist, they must state that more matches were
found and direct the operator to `atm list` for metadata inspection instead of
emitting additional full bodies.

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
- `--json`

### 8.3 Required Behavior

- resolve the caller's own inbox using invoking-shell `ATM_IDENTITY`
- fail before daemon dispatch if caller identity is unavailable
- locate the target message in the merged inbox surface
- require the target message to be in the pending-ack ack state
- persist the ack transition back to the physical inbox file that owns the source message when the merged inbox surface includes origin inbox files
- atomically:
  - set `read = true`
  - remove `pendingAckAt`
  - set `acknowledgedAt`
  - append a reply message to the original sender's inbox unless the
    acknowledged pending-ack message is an unqualified same-agent/same-team
    historical self-addressed message
- preserve `acknowledgesMessageId` on the emitted reply
- hardcode `requires_ack = false` on the emitted reply
- do not allow an acknowledgement reply to request acknowledgement itself
- reject duplicate acknowledgement of an already acknowledged message
- run matching `[[atm.post_send_hooks]]` rules after a successful ack, using the reply message as the hook subject
- when the pending-ack message is an unqualified same-agent/same-team
  historical self-addressed message, mark it acknowledged, suppress reply
  emission, and report the suppression explicitly in the ack output contract.
  A host-qualified source is never this suppression case: it produces the
  ordinary canonical ACK reply write.

Phase R continuation semantics:
- one successful acknowledgement clears the chain-level acknowledgement
  obligation for the current terminal message and all of its ancestors
- if a later update arrives on an already acknowledged ack-required chain, the
  chain becomes pending again until the new terminal message is acknowledged

### 8.4 Successor Chains And Ephemeral Retention

- `REQ-P-THREAD-001` ATM message update chains must be strictly linear.

  Required behavior:
  - each message may have at most one direct successor
  - each successor references exactly one predecessor
  - no branching successor graph is permitted
  - the terminal node in the chain is the effective current instruction or
    state for normal reads

- `REQ-P-THREAD-002` Only the original sender may update a message chain.

  Required behavior:
  - only the root/original sender may append successors
  - recipients and third parties must not add `add-details` or `supersede`
    updates to another sender's chain

- `REQ-P-THREAD-003` ATM supports exactly two successor modes for non-ephemeral
  chains:
  - `add-details`
  - `supersede`

  Required behavior:
  - compatibility/export payloads carry successor metadata with
    `parentMessageId` and `threadMode`
  - `add-details` appends missing context while preserving the prior message as
    valid historical context
  - `supersede` replaces the prior message as the effective current
    instruction
  - logical-current selection keeps the terminal message id for both modes
  - terminal `add-details` preserves still-valid predecessor context in the
    effective current body used for matching and display
  - terminal `supersede` uses only the replacement body as the effective
    current instruction
  - if a successor arrives after the predecessor was already read, the
    successor still produces a new nudge so the current effective instruction
    is visible

- `REQ-P-THREAD-004` Ack is a chain-level importance property.

  Required behavior:
  - a chain is either ack-required or not ack-required
  - the root/original message establishes that ack class
  - successors inherit the existing chain ack class and must not flip it
  - one acknowledgement clears the chain up to the then-current terminal node
  - if a later successor arrives on an ack-required chain after that
    acknowledgement, the chain becomes pending again
  - parent messages must not remain separately actionable for acknowledgement
    once a successor exists

- `REQ-P-THREAD-005` Ephemeral messages are standalone, time-bounded records.

  Required behavior:
  - ephemeral messages expire by time only, using SQLite-owned `expires_at`
  - compatibility/export payloads carry ephemeral expiry with `expiresAt`
  - no product behavior may depend on first-read deletion semantics
  - periodic daemon cleanup deletes expired ephemeral rows
  - ephemeral messages are not updatable
  - ephemeral messages may not be parents or children in successor chains
  - once read, an ephemeral message becomes hidden from normal reads but
    remains visible through `--view-all` until `expires_at`

- `REQ-CORE-MAILBOX-UNIFIED` Mutable mailbox/runtime state must be owned by one
  canonical SQLite table, `mail_message_states`.

  Required behavior:
  - `mail_messages` remains the immutable message-content table
  - `mail_message_states` is the only canonical owner for mutable mailbox
    state:
    - `read`
    - `pending_ack_at`
    - `acknowledged_at`
    - `expires_at`
    - `deleted_at`
    - `updated_at`
  - the retired split-state model (`mail_visibility_states` plus `ack_state`)
    must not be reintroduced under old or new names
  - normal mailbox queries must hide rows with `deleted_at`
  - deleted rows may surface only through explicit admin/diagnostic paths
  - time-bounded ephemeral retention uses `expires_at` from
    `mail_message_states`, not a field on `mail_messages`

### 8.5 Output Contract

JSON output must include:
- `action = "ack"`
- `team`
- `agent`
- `message_id`
- `reply_disposition`
  - `kind = "sent"` with `reply_message_id` and `reply_target` when a reply
    message was emitted
  - `kind = "suppressed_self_ack"` only when the historical pending-ack
    message was unqualified same-agent/same-team and no reply was emitted
- `reply_text` (validated reply body; retained even when self-ack suppression
  prevents reply emission)
- `task_id` (optional String, present when the source message has `taskId`)
- `warnings` (array of strings, omitted when empty)

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
- `--team <name>`
- `--older-than <duration>`
- `--idle-only`
- `--dry-run`
- `--json`

### 9.3 Required Behavior

- default to the caller's own inbox when no target agent is provided
- resolve the target inbox using the retained address and identity rules
- fail before daemon dispatch if caller identity is unavailable
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

`atm doctor` remains a local diagnostics command, but in the current SQLite/daemon architecture
architecture it must also report daemon/runtime availability because normal ATM
mail behavior depends on the singleton daemon being present.

Phase-AA target direction:
- `atm doctor` keeps daemon/runtime reporting, but daemon routing is not the
  only legal path for diagnostics
- checks that are inherently local and store-backed may run directly from the
  CLI
- daemon-routed doctor data is additive and exists for daemon-owned runtime
  state or faster asynchronous answers when the daemon is already live

### 11.2 Supported Flags

- `--team <name>`
- `--json`

### 11.3 Required Checks

The initial doctor implementation must cover:
- config file discovery and parse health
- effective team resolution
- caller identity/team visibility and optional diagnostic scope behavior
- obsolete config identity drift detection (`[atm].identity` and legacy
  top-level `identity`)
- daemon control-socket existence and reachability
- singleton daemon ownership health
- SQLite mail-store path visibility and openability when the current runtime is
  active
- baseline `[atm].team_members` coverage against `config.json.members`
- team directory existence
- team config existence and parse health
- inbox directory existence and writability
- stale mailbox lock detection across `~/.claude/teams/*/inboxes/*.lock` using
  start-of-run and end-of-run snapshots; a lock present in both snapshots is
  stale and must be reported with `ATM_WARNING_STALE_MAILBOX_LOCK` as a
  transitional compatibility finding rather than a normal mail-correctness
  dependency in the current SQLite/daemon architecture
- `ATM_HOME`, `ATM_TEAM`, and `ATM_IDENTITY` override visibility
- `sc-observability` initialization health
- active shared log path visibility
- `sc-observability` query-health readiness for `atm log`

Caller-context behavior for `atm doctor`:

- `atm doctor` must not require `ATM_IDENTITY`
- `atm doctor` must not require `ATM_TEAM`
- `atm doctor --team <name>` may narrow diagnostic scope when supplied
- `atm doctor` may report caller-context visibility and invalid override
  situations diagnostically, but it must not fail solely because caller
  identity/team are absent

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
- `atm teams update-member`
- `atm teams backup`
- `atm teams restore`

The retained surface explicitly does not include broader historical team
orchestration commands such as:
- `spawn`
- `join`
- `resume`
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
- persist the member's durable `home_dir` on the canonical ATM roster row and
  project that same `home_dir` into compatibility `config.json.members`
- create any required local inbox state atomically with the roster update

`atm teams update-member` must:
- validate that the target team exists
- validate that the target member already exists
- update existing canonical roster metadata without creating a new member
- accept point updates for the accepted mutable roster metadata:
  - `home_dir`
  - `harness`
  - `agent_type`
  - `model`
  - `recipient_pane_id`
- reject requests that attempt to use `update-member` as implicit member
  creation
- reject operator attempts to set `cwd`, `live_cwd`, or `launch_cwd`; runtime
  working location and startup-location logging are not operator-settable
  through `update-member`
- project the repaired metadata deterministically into compatibility
  `config.json`
- preserve unchanged member metadata when a field is not supplied

`atm teams backup` must:
- create a timestamped snapshot under the ATM team backup area
- capture the current `config.json`
- capture the ATM-owned `.atm-state` tree for workflow compatibility state when
  present
- capture the selected team's durable state from the host-scoped SQLite
  database at `~/.atm/db/mail.db`
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
- restore the ATM-owned `.atm-state` workflow compatibility state from the
  chosen snapshot when present
- restore the selected team's durable state back into the host-scoped SQLite
  database from the chosen snapshot
- restore non-lead inbox files from the chosen snapshot deterministically
- treat stale inbox `*.lock` sentinels as transitional compatibility
  diagnostics rather than a restore correctness gate
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

`update-member` JSON output must additionally include:
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
- load the local team roster from canonical ATM roster state
- return a structured error when the team is missing from canonical ATM roster
  state
- show all rostered members deterministically, with `team-lead` first when
  present and remaining members in stable local order
- use these names distinctly:
  - `home_dir`: durable SQL-backed agent-home directory for the member; for
    worktree-backed members it preserves the worktree home and the canonical
    association back to the owning main repo
  - `live_cwd`: runtime-only working-directory overlay for the invoking ATM
    member when the active CLI/doctor process can bind `ATM_IDENTITY` to the
    displayed member; never durable roster metadata
  - `launch_cwd`: startup-only current-directory snapshot emitted to ATM CLI
    startup logs; never durable roster metadata
- never use bare `cwd` when `launch_cwd` or `live_cwd` is the real meaning
- expose currently persisted member metadata that ATM already knows durably,
  such as `home_dir`, type, model, or pane id, and may overlay `live_cwd` for
  the invoking member only
- not persist `live_cwd` or `launch_cwd` as canonical member roster metadata
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

## 14. `atm help` (Phase Y additive CLI feature)

Product requirement ID:
- `REQ-P-HELP-001` `atm help` must satisfy the documented conceptual-help
  contract for the daemon + SQLite release line.

Satisfied by:
- `REQ-ATM-CMD-001` for CLI entry, parsing, and dispatch aspects
- `REQ-ATM-OUT-001` for human-readable and JSON output aspects

### 14.1 Purpose

Provide one ATM-owned conceptual help surface that complements clap-generated
syntax help without duplicating the flag/argument contract already exposed by
`--help`.

### 14.2 Required Behavior

`atm help` must:
- remain a separate subcommand from clap-generated `atm --help`
- provide `atm help --list`
- provide `atm help <topic>`
- provide `atm help <topic> --json`
- delegate `atm help <subcommand>` to the authoritative clap `--help` output
  first, with any ATM-owned prose appended after that output when needed
- treat clap output as the single source of truth for command flag
  documentation
- keep concept topics in one typed topic registry rather than scattered prose
  fragments
- keep topic output concise and point to installed long-form docs when the
  topic has an authoritative user-doc file
- keep this Phase `Y` slice narrowly on conceptual help plus wording cleanup
  rather than broadening into general structured JSON-input work

Tier-1 concept topics for the first delivery:
- `config`
- `errors`

Tier-2 concept topics for the first delivery:
- `hooks`
- `identity`
- `skills`

### 14.3 Output Contract

Human output must:
- clearly distinguish concept topics from command syntax help
- preserve clap output verbatim when the target is a subcommand
- include the installed-doc pointer when the concept topic has authoritative
  long-form user docs

JSON output must:
- expose the requested topic or command target
- identify whether the result is:
  - `overview`
  - `topic_list`
  - `concept_topic`
  - `command_help`
- include the rendered help body in a structured field suitable for agent use
- include the installed-doc pointer when the concept topic has authoritative
  long-form user docs

### 14.4 Installed User Documentation

Product requirement ID:
- `REQ-P-USER-DOCS-001` ATM must ship a versioned installed end-user document
  corpus that complements `atm help`.

Satisfied by:
- intentionally undecomposed product requirement; this spans repo-owned
  documentation, release packaging, help rendering, and publisher preflight

Required behavior:
- the authoritative repo-owned source tree for installed end-user docs is
  `docs/user-documents/`
- the installed destination is `<install-root>/share/doc/atm/`
- the installed primary entrypoint is `<install-root>/share/doc/atm/README.md`
- the default local-install root remains `~/.local/atm/<version>/`
- installed-doc lookup at runtime is derived from the installed `atm` binary
  location using the executable-relative path `../share/doc/atm/`
- runtime state under `~/.atm/` must remain distinct from the installed
  document tree
- `ATM_HOME` is the runtime/data root only and must not be used to locate the
  installed end-user document tree
- `atm help` may stay concise, but it must point users to the installed corpus
  for long-form operator guidance
- end-user docs must remain operator-facing only:
  - no direct SQLite queries
  - no direct database edits
  - no repo-internal development workflow instructions
- hook and built-in nudge-template docs must enumerate the exact supported
  operator surface and variables
- all user-doc links must be relative so the copied installed tree remains
  navigable after packaging

- `REQ-P-USER-DOCS-002` Installed end-user docs are a release-gated artifact.

Satisfied by:
- intentionally undecomposed product requirement; this spans repo-owned
  validation and release/publisher automation

Required behavior:
- every file in `docs/user-documents/` must carry the accepted metadata header
  with `reviewed_for_release`
- the release/publisher gate must fail closed when a required user-doc file is
  missing, stale for the target release version, or structurally invalid
- fenced `json`, `xml`, `toml`, and `bash` examples in the user-doc corpus
  must be mechanically validated
- the same canonical verifier must validate both the repo-owned source tree and
  the staged/installed copied tree
- phase-close evidence must prove the installed/archive output contains the
  expected copied corpus

## 15. Message And Workflow Model

Product requirement ID:
- `REQ-P-WORKFLOW-001` The message/workflow model must satisfy the documented
  persisted-field, two-axis, and legal-transition rules.

Satisfied by:
- `REQ-CORE-WORKFLOW-001` for the canonical two-axis model and legal
  transitions

### 15.1 Persisted Message Fields

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
- `parentMessageId`
- `threadMode`
- `metadata`

Unknown fields must be preserved.

For ATM-authored messages:
- ATM machine-readable identity is mandatory
- ATM uses one logical message identity and exports it through `message_id` on
  the shared compatibility surface
- ATM service addressing accepts ULID text only
- thread/update metadata uses `parentMessageId` plus `threadMode`
- time-bounded ephemeral retention uses SQLite-owned `expires_at`
- ATM-authored machine identifiers must not be null or blank

Legacy or externally imported records may still omit `message_id`; the rewrite
must preserve such records without inventing synthetic ids during read.

### 15.2 Two-Axis Canonical Model

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

### 15.3 Required State Transitions

```text
Send normal message
  -> (Unread, NoAckRequired)

Send ack-required message
  -> (Unread, PendingAck)

Send task-linked message
  -> persist taskId
  -> (Unread, PendingAck)

Read own inbox with marking enabled, normal unread message
  (Unread, NoAckRequired) -> (Read, NoAckRequired)

Read own inbox with marking enabled, ack-required unread message
  (Unread, PendingAck) -> (Read, PendingAck)

Peek any inbox
  (Unread, NoAckRequired) -> (Unread, NoAckRequired)
  (Unread, PendingAck) -> (Unread, PendingAck)
  (Read, NoAckRequired) -> (Read, NoAckRequired)
  (Read, PendingAck) -> (Read, PendingAck)
  (Read, Acknowledged) -> (Read, Acknowledged)

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

### 15.4 Task Metadata Rule

Messages with `taskId` are task-linked messages.

Required rules:
- every task-linked message must require acknowledgement
- a task-linked message remains actionable until acknowledged
- a task-linked message must continue to appear in `atm read` until acknowledged
- a task-linked message must never be removed by `atm clear` before acknowledgement

## 16. Observability Requirements

Product requirement ID:
- `REQ-P-OBS-001` ATM observability must satisfy the documented best-effort
  emit behavior and shared query/follow/health expectations.
- `REQ-P-OBS-002` ATM retained logs must use one host-scoped ATM-owned default
  directory and must not derive that retained location from `ATM_HOME`.
- `REQ-P-OBS-003` ATM retained logging must be non-silent by default for the
  daemon lifecycle baseline and for every warning/error emitted by ATM
  subsystems.
- `REQ-P-OBS-004` ATM retained-log maintenance must keep daemon success-path
  observability off the synchronous file-I/O hot path.

Satisfied by:
- `REQ-ATM-OBS-001` for CLI bootstrap/injection aspects
- `REQ-CORE-LOG-001` for ATM log query/follow service aspects
- `REQ-CORE-DOCTOR-001` for observability health reporting aspects
- `REQ-CORE-OBS-001` for ATM event and query-model boundary aspects
- `REQ-DAEMON-OBS-001` and `REQ-DAEMON-OBS-002` for daemon/runtime retained
  event-baseline aspects

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

Required retained-log maintenance behavior:
- successful daemon event emission must spend at most one bounded in-memory
  handoff on the synchronous path; it must not reopen, append, flush, rotate,
  or prune retained files inline before returning control to the active daemon
  request/lifecycle path
- blocking retained-log admission must use the queue-backed
  `sc-observability` logger admission path (`Logger::log()`); any future
  non-blocking admission path must use `Logger::try_log()` and handle explicit
  queue-full degradation
- `flush()` / `shutdown()` are the only durability barriers for retained-log
  writes; queue admission alone must not be treated as immediate persistence
- retained-log file append, rotation, and pruning must run on background
  maintenance machinery instead
- if retained-log maintenance falls behind, ATM must degrade explicitly with
  structured diagnostics rather than silently blocking the daemon success path
- retained-log pruning must use a bounded work budget per maintenance tick and
  must not rely on an unbounded wall-clock scan
- the retained-log shutdown threshold is configured through
  `RetainedLogPolicy.writer_shutdown_timeout`
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

Typed observability migration requirement:
- ATM must complete the phased migration from raw observability labels to
  validated `ActionName` / `OutcomeLabel` values at every `DaemonEvent`
  construction site and every `SubsystemObservability::event()` call site.
- The current Phase W line intentionally stops short of that full migration.
  The remaining call-site conversion is tracked work, not optional cleanup.
- The final migration step depends on upstream `sc-observability-types`
  support for a validated static-construction helper such as
  `validated_static!` or `const new_static()`.

Sink policy:
- the shared file sink is required for retained ATM observability
- default ATM-owned retained logs live at `~/.atm/logs/atm.log.jsonl`
- `ATM_LOG_DIR` overrides the exact retained log directory
- retained log location is host-scoped and must not derive from `ATM_HOME`
- ATM-owned retained logs must not default to:
  - `~/logs/`
  - `~/.claude/logs/`
  - `.local/share/logs/`
- the shared console sink is optional and must remain off by default for normal
  ATM CLI command execution so command output stays stable
- console logging may be enabled later for explicit local debugging or
  integration testing

Diagnostic logging rules:
- retained logging must include the daemon lifecycle baseline by default:
  - start requested
  - startup completed / ready
  - shutdown requested
  - shutdown completed
  - degraded / abnormal-exit signals
- command failures must emit structured failure diagnostics before the CLI
  exits, even when the command fails before reaching a core service
- degraded recovery paths that intentionally continue, such as malformed-record
  skips or missing-config fallback warnings, must also emit structured warning
  diagnostics
- every ATM warning/error diagnostic must carry a stable ATM-owned error code in
  addition to human-readable text
- command lifecycle failure events must include the stable error code when one
  is available
- every `warn!` / `error!` event emitted by ATM subsystems must remain present
  in retained logs by default

`atm log` and `atm doctor` are not best-effort features in the same sense:
- they are explicit observability consumers
- if shared query/health APIs are unavailable, they must fail with clear structured errors

## 17. Error Requirements

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

Current runtime required families:
- store:
  - SQLite bootstrap/open
  - schema/transaction
  - busy-timeout / saturation
- ingest:
  - replay/import failure
  - backpressure/degraded ingest
- export:
  - historical Claude compatibility export failure
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

## 18. Reliability Requirements

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

## 19. Testing Requirements

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

Required testing architecture:
- default test suites and all core correctness tests must not depend on:
  - daemon spawn
  - socket publication timing
  - retry sleeps
  - environment mutation races
  - auto-start side effects
  - unbounded waits
  - panic-unsafe shared/global test hooks
- these patterns are treated as sources of flake and false confidence rather
  than as acceptable test infrastructure
- a test that might hang is invalid even if it does not use
  `thread::sleep(...)`
- tests must use bounded waits tied to observable predicates or handshakes

- `REQ-P-SMOKE-001` The repository must expose one smoke command family:
  - `just smoke`
  - `just smoke fast`
  - `just smoke thorough`

  Required behavior:
  - `just smoke fast` must prove the clean-room happy path on a new disposable
    baseline:
    - daemon bring-up
    - team setup
    - `doctor`
    - `atm send` without `--requires-ack`
    - `atm send` with `--requires-ack`
    - `atm read`
    - `atm ack`
    - nudge-visible flow
    - clean shutdown
  - `just smoke` must include the `fast` lane plus broader retained/admin/
    operator coverage and must provide root-cause notes for every deviation
  - `just smoke thorough` must include the `normal` lane plus every CLI
    interface on happy path and common error paths, with explicit PASS/FAIL/
    SKIP row output and root-cause notes for every deviation
  - `just smoke thorough` must also include one real same-host `atm-graft`
    lane that proves:
    - one graft host session connects to the same daemon used by the CLI lane
    - post-send nudge delivery succeeds end-to-end
    - unary graft `read`, `ack`, and `send` all succeed over the shared daemon
      contract
    - the CLI operator can observe the graft-host reply/follow-up effects
  - `just smoke thorough` must also include one shared-host multi-workspace
    lane where two or more workspaces use different `ATM_HOME` values while
    sharing the same host `HOME`, daemon, SQLite database root, and retained
    log root; that lane must prove:
    - concurrent `send` traffic from multiple workspaces succeeds
    - concurrent `read` / `ack` traffic from multiple workspaces succeeds
    - no cross-workspace message leakage occurs
    - the shared daemon remains healthy until both workspaces finish

- `REQ-P-SMOKE-002` Smoke reporting must write:
  - tracked latest smoke reports:
    - `reports/smoke/smoke-fast.md`
    - `reports/smoke/smoke.md`
    - `reports/smoke/smoke-thorough.md`
  - gitignored timestamped smoke reports using the shared
    `YYYY-MM-DD-HH-MM-SS-*` convention
  - one canonical JSON payload per run that records row verdicts, binary SHA,
    duration, and pass/fail/skip counts

- `REQ-P-SMOKE-003` Smoke logging must support two modes:
  - smoke/debug mode may enable detailed lifecycle/send/read/ack/nudge event
    visibility for retained-log analysis
  - ordinary runtime logging must remain quiet enough that routine send/read/
    ack success does not clutter normal operator logs

- `REQ-P-COVERAGE-001` Coverage reporting must remain separate from ordinary
  test execution.

  Required behavior:
  - the repository must expose `just test coverage`
  - plain `just test` must not implicitly collect coverage
  - coverage reporting must write tracked latest reports:
    - `reports/coverage/mac.md`
    - `reports/coverage/win.md`
  - coverage reporting must also write gitignored timestamped reports using
    the same timestamp convention as smoke reporting
  - an explicit local coverage run may overwrite only the tracked latest
    report for the host platform that executed the run
  - the other tracked platform report may remain at its last real result or an
    explicit placeholder until that platform executes its own coverage run
  - Linux tracked-latest coverage artifacts are deferred/unsupported in the
    current Phase Z line and the coverage runner must fail clearly on Linux
    rather than silently pretending to produce supported tracked artifacts
- bare `join()`, `recv()`, `wait()`, or equivalent waits are prohibited in
  risky runtime/daemon test paths unless completion has already been proven by
  a bounded synchronization step
- test code must not use or reintroduce the current daemon-spawn pattern by
  name:
  - `spawn_test_daemon`
  - `warm_daemon`
  - `DaemonGuard`
  - `ATM_DAEMON_BIN`
  - direct `Command::new(...atm-daemon...)`
- there is no approved "test daemon launch" path for ordinary ATM correctness
  tests
- the primary test tiers are:
  - CLI/composition tests using a fake HTTP application client
  - in-process integration tests using the HTTP adapter over the shared
    request/response contracts
  - a narrow daemon-runtime suite for singleton/startup/shutdown/recovery
    requirements only
- real daemon process tests, if any, must be isolated to the daemon-runtime
  suite and must never become the default validation path for CLI or core
  business correctness

## 20. Acceptance Criteria

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
  visibility in the current SQLite/daemon architecture
- `atm teams` provides the retained local team recovery surface
- `atm members` provides the retained local roster verification surface
- retained commands preserve documented behavior, and any current-runtime shape
  changes are explicit in the requirements and architecture
- workflow-axis classification is correct
- workflow-axis transitions are encoded in implementation structure
- display buckets are derived consistently from the two-axis model
- task-linked messages remain pending until acknowledged unless the operator
  explicitly acknowledges them through `atm ack`
- observability integration is exercised by automated tests
- the file-by-file migration plan is complete enough to implement directly
- daemon singleton is enforced as requirement `#1` with the documented
  multi-layer guards
- the default test and CI paths contain no banned daemon-spawn helpers or
  timing-based daemon orchestration patterns
- the lint gate that enforces singleton/test-fidelity rules passes in `just
  lint`

Cross-document invariants that must remain true:
- `taskId` implies ack-required behavior at send time
- displayed messages always persist `read = true`
- pending-ack messages remain actionable until acknowledged
- `atm clear` never removes unread messages
- `atm clear` never removes pending-ack messages
- `atm read --timeout` returns immediately when the requested selection is already non-empty


## 21. Phase M: Mailbox Concurrency And Restore Atomicity

Phase M addresses blocking and important findings from the Phase L code review
(ARCH-CR-001 through ARCH-CR-004 and associated QA findings) that must be
closed before the 1.0 release.

### 21.1 Mailbox Concurrency Safety

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
  - any retained historical Claude inbox tooling must not let the sentinel lock
    block Claude Code native inbox appends because Claude does not participate
    in ATM's cooperative lock protocol

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

### 21.2 Shared Mutable File Atomicity

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

  Mailbox state is durable SQLite state; send, read, ack, clear, and
  missing-config notices persist it through the retained mailbox runtime.

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
    - mailbox state:
      the retained SQLite mailbox runtime (`persist_message_record(...)` and
      `persist_message_state(...)`); no filesystem workflow sidecar exists
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
  - command-layer code must not add a filesystem workflow-state mirror; SQLite
    remains the sole mailbox-state authority.

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

### 21.2.1 Shared Commit And Freshness Validation

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

### 21.2.2 Locking Failure-Path Test Contract

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

### 21.3 Restore Transaction Atomicity

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

### 21.4 Error Display And Diagnostics

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

### 21.5 Code Consolidation And Documentation

- `REQ-CORE-IDENTITY-CONSOLIDATE-001` The duplicated `resolve_actor_identity`
  function must be consolidated into a single shared implementation.

  Required behavior:
  - the identical helper currently present in `ack/mod.rs`, `clear/mod.rs`, and
    `read/mod.rs` must be moved to `identity/mod.rs` as `pub(crate)`

- `REQ-CORE-CONFIG-DOC-001` The deprecated `[atm].identity` config key and
  legacy top-level `identity` key must be documented in a `# Deprecated`
  section in the config module documentation.

  Required behavior:
  - migration guidance: use `ATM_IDENTITY` environment variable instead
  - reference `ATM_WARNING_IDENTITY_DRIFT` error code

- `REQ-CORE-PANIC-DOC-001` The panic path in `normalize_json_number` must be
  eliminated and documented.

  Required behavior:
  - `normalize_json_number(...)` must return the raw input string on exponent
    parse failure or unsupported exponent range instead of panicking
  - a library function must not panic on potentially untrusted input

## 22. Current SQLite Mail SSOT, Runtime Boundaries, And Lock Elimination

The current SQLite/daemon architecture supersedes the mailbox-lock line as the target architecture for ATM
mail correctness. The `REQ-CORE-MAILBOX-LOCK-*` requirements remain
transitional compatibility constraints only for the interim file-based line.
The release-complete target is elimination of mailbox-lock dependence from ATM
mail correctness.

### 22.1 SQLite Mail And Roster Ownership

- `REQ-CORE-RUNTIME-001` ATM mail and team roster state must move to SQLite as
  the authoritative source of truth.

  Required behavior:
  - SQLite is the durable source of truth for:
    - message records
    - read/unread state
    - ack-required / acknowledged state
    - clear/delete/message state
    - task linkage and task metadata
    - team roster
  - Claude-owned inbox JSONL files are compatibility ingress/export surfaces,
    not ATM's authoritative durable mail store
  - `config.json` becomes a roster-ingress source, not the durable roster truth

- `REQ-CORE-STORE-001` The SQLite store must use one documented schema
  contract with stable keys, constraints, and indexes.

  Required behavior:
  - the authoritative schema must define at least:
    - `messages`
    - one unified mutable message-state surface
    - one canonical roster/member surface
    - `inbox_ingest`
  - `message_key` is the canonical ATM durable message identity
  - `message_key` format must be deterministic and typed by source family:
    - `atm:<ulid>` for ATM-authored durable rows
    - `ext:<fingerprint>` for imported external rows without ATM ids
  - schema constraints must forbid duplicate authoritative identities
  - schema changes are contract changes and require explicit user approval plus
    synchronized requirements, architecture, and boundary doc updates before
    implementation is accepted
  - the schema must document the required lookup indexes for message lookup,
    task lookup, visibility projection, and ingest dedupe

- `REQ-CORE-STORE-002` The SQLite store must enforce WAL and explicit
  transaction policy.

  Required behavior:
  - `journal_mode = WAL`
  - `foreign_keys = ON`
  - mutating ATM command flows must use explicit transactions
  - no production mutating path may rely on implicit per-statement autocommit
    as the normal correctness model

- `REQ-CORE-STORE-003` All database access must use the backend-neutral
  storage-trait contract.

  Required behavior:
  - only `atm-storage-rusqlite` may import `rusqlite`, own schema/SQL, or
    expose concrete SQLite behavior
  - daemon, core, CLI, graft, and transport code must hold storage traits only
  - `atm-runtime` may assemble selected backend trait objects but must not
    expose SQLite types or introduce a daemon-specific persistence trait
  - new backend selection occurs at composition without changing daemon,
    transport, CLI, or graft source
  - replay/outbox/finalizer traits created solely for daemon transport state are
    forbidden

- `REQ-CORE-INGEST-001` Inbox/config ingest must use one owned contract for
  replay, backpressure, and degradation.

  Required behavior:
- ingest must be idempotent
- historical Claude inbox ingest tooling must accept the prior legal inbox
  container shape: one top-level JSON array document for each shared `.json`
  inbox file
- parseable external rows must not be silently dropped
- malformed external rows must emit structured diagnostics rather than panic
- on the earlier compatibility line, legal Claude JSON-array inbox files stayed
  on the normal supported ingest path; repair/rebuild was reserved for
  malformed or unsupported mailbox state
- backlog/slow-ingest conditions must surface through structured diagnostics
  or health findings rather than dropping records silently
- roster/config ingest must apply one deterministic last-write-wins policy
  for replacing roster truth in SQLite

- `REQ-CORE-RUNTIME-003` Crash recovery preserves committed local mailbox
  state. The daemon must not maintain a replay store, remote outbox, or retry
  state.

- `REQ-CORE-RUNTIME-002` Live agent status must not use SQLite as its
  authoritative live truth.

  Required behavior:
  - live status is runtime-owned daemon state
  - SQLite stores canonical roster membership and optional routing metadata,
    but not the current process `pid`
  - daemon memory caches the current `pid` as the primary liveness field
  - daemon runtime state must include `last_active_at` for each known active
    agent/member entry
  - the shared protocol must expose typed heartbeat request/response DTOs for
    runtime state updates and PID continuity handling
  - SQLite must not own live `last_active_at`; it remains daemon-memory-only
    runtime state
  - roster truth and live-status truth must remain distinct
- `pid` is transient daemon-owned runtime state rather than durable roster
  truth and must not be persisted in SQLite

### 22.2 Singleton Daemon Runtime

- `REQ-CORE-DAEMON-001` ATM must run exactly one daemon per host in the current architecture
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
    - direct post-send emission routing when persistence succeeds
  - the daemon must not become the only place where ATM mail semantics are
    implemented

- `REQ-CORE-DAEMON-003` Production ATM commands must connect to the daemon and
  auto-start it when absent.

  Required behavior:
  - production CLI/runtime calls first attempt to connect to an already-running
    daemon
  - if the daemon is absent, the production CLI/runtime path auto-starts it
    and retries once
  - if the daemon remains unavailable after auto-start, ATM must fail with a
    clear recovery message rather than silently falling back to direct SQLite
    or inbox-file access
  - in-process test harnesses may bypass the daemon only inside explicit test
    wiring, not in the production path

  Satisfies:
  - `REQ-P-RUNTIME-001`

- `REQ-CORE-DAEMON-004` The daemon must implement one documented graceful
  shutdown and runtime-control contract.

  Required behavior:
  - each supported host platform must expose one typed graceful-shutdown
    control path before listeners begin accepting
  - each supported host platform must expose one typed bounded reload/rescan
    control path without releasing singleton ownership
  - Unix may satisfy this through `SIGINT` / `SIGTERM` / `SIGHUP`
  - Windows may satisfy this through console or service-control equivalents
  - graceful shutdown must stop accepts, drain inflight work, checkpoint WAL,
    and release singleton ownership in order
  - Phase R transport remains one request per accepted connection, so the
    documented `32` per-connection inflight ceiling is satisfied by structure
    until framed multiplexing is introduced

### 22.2.1 Phase S Daemon Parity Traceability

- `REQ-DAEMON-PLATFORM-001` is the `atm-daemon` crate traceability record for
  the same-host daemon parity requirement carried by:
  - `REQ-P-PLATFORM-001`
  - `REQ-P-PLATFORM-002`
  - the same-host daemon portions of `REQ-CORE-DAEMON-003`
  - the same-host daemon portions of `REQ-CORE-DAEMON-004`
- `REQ-DAEMON-PLATFORM-002` is the `atm-daemon` crate traceability record for
  constraining operating-system differences behind daemon-owned portability
  boundaries for:
  - `REQ-P-PLATFORM-002`
  - `REQ-CORE-BOUNDARY-001`

### 22.3 Strict I/O Ownership Boundaries

- `REQ-CORE-BOUNDARY-001` Every subsystem must be behind a strict trait
  boundary for all external I/O.

  Required behavior:
  - only the owning store subsystem may touch SQLite
  - only the owning config-ingress subsystem may parse team `config.json`
  - only the owning transport subsystem may touch sockets
  - only the owning post-send/advisory subsystem may talk to agent processes
  - no business logic may live in I/O adapter code
  - no "just this one call site" bypasses are allowed
  - I/O-owning boundary traits are sealed by default; opening a boundary for
    external implementation requires explicit architectural approval
  - concrete I/O adapter types and constructors remain private unless a
    documented boundary contract requires wider visibility
  - violation of any ownership rule above is a direct QA failure for the Phase
    Q implementation line

### 22.3.1 Structured Error Boundaries

- `REQ-CORE-BOUNDARY-002` Production runtime code must model fallible runtime
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
  - the `AtmErrorCode` registry is centralized and read-only from the
    perspective of feature/service code; subsystems consume codes from the
    registry and do not mint local alternatives
  - violation of these structured-error rules is a direct QA failure for the
    current SQLite/daemon implementation line

### 22.4 Transport And Routing Model

- `REQ-CORE-TRANSPORT-001` Phase AI must replace the local frame protocol with
  one HTTP daemon API with local and peer production ingress classes plus one
  test adapter.

  Required behavior:
  - Unix same-host clients use HTTP over UDS through the shared
    `atm-daemon-client` facade and may use HTTP over loopback TCP; consumers
    such as `atm-graft` must not take a direct `interprocess` dependency.
    Windows same-host clients use HTTP over loopback TCP only
  - normal remote peers use HTTPS over TCP; the explicit daemon-only
    `plaintext-test` smoke profile is governed by
    `REQ-CORE-TRANSPORT-002B1` and cannot create a second HTTP route
  - all production adapters call one HTTP router and the same application handlers
  - the stable initial resources are `/v1/atm/messages`,
    `/v1/atm/message/{message-id}`, `/v1/atm/message/{message-id}/read`, and
    `/v1/atm/doctor`; their typed route-specific schemas and methods are the
    versioned OpenAPI contract
  - the test adapter exercises the same router/handler contract without a live
    socket
  - HTTP adapters perform decode, authentication, and response translation
  only; they must not perform SQLite mutation, acknowledgement mutation,
  recipient routing, or post-send emission
  - Windows loopback TCP binds only a loopback address and requires a daemon
    created owner-readable endpoint record plus a 32-byte base64url local
    capability; Unix UDS uses owner-only endpoint permissions
  - ingress authentication creates `AuthenticatedIngress::Local` only after
    the local capability or UDS ownership check, and
    `AuthenticatedIngress::Peer` only after mTLS plus allowlist verification;
    adapters must not infer local/peer status from socket family or address
  - Unix/Windows parity requires equivalent local HTTP request/response tests:
    UDS plus loopback TCP on Unix and loopback TCP on Windows
  - Unix clients select UDS by default. `ATM_LOCAL_TRANSPORT=tcp` is the
    explicit, observable loopback-TCP parity/diagnostic mode; an unavailable
    UDS endpoint must fail rather than silently falling back to TCP

- `REQ-CORE-TRANSPORT-001B` Request routing must live behind one explicit HTTP
  router and injectable typed application handlers.

  Required behavior:
  - transport adapters hand authenticated HTTP requests to the router
  - the router owns route selection only
  - concrete request-family behavior lives in injectable handlers behind the
    dispatcher
  - adding a route must not duplicate an existing handler or require adapter
    logic beyond decode + dispatch
  - any violation of this dispatcher/handler rule is a direct QA failure for
    the current SQLite/daemon implementation line

- `REQ-CORE-TRANSPORT-001A` is historical only.

  Phase AD retires filesystem watch/reconcile from the accepted runtime.
  New transport or daemon work must not preserve or expand that retired
  subsystem.
  - on the earlier compatibility line, the daemon implementation could use a
    bounded polling watch registry instead of OS-native filesystem
    subscriptions, and the watch lifecycle remained daemon-owned and long-lived
    rather than one-shot helper calls
  - historical reconcile triggering supported debounce/coalesce so repeated
    identical requests did not fan out into duplicate import work
  - `R.17` had completed this lane as a daemon-owned polling watch registry, an
    ordered debounce/coalesce reconcile worker, and a queued notifier runtime;
    those lanes started and stopped only through the daemon composition root
  - the historical notifier lane used a bounded queue of `64` events and failed
    closed with typed backpressure instead of silently buffering unbounded
    plugin traffic

- `REQ-CORE-TRANSPORT-002` After AI.9, cross-host traffic must be daemon-to-daemon HTTPS
  only.

  Required behavior:
  - native agent/plugin code talks only to the local daemon
  - cross-host delivery happens only between daemons
  - agent/member names and team names are path-segment-like identifiers, not
    free-form labels
  - the only allowed characters in agent/member names and team names are ASCII
    letters, ASCII digits, `-`, and `_`
  - agent/member names and team names must reject:
    - path delimiters: `/` and `\`
    - traversal forms: `.` and `..`
    - reserved address delimiters: `.` and `:`
    - whitespace
    - wildcard or pattern characters that could be interpreted by current or
      future parsers, including at minimum `*`, `?`, `[` and `]`
  - the supported remote-send CLI form is exactly
    `atm send <agent>@<team>.<host> ...`; host qualification is part of the
    typed address grammar, not a second flag or alternate route
  - because team names cannot contain `.`, the inline form splits at the first
    `.` after `@`; the remainder is the host and may be a DNS name or IP
    address containing additional periods
  - one post-write router selects local nudge for an empty destination host and
    the HTTPS adapter for every present destination host, including `localhost`
    and the daemon's own advertised or bound IP address
  - local CLI HTTP, host-qualified same-host HTTP, and remote peer HTTP submit
    the same canonical write resource and request schema; TLS/authentication
    is adapter work before that resource, never a second write endpoint,
    persistence path, ACK path, or nudge path
  - every canonical write orders idempotent persistence, optional receiver-side
    acknowledgement mutation, and exactly one post-write router dispatch;
    nudge or peer delivery must never occur before persistence
  - a destination host is consumed as an origin-side routing selector before
    an authenticated peer request reaches receiver-side routing; source host is
    durable provenance shown by read/nudge/ack projections
  - the canonical local write may persist the sender's immutable outbound
    message record before post-write routing, but it must not create a local
    recipient-inbox row for a remote recipient or any remote-delivery queue
  - when a host-qualified same-host peer receipt encounters that daemon's own
    identical retained origin ULID, storage logs
    `peer_duplicate_write_skipped` with the ULID, both hosts,
    `same_store_peer_receipt=true`, `database_write=skipped`, and
    `delivery=continued`; it skips the second database write without altering origin destination-host
    metadata; ordinary inbound recipient delivery continues to its post-write
    local nudge and must not re-enter peer delivery. A later ACK to that
    retained record derives its host-qualified reply target from the preserved
    origin destination metadata and still creates the ordinary canonical ACK
    write

- `REQ-CORE-TRANSPORT-002A` Cross-host HTTPS listener, local certificate, and
  peer-trust configuration must use durable storage-backed state rather than
  environment variables. SQLite is the initial backend behind that trait.

  Required behavior:
  - the daemon reads enabled bind/advertise interfaces, certificate identity,
    and trusted peers from durable state
  - CLI commands are the sole operator surface for adding, enabling,
    disabling, replacing, removing, and listing those records
  - if no enabled interface rows exist, no cross-host listener binds
  - environment variables must not configure cross-host networking or trust

- `REQ-CORE-TRANSPORT-002D` A peer authority is one durable registered DNS
  hostname, HTTPS port, and pinned certificate fingerprint.

  Required behavior:
  - a hostname target exact-matches one registered authority name; its durable
    HTTPS port selects the endpoint
  - a literal IP target is accepted only when a bounded fresh DNS lookup of
    exactly one registered hostname contains that address
  - resolved addresses are not stored in SQLite or another durable alias store
  - zero or multiple matching registered names fail closed before TLS or route
  - reverse DNS is forbidden; an IP-only registration never authorizes a name
  - every registered hostname must be forward-resolvable by the peers that
    use it; a changing VPN or Wi-Fi address is updated by the host's normal
    DNS/DDNS mechanism, never by ATM reverse lookup or a SQLite IP alias
  - several account-owned daemons may use the same current host IP only when
    each authority has a distinct `(hostname, port)` endpoint and certificate
    pin; an OS bind collision fails closed and must not select another port
  - trust add, replace, and revoke refresh the one live daemon's verifier
    atomically without starting a second daemon

- `REQ-CORE-TRANSPORT-002B` Cross-host inbound authorization must use mTLS and
  a durable deny-by-default exact peer allowlist before routing.

  Required behavior:
  - inbound peers are rejected unless their declared stable host identity,
    configured HTTPS port, and pinned certificate fingerprint match one
    enabled record; the TCP source IP is routing information only
  - wildcard, prefix/suffix, subnet-derived, and regex trust are forbidden
  - rejection happens before router, mailbox, acknowledgement, or roster work
  - doctor output must surface listener, certificate, and trust state without
    exposing private key material

- `REQ-CORE-TRANSPORT-002B1` A daemon may run an explicit, process-local
  plaintext peer-wire profile only for debug/smoke diagnosis.

  Required behavior:
  - default and every normal release invocation use mTLS plus the exact peer
    allowlist; no TLS, certificate, or allowlist failure may fall back to
    plaintext
  - only `atm-daemon --peer-wire-security plaintext-test` enables plaintext;
    the setting is non-durable, not environment-driven, and a restart without
    that argument restores mTLS
  - plaintext-test uses the same HTTP resource, `WriteRequest`, router,
    persistence, and post-write path as mTLS. It must not introduce a
    plaintext-only message shape, route, nudge, or acknowledgement path
  - plaintext-test does not authenticate or authorize a peer. A declared
    source-host is untrusted smoke provenance only and must not be presented as
    authenticated, used to authorize a recipient, or treated as production
    trust evidence
  - doctor, retained logs, smoke JSON, and XHTML label the active wire-security
    mode. Plaintext-test evidence never satisfies mTLS/allowlist acceptance
    criteria

- `REQ-CORE-TRANSPORT-002C` Same-host proof must use the ordinary remote-host
  contract and must not be implemented as a special loopback-only send mode.

  Required behavior:
  - same-host transport proof uses the same daemon peer listener/send path as
    any other remote host proof
  - `localhost` and the host's own advertised or bound IP address are valid
    ordinary remote-host targets
  - the required same-host transport proof targets the daemon's advertised or
    bound virtual-Ethernet IP over TCP; a `localhost` row is address-grammar
    coverage only and cannot substitute for that proof
  - same-host proof must not require a dedicated wire field, request flag, or
    special-case routing branch outside the normal remote-host classifier
  - successful same-host rows do not by themselves authorize second-host
    release claims

- `REQ-CORE-TRANSPORT-003` Cross-host transport owns no delivery state.

  Required behavior:
  - no replay store, outbox, retry queue, deferred receipt, remote
    acknowledgement state, or duplicate-delivery subsystem may exist
  - an unavailable peer returns one normal transport error for the attempted
    write; retry is an ordinary repeat of the immutable message identity
  - duplicate arrival is idempotent at storage by the existing message ULID;
    an identical already-delivered remote duplicate has no side effect, while
    the narrow same-host retained-origin receipt defined by
    `REQ-CORE-TRANSPORT-002` logs its skipped write and continues the ordinary
    inbound recipient nudge without a second database write
  - the only exception is REQ-CORE-TRANSPORT-003A's bounded, user-selected
    reconciliation scan; it creates no delivery state

- `REQ-CORE-TRANSPORT-003A` Bounded peer reconciliation may re-send canonical
  immutable records after a peer reconnects without adding delivery state.

  Required behavior:
  - durable backend-neutral `PeerSyncPolicy.max_message_age` and
    `max_batch_messages` control the feature; zero age disables it by default
    and the batch cap defaults to 100
  - an operator can enable policy and request a one-shot sync. Automatic work
    is signalled only by a locally persisted host-qualified write or an
    unconfirmed peer delivery; ordinary peer success does not create a probe
    loop or a second scheduler
  - storage queries locally persisted outbound records for the exact peer newer
    than the configured age and returns their original ULID and immutable
    payload through a storage trait
  - every selected record uses the ordinary canonical HTTPS write request; no
    receiver-specific replay path exists
  - no outbox, replay store, retry queue, background monitor, checkpoint,
    cursor, receipt, retry budget, or per-message delivery state is allowed
  - an exact duplicate ULID/payload is a no-op except for the
    same-host-retained-origin receipt defined by `REQ-CORE-TRANSPORT-002`,
    which logs a skipped write and continues its inbound nudge without a
    database write; same ULID with different immutable data returns a typed
    conflict, logs the discrepancy, preserves the original record, and has no
    side effect or panic
  - explicit sync runs one bounded pass through the same per-host coordinator
    as automatic recovery; it introduces no second transport or write route

- `REQ-CORE-TRANSPORT-004` A remote write succeeds only after the remote daemon
  accepts the canonical write request.

  Required behavior:
  - local admission alone is not remote success
  - a failed HTTPS request may leave the already-persisted immutable local
    sender record, but creates no remote recipient row, delivery receipt,
    retry state, or sender-side acknowledgement mutation
  - the receiving daemon validates the recipient against its own local roster
    in the shared write handler; it never reads or preflights the sender host's
    roster. A remote rejection returns the ordinary `AtmError` response and
    leaves receiver mailbox state unchanged
  - acknowledgement is an ordinary canonical write with
    `acknowledges_message_id` populated; its state transition occurs only in
    the receiver's shared write handler
  - the origin-created message ULID and all immutable fields are preserved on
    the receiver; exact already-delivered remote duplicates do not repeat a
    nudge or acknowledgement transition. The same-host retained-origin
    receipt is the narrow exception defined by `REQ-CORE-TRANSPORT-002`: it
    logs the skipped database write and continues the inbound nudge without a
    second record or peer re-delivery. A conflicting payload for the same ULID
    is a typed error

- `REQ-CORE-TRANSPORT-005` The daemon runtime must use concrete timeout and
  capacity limits for transport/store/health operations.

  Required behavior:
  - every request has one absolute `RequestDeadline`; local HTTP, router,
    dispatcher, post-write router, and HTTPS consume only its remaining budget
  - no peer connect, TLS, request, or response leg may create a longer
    independent deadline below dispatcher scope
  - SQLite `busy_timeout`: `5000ms`
  - ingest batch processing slice: `2s`
  - doctor health query deadline: `3s`
  - max concurrent accepts: `64`
  - max per-connection inflight requests: `32`
  - ingest queue depth: `1024`
  - SQLite handle budget: `1..=4`
  - live status-cache cap: `4096`
  - saturation behavior must fail with typed errors or structured degradation,
    never silent drop
  - outbound peer connections resolve/bind per attempted request so ordinary
    local interface changes do not require daemon restart
  - inbound HTTPS listeners bound to wildcard/unspecified local addresses must
    survive ordinary interface rebinding without daemon restart
  - if the configured listener bind address itself changes or disappears, the
    daemon must require bounded reload/rebind through the documented reload
    path and must surface degraded status until rebind succeeds
  - HTTP request bodies are capped at `1_048_576` bytes and rejected before decode; UDS,
    loopback TCP, and HTTPS shutdown stop accepts then drain or cancel tracked requests within
    the one documented daemon shutdown deadline

- `REQ-CORE-TRANSPORT-005A` A remote write is confirmed only after the peer
  daemon returns canonical HTTP acceptance.

  Required behavior:
  - origin persistence is observable separately and is never labelled sent
  - deadline, disconnect, or failed response after dispatch returns the typed
    `REMOTE_DELIVERY_UNCONFIRMED` error, never `DAEMON_UNAVAILABLE` when the
    local daemon accepted the request
  - accepted work remains runtime-tracked and is cancelled on deadline expiry
    or local disconnect; detached delivery is forbidden
  - the possible remote side effect is resolved only by repeating the same
    immutable ULID through ordinary idempotent write handling
  - daemon logs record `write_persisted`, `peer_delivery_confirmed`, or
    `peer_delivery_unconfirmed`; terminal handler/response-write failures are
    retained structured events

- `REQ-CORE-TRANSPORT-003B` An enabled peer reconciliation policy may recover
  recent immutable outbound writes after connectivity loss without delivery
  state.

  Required behavior:
  - policy selects a bounded send window and batch; zero window disables it
  - exactly one non-durable drain lease exists per canonical peer hostname;
    it owns storage paging, one connection, ordered sends, and final rescan,
    but contains no message ID, payload, cursor, receipt, or attempt history
  - every drain advances a transient exclusive `(created_at, message_ulid)`
    lower bound through pages of at most `max_batch_messages`, ordered
    oldest-first, until an empty page, transport failure, or cancellation. It
    sends ordinary canonical writes sequentially on one HTTP(S) connection;
    no batch request shape or recovery-only endpoint is allowed. The lower
    bound is dropped with the in-memory lease and is never durable state
  - every newly persisted outbound write signals a per-host generation. The
    drain must re-scan before lease release if that generation changes, and a
    post-release signal starts the next lease; no write may be lost in the
    final-scan/release race
  - the post-write router has one `PeerDeliveryCoordinator::deliver_after_persist`
    handoff for host-qualified writes. It serializes a foreground write behind
    the same host lease and existing older backlog; it must not open a second
    socket or bypass ordered canonical records. A request-local wait ends at
    its existing deadline and is never durable or per-message delivery state
  - first recovery attempt is no earlier than 60 seconds; later failures use
    exponential backoff capped at 15 minutes
  - recovery submits original ULIDs through normal HTTPS; no ping, outbox,
    cursor, receipt, payload cache, per-message attempt state, or alternate
    write route is allowed
  - empty window, policy disable, or peer revoke stops scheduling
  - retained events distinguish scheduled, attempted, confirmed, and
    unconfirmed recovery without body or certificate material
  - doctor exposes a bounded, secret-free per-host link projection: quality,
    last success/failure, last typed error, next attempt, drain state, and
    bounded candidate count. It is observability only, not durable delivery
    state

### 22.5 Direct Post-Send And Native Agent Path

- `REQ-CORE-COMPAT-001` Claude inbox-append runtime behavior and the former
  `crates/atm-storage-claude` backend are retired from the accepted line.

  Required behavior:
  - no retained production path may use Claude inbox `.json` or `.jsonl` files
    for context injection or background ingress
  - if a retained Claude mailbox compatibility export helper survives
    temporarily, it must be explicit historical/obsolete-only scaffolding and
    must not define current send/read/post-send semantics
  - the accepted line must not ship the former `atm-storage-claude` crate or its
    boundary records as a production backend
  - the shared backend contract remains required after Claude backend
    retirement; SQLite is one backend implementation and future SQL backend
    support remains an architectural requirement
  - no retained production path may require watcher/reconcile observation of
    Claude mailbox files
  - any surviving Claude mailbox documentation must be clearly historical and
    must not redefine current send/read semantics

- `REQ-CORE-COMPAT-002` Native agent/plugin traffic must use the daemon API,
  not Claude mailbox JSON.

  Required behavior:
  - native agent/plugin delivery and notification uses the daemon API only
  - thin-client surfaces such as graft align to the shared daemon/API contract
    rather than to a mailbox-JSON transport

- `REQ-CORE-COMPAT-003` Post-send behavior must use one direct post-persist
  emitter seam.

  Required behavior:
  - `atm send` persists the message to durable ATM state
  - `atm ack` persists the reply to durable ATM state
  - after successful persistence, ATM emits post-send behavior only when the
    recipient exposes that capability
  - the shipped default post-send path is the built-in in-process
    implementation
  - teams may override any subset of the six built-in nudge template bodies
    through host-scoped, team-keyed ATM-managed override rows resolved through
    the storage-neutral `NudgeTemplateOverrideStore` contract
  - emission failure must be logged and surfaced as a sender-visible warning
  - post-send emission must not redefine send success after persistence
  - the authoritative Phase AD release smoke lane for post-send behavior must
    prove exactly these closure cases:
    - external hook success
    - external hook partial failure
    - built-in fallback
    - override reset-to-default after a prior stored override row
    - explicit disable behavior when that retained state is supported

- `REQ-CORE-COMPAT-004` Post-send capability resolution must not depend on
  caller working directory or retired mailbox/config side channels.

  Required behavior:
  - running `atm send` from another repository or working directory must not
    silently change whether post-send emission is attempted
  - hook configuration lookup must follow the sender's canonical roster
    `home_dir` metadata
- authoritative `recipient_pane_id`, when known, must come from canonical ATM
  roster state rather than from rediscovering live pane routing through local
  mailbox files
- live pane routing for built-in tmux nudge must not depend on committed
  `.atm.toml` `tmux_pane_id` values
- retained repo-local compatibility helpers may use only authoritative
  `recipient_pane_id` payload/roster data or an explicit operator-provided
  `--pane`; they must not revive committed `.atm.toml` pane lookup

- `REQ-CORE-COMPAT-005` `NotificationSink`, queued notifier runtimes, and
  typed delivery-plan execution are not the governing send-path contract.

  Required behavior:
  - post-send ownership must remain a direct emitter seam on the send/ack path
  - if notification logging is retained, it must be a direct append at the
    event site rather than a daemon worker/runtime subsystem
  - no retained send/ack contract may require `DeliveryPlan`,
    `ReplyDeliveryPlan`, or `NotificationSink`

### 22.6 Lock Elimination Target

- `REQ-CORE-LOCK-RETIRE-001` ATM mail correctness must stop depending on
  mailbox lock artifacts.

  Required behavior:
  - mailbox locks may remain only as transitional compatibility machinery for
    the interim file-based line
  - the current SQLite/daemon architecture must eliminate mailbox-lock dependence from
    normal ATM mail correctness
  - completion of the current architecture requires that stale lock artifacts can no longer wedge
    normal ATM mail flows

### 22.7 Test Strategy Constraints

- `REQ-CORE-TEST-RUNTIME-001` Core target daemon-runtime behavior must be
  testable without daemon process spawning.

  Required behavior:
  - daemon spawning is not part of the core test strategy
  - core service behavior must be testable in-process
  - transport/watch/runtime logic must be testable with fakes or in-process
    harnesses
  - no default test path may depend on daemon process lifecycle to validate ATM
    mail correctness
  - there is no approved test-only daemon launch path for ordinary ATM
    correctness tests
  - ordinary tests must not depend on socket publication timing, retry sleeps,
    parent-process environment mutation, or auto-start side effects

### 22.8 Observability Requirements

- `REQ-CORE-OBS-002` The target daemon-runtime architecture must keep
  structured observability first-class at both CLI and daemon boundaries.

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

### 22.8.1 Doctor Health Interface

- `REQ-CORE-DOCTOR-002` The target daemon runtime must expose a daemon health
  query interface consumable by `atm doctor`.

  Required behavior:
  - `atm doctor` remains a CLI command
  - daemon/runtime health information must be obtained through an explicit
    daemon-facing interface rather than direct CLI inspection of private daemon
    state
  - the health interface must be able to report at least:
    - daemon reachability
    - daemon liveness and readiness as separate dimensions
    - singleton ownership status
    - live status-cache summary
    - ingest backlog / degraded-ingest state when present
    - SQLite open/readiness state

### 22.9 QA Invariants

- `REQ-CORE-QA-RUNTIME-001` Every QA pass for the current runtime must verify the daemon
  and boundary invariants.

  Required behavior:
  - impossible to run two active ATM daemons on one host
  - daemon singleton remains host-wide rather than socket-path-local
  - daemon unavailability after one auto-start attempt fails clearly with no
    hidden direct I/O fallback
  - every subsystem performs external I/O only through its owning trait
    boundary
  - production error handling uses typed `Result`/error-enum boundaries instead
    of panic/unwrap for fallible runtime paths
  - daemon/runtime code remains thin and does not accumulate business logic
  - daemon spawning is not the test strategy
  - banned daemon-spawn helpers and launch shortcuts are absent from the
    default test path
  - SQLite remains the source of truth for mail and roster
  - live agent status remains runtime-owned state
  - structured `sc-observability` coverage remains present at both CLI and
    daemon layers
  - any retained historical Claude compatibility export remains a
    compatibility projection only and is never the ATM-owned runtime truth
  - runtime roster truth remains the canonical ATM roster rather than
    `config.json`
  - `config.json` parsing remains limited to the approved ingress/comparison
    allowlist rather than generic retained command/runtime access

### 22.10 Postmortem Lint Backfill

- `REQ-P-LINT-POSTMORTEM-001` Mechanically-detectable postmortem finding
  families must become repository lint or CI gates rather than recurring QA
  rediscoveries.

  Required behavior:
  - `atm-core` is the proving ground for new postmortem lint rules; a rule
    lands here first, is tuned against the live codebase, and is migrated to
    standalone `sc-lint` only after the rule shape is stable and demonstrably
    reusable
  - reusable Rust/static-analysis rules must be implemented against the
    embedded `crates/sc-lint-*` surface on the `atm-core` branch before any
    upstream migration
  - ATM-specific repository policy rules may stay as `.just/` or `scripts/`
    lints when the semantics are tied to ATM-only names, documents, or review
    process state
  - the default `just lint` path remains the required development gate for
    any new postmortem rule that is cheap and deterministic enough for normal
    local use

  Family-specific obligations:
  - ungated `std::os::unix` imports in production paths must fail the
    portability lint unless they are already protected by an approved Unix-only
    boundary
  - `#[cfg_attr(not(unix), allow(dead_code))]` must not be used as a
    portability suppressor in production code
  - duplicated raw semantic literals in non-test Rust code must fail the ATM
    identity-literal gate unless they come from a canonical constant or an
    explicit allow-list
  - raw `"team-lead"` role-name literals are the first mandatory case and must
    fail everywhere except the canonical role-definition source
  - fixed `thread::sleep(...)` in ordinary Rust test code must fail a
    test-hygiene gate unless the file or callsite is explicitly part of the
    narrow daemon-runtime suite
  - mechanically-detectable unbounded wait patterns in the narrow same-host
    daemon/runtime suites must move into repository lint or analyzer gates once
    the rule shape is proven deterministic enough for default local use
  - `PORT-004` must reject production `std::os::unix` imports that are not
    protected by an approved Unix-only boundary
  - `PORT-005` must reject
    `#[cfg_attr(not(unix), allow(dead_code))]` when used as a portability
    suppressor in production code
  - `SCB-RUNTIME-001` must reject bare production `Condvar::wait(...)`
  - `SCB-RUNTIME-002` must reject production `wait_timeout*` calls whose
    `WaitTimeoutResult` is discarded or stored only in underscore bindings
  - `SCB-CONFIG-001` must reject production direct team `config.json` roster
    reads outside the explicit allowlist; during `Z.7` the only approved
    survivor is
    `hydrate_roster_from_team_config_once_at_startup_if_empty(...)` with
    `sunset_sprint = "Z.8"`
  - `SCB-CONFIG-002` must reject generic runtime `load_team_config(...)`
    helper use from retained command/runtime paths and boundary adapter chains
  - `SCB-CONFIG-003` must reject Claude send paths that consult `config.json`
    before the durable ATM write has succeeded
  - bare `Condvar::wait(...)` in non-test production code must fail a runtime
    liveness gate; `wait_timeout(...)` and `wait_timeout_while(...)` remain the
    required production shapes
  - triage Turtle records must not report contradictory aggregate and terminal
    state fields in the same record
  - any scoped exclusions for the semantic-literal gate must stay narrow and
    explicit; they must not exempt ordinary production code wholesale
