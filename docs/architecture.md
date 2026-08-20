# ATM CLI Architecture

> **Phase AI target — not yet implemented:** the daemon API becomes REST over
> HTTP: Unix HTTP/UDS or loopback TCP, Windows loopback TCP, and HTTPS/TCP for
> remote peers. ADR-032 through ADR-038 and `REQ-CORE-TRANSPORT-*` govern that
> migration. The current implementation remains the custom transport contract
> documented by the current boundary manifests.

## 1. Overview

The current target architecture keeps the ATM CLI surface, but moves durable
mail and roster ownership to SQLite and reintroduces one tightly-bounded
singleton daemon runtime for routing, notification, transport, and runtime
health/state queries.

The current merged workspace contains:
- `atm-core`: reusable service library
- `atm`: CLI binary

The daemon/runtime expansion adds:
- `atm-daemon`: daemon runtime binary / transport host
- `atm-runtime`: concrete runtime/store composition root
- `atm-storage-rusqlite`: first concrete SQLite store implementation

The CLI stays thin. Product logic moves into `atm-core`.

The retained command surface is:
- `send`
- `list`
- `read`
- `ack`
- `clear`
- `log`
- `doctor`
- `teams`
- `members`

Approved additive CLI feature for the Phase `Y` line:
- `help`
- the `help` addition stays on the CLI conceptual-help surface only; it does
  not reopen mailbox-truth or boundary-ownership work inside `Y.1`

## 1.1 Installed User Documentation Surface

ATM has two distinct documentation audiences:

- repo/developer documentation under `docs/`
- installed end-user documentation sourced from `docs/user-documents/`

The installed user-doc surface is part of the product architecture:

- the repo-owned source tree is `docs/user-documents/`
- packaging copies that tree into `<install-root>/share/doc/atm/`
- the installed primary entrypoint is `<install-root>/share/doc/atm/README.md`
- the default local install root is `~/.local/atm/<version>/`
- installed-doc lookup is executable-relative from `<install-root>/bin/atm` as
  `../share/doc/atm/`
- runtime state under `~/.atm/` remains separate and must not be presented as
  the installed document tree
- `ATM_HOME` remains the runtime/data root only and must not be used as the
  installed-doc locator
- long-form operator guidance lives in installed markdown, not in new help-only
  commands
- `atm help` remains the concise CLI-owned conceptual-help layer that points
  users toward the installed corpus
- installed user docs must survive the copy step unchanged, so inter-document
  links are relative and validated mechanically
- fenced `json`, `xml`, `toml`, and `bash` examples in the installed corpus
  are release artifacts and must be validated before publish
- one canonical verifier should validate both the repo-owned source tree and
  the staged/installed copy

## 1.2 Documentation Structure

Documentation structure is governed by
[`documentation-guidelines.md`](./documentation-guidelines.md).

This file owns product architecture. Crate-local architectural detail is being
moved into:

- [`docs/atm/architecture.md`](./atm/architecture.md)
- [`docs/atm-core/architecture.md`](./atm-core/architecture.md)
- [`docs/atm-daemon/architecture.md`](./atm-daemon/architecture.md)
- [`docs/atm-runtime/architecture.md`](./atm-runtime/architecture.md)
- [`docs/atm-rusqlite/architecture.md`](./atm-rusqlite/architecture.md)

### 1.3 Verification Report Site

`site/` is the repository's durable static verification-report publish root.
`just reports-index` runs `.just/generate_report_index.py` to discover
schema-versioned report envelopes, validate their relative HTML/evidence
paths, and generate `site/reports/index.html`. AI.47 owns generation of
`site/index.html`, which links to that reports index. The generator owns report
discovery: benchmark envelopes aggregate to one report; fuzz envelopes produce
one report entry per campaign. Report producers invoke it after every artifact
write, and `just reports-index --check` rejects stale or invalid report
indexes. Under ADR-044, `site/` is public data: envelopes carry only an opaque
`host_label`, never raw host or endpoint data.
`.just/build_view_site.py` may expose links only; `artifacts/view` is transient
and never a second report renderer.

Phase-Q supersession note:
- earlier daemon-free architecture statements in this file are historical from
  the prior rewrite line
- for the current mail/runtime target architecture, Section 21 is
  authoritative

Phase-R redesign note:
- the abandoned early SQLite/daemon line is not the architectural baseline for forward
  work
- the Phase R redesign starts from crate-local boundary inventories and ADRs,
  then rebuilds the implementation under lint/visibility guardrails
- Phase R treats thin-client extension pressure as a first-class architectural
  input (`atm-graft` was implemented in T.6–T.8, then completed and narrowed
  through Phase U as the supported thin-client host line rather than removed)
- for the boundary / adapter model, Phase R supersedes any earlier
  pre-Phase-R architecture statements in this document that conflict with the
  crate-local boundary inventories, ADRs, or `docs/plans/phase-R/plan-phase-R.md`

Phase-S portability note:
- the Phase R integrated daemon proved the runtime split, but it still hard-
  coded Unix-only assumptions into the same-host host/runtime shell
- Phase S is the active planning line for making the daemon feature-complete on
  Windows as well as Unix-like hosts
- feature parity across supported operating systems is mandatory; platform-
  specific implementation differences are allowed only behind documented ATM-
  owned portability boundaries
- the legacy frame protocol is deleted and has no fallback contract
- Phase AI's target HTTP resources and schemas are owned by
  [`docs/atm-daemon/http-api.md`](./atm-daemon/http-api.md)
- S.5 planning adds `atm list` as a distinct CLI query surface; S.7 owns the
  implementation line that refines the queue-query packet mapping instead of
  preserving the old multi-message `read` response shape as the final
  contract
- Phase S planning is tracked in [`docs/plans/phase-S/plan-phase-S.md`](./plan-phase-S.md)

Phase-AA simplification note:
- the current daemon composition root and daemon-routed doctor model are not
  the intended steady-state architecture
- Phase AA moves concrete SQLite construction into a dedicated `atm-runtime`
  crate and removes adapter-specific health/observability ownership from
  `atm-daemon`
- the daemon remains in the product, but as a thin router rather than a
  concrete storage/runtime host
- subsystem-specific diagnosis belongs behind subsystem-owned diagnostic traits
  instead of daemon-local backend-aware helpers
- top-level doctor code may aggregate subsystem reports and daemon-owned
  runtime state, but must not reimplement backend-specific diagnosis logic
- `MailStore` and `RosterStore` remain the primary storage-neutral capability
  traits in the simplification line described here
- backend-specific implementations such as SQLite-backed and Claude-JSON-backed
  adapters are allowed to satisfy the approved behavior-named trait family
- `AA.5` relocks the daemon-to-SQLite edge in both the runtime-composition and
  SQLite boundary records, adds `crates/atm-architecture/` as the primary
  code-driven merge gate as the sole second enforcement layer, and treats
  policy widening as an architecture change rather than routine lint-data
  churn
- `AA.11` narrows the active SQLite runtime baseline to the current
  `message_id` durable schema; abandoned pre-production compatibility shapes
  such as `legacy_message_id` remain historical documentation only and are not
  part of normal bootstrap/migration behavior
- Phase `AC` later supersedes this simplification line for storage contracts:
  `MessageStore` and `RosterStore` become the approved shared contract, while
  task storage is explicitly deferred until a later line starts from canonical
  Claude-code task schema plus Pydantic validation instead of inheriting any
  speculative legacy task-store surface; `AC.6` deletes that speculative
  contract instead of preserving it as a compatibility line

## 2. Crate Boundaries

The post-Q product runtime is implemented by five crates:

- `atm-core`
- `atm`
- `atm-daemon`
- `atm-runtime`
- `atm-rusqlite`

Product-level boundary rules:

- `atm-core` owns ATM business logic and the strict I/O boundaries that the current SQLite/daemon architecture
  routes through a daemon runtime.
- `atm` owns CLI parsing, dispatch, rendering, and bootstrap.
- `atm-daemon` owns transport adapters, singleton enforcement, live-status
  runtime state, request routing, and daemon-owned runtime projection.
- `atm-runtime` owns concrete runtime/store composition and storage-neutral
  doctor/runtime assembly for daemon and direct CLI doctor callers.
- `atm-rusqlite` owns the first concrete SQLite implementation of the durable
  store boundaries.
- `atm-core` must not own clap or terminal-formatting concerns.
- `atm` must not own mailbox, workflow, log-query, or doctor business logic.
- `atm-daemon` must not become a second business-logic crate.
- `atm-runtime` must remain a thin composition crate rather than a second
  daemon or workflow host.
- `atm-rusqlite` must not absorb workflow or command logic; it implements store
  contracts only.
- crate-local boundary records in `docs/<crate>/boundaries.md` are the
  machine-readable contract used to drive architectural linting and review
- thin-client workflow surfaces should be modeled around `send` and `receive`
  rather than a broad command inventory
- Phase T added `atm-graft` as a thin-client line (T.6–T.8); Phase U later
  completed and tightened that supported line rather than deleting it
  as out-of-scope for the 1.0 surface
- `ack` may remain a retained CLI/user workflow, but thin-client protocol
  surfaces should carry it through send-shaped request data rather than a
  separate top-level method family
- Phase R may depend on `sc-lint` for boundary/parser gate verification, but
  `sc-lint` is an external tool dependency rather than an ATM-owned product
  subsystem
- durable ATM state is one host-scoped SQLite database at `~/.atm/db/mail.db`
- the daemon is the only ATM writer for that database
- direct read-only SQLite consumers are an allowed integration surface, but
  ATM-owned command/runtime writes must not bypass the documented daemon/store
  boundaries
- canonical roster truth is the ATM roster in SQLite; Claude Code
  `config.json` is ingress/projection/diagnostic surface only and must not
  become a second runtime roster-truth dependency
- mailbox row provenance/timing convenience fields are not part of the public
  message contract unless one clear product requirement explicitly keeps them

Lint and tooling boundary rules:
- `atm-core` owns repository-local lint orchestration through `just`,
  `.just/`, and `scripts/`
- reusable static-analysis engines are incubated on `atm-core` through the
  embedded `crates/sc-lint-*` workspace members, then migrated to the
  standalone `sc-lint` repository only after the rule semantics stabilize
- ATM-specific repository policy checks stay local to `atm-core` when they
  depend on ATM role names, ATM-only document schemas, or ATM team-process
  records
- postmortem-linter partition for the current follow-up line is:
  - reusable/static rules:
    - Unix platform-gating checks
    - bare production `Condvar::wait(...)` checks
    - fixed-sleep test-hygiene checks after the current repository-local rule
      shape is proven and extracted to `sc-lint`
    - `config.json` roster-boundary rules after the repository-local allowlist
      and false-positive shape is proven on `atm-core`
  - ATM-local rules:
    - duplicate semantic string-literal checks in non-test Rust code
    - targeted same-host daemon test unbounded-wait checks until or unless the
      rule family proves reusable enough for `sc-lint`
    - triage Turtle consistency checks
    - staged `config.json` allowlist gates until the reusable rule semantics
      stabilize

Crate-local boundary detail is owned by:

- [`docs/atm-core/architecture.md`](./atm-core/architecture.md)
- [`docs/atm-core/boundaries.md`](./atm-core/boundaries.md)
- [`docs/atm/architecture.md`](./atm/architecture.md)
- [`docs/atm/boundaries.md`](./atm/boundaries.md)
- [`docs/atm-daemon/architecture.md`](./atm-daemon/architecture.md)
- [`docs/atm-daemon/boundaries.md`](./atm-daemon/boundaries.md)
- [`docs/atm-rusqlite/architecture.md`](./atm-rusqlite/architecture.md)
- [`docs/atm-rusqlite/boundaries.md`](./atm-rusqlite/boundaries.md)

Historical Phase R boundary direction (retired by Phase AI):
- shared protocol contract: `AtmProtocol` in `atm-core`
- outbound transport boundary: `ClientTransport`
- inbound transport boundary: `ServerTransport`
- request routing boundary: `RequestDispatcher`
- receiver-only notification boundary: `MessageReceivedHookEmitter`
- inbound runtime status boundary: `StatusSource`
- historical production composition ownership:
  - `atm` is the CLI client composition root
  - `atm-daemon` is the runtime composition root
  - a separate composition crate remains out of scope unless an ADR opens it
- Phase AI target boundaries (not yet implemented):
  - `ApiRequest` / `ApiResponse` application contract
  - `DaemonApiClient` for CLI, graft, and tests
  - `ApiRouter` reached by every HTTP transport adapter
  - `PostWriteRouter` invoked after canonical persistence
- Phase AA target ownership:
  - `atm` remains the CLI composition root
  - `atm-runtime` becomes the concrete runtime/store composition root
  - `atm-daemon` consumes storage-neutral runtime inputs and stops
    constructing SQLite-backed adapters directly in production composition
  - relocked boundary records forbid a direct `atm-daemon -> atm-rusqlite`
    edge; any reintroduction must fail the Rust
    `crates/atm-architecture/` dependency guard (`cargo test --package
    atm-architecture`), which is the sole code-driven boundary enforcement
    layer

Current Phase R lint partition direction:
- extend the existing `sc-portability` analyzer for reusable platform-gating
  rules
- extend the existing `sc-boundary` analyzer for reusable production-liveness
  rules that need Rust-aware analysis
- treat fixed-sleep test hygiene as a reusable lint family whose current
  repository-local rule is the proving implementation before `sc-lint`
  extraction
- keep ATM duplicate semantic literal policy in the existing repository-local
  identity lint
- keep targeted same-host daemon unbounded-wait checks as repository-local
  lint/CI first, then reevaluate extraction only after the false-positive and
  allow-list shape is proven on `atm-core`
- keep triage-record validation as repository-local lint/CI unless its rule
  semantics become clearly reusable
- keep `config.json` roster-boundary allowlist checks as repository-local
  lint/CI until the rule semantics and approved-caller inventory stabilize

Active postmortem rule families:
- reusable analyzer rules:
  - test-scope portability helpers:
    - `PORT-001` hardcoded Unix-only absolute paths in test code
    - `PORT-002` direct `dirs::home_dir()` without configured override checks
    - `PORT-003` `std::env::set_var()` in test code
  - production portability rules:
  - `PORT-004` ungated `std::os::unix` imports in production code
  - `PORT-005` `cfg_attr(not(unix), allow(dead_code))` portability suppressors
  - `SCB-RUNTIME-001` bare production `Condvar::wait(...)`
  - `SCB-RUNTIME-002` discarded `wait_timeout*` results in production code
  - `SCB-CONFIG-001` production direct `config.json` roster reads outside the
    explicit allowlist
  - `SCB-CONFIG-002` generic runtime `load_team_config(...)` helper use from
    retained command/runtime paths
  - `SCB-CONFIG-003` Claude send pre-write `config.json` membership gates
  - `Z.7` keeps the rule family machine-runnable by checking in both the
    explicit allowlist and a known-bad fixture self-test for
    `just lint boundaries`
- ATM-local repository rules:
  - duplicate semantic role-name literals in non-test Rust code
  - targeted same-host daemon test unbounded-wait checks
  - triage Turtle aggregate/branch consistency checks

### 2.3 Release Publication Boundary

The `1.0` retained-surface release is a source-repo replacement of the old
`agent-team-mail` CLI/core publication path, not a new public package family.

Architectural rules:
- this repo becomes the source of truth for publishing:
  - `agent-team-mail`
  - `agent-team-mail-core`
- this repo does not publish its retained CLI/core release under the crate
  names `atm` or `atm-core`
- crate identity continuity for downstream users is preserved by package-name
  replacement while keeping the CLI binary name `atm`
- historical parity channels remain:
  - crates.io
  - GitHub Releases
  - Homebrew
- `winget` is not part of historical parity, but it is required in the new
  release architecture because Windows installation must be first-class for
  `1.0` without Rust tooling or manual archive extraction

Release-process ownership rules:
- release automation is repo-owned infrastructure, not ad hoc operator
  procedure
- the new repo must own:
  - release artifact manifest
  - preflight workflow
  - release workflow
  - release-gate script/helpers
  - release inventory generation and verification
  - Homebrew formula update automation
  - `winget` manifest/update automation and verification
- the `publisher` agent instructions are part of the release-control surface
  and must be ported into this repo with source-of-truth paths updated to the
  new repo layout and retained crate list

Release infrastructure notes:
- Homebrew continues to use the shared `randlee/homebrew-tap` repository and
  existing `Formula/agent-team-mail.rb` / `Formula/atm.rb` formulas
- `HOMEBREW_TAP_TOKEN` is a required secret for the `atm-core` repo before the
  ported Homebrew update automation can run successfully
- `winget` uses the same `randlee` publisher namespace proven in
  `claude-history`; the retained CLI package ID for this repo is
  `randlee.agent-team-mail`
- the ported `winget` flow requires a dedicated
  `WINGET_GITHUB_TOKEN` repo secret because the default workflow token cannot
  create branches / PRs against the `randlee/winget-pkgs` fork
- the release workflow should use
  `vedantmgoyal2009/winget-releaser@v2` against the Windows ZIP release asset
  and its SHA256 rather than inventing repo-specific manifest plumbing first
- the initial `winget` manifest submission is a one-time manual bootstrap
  action; recurring releases are workflow-driven after the package exists in
  `microsoft/winget-pkgs`
- release verification must treat `winget` submission success and manifest
  generation as the immediate release signal because Microsoft review normally
  delays public installability by 1-2 days

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

### 2.4 Shared Observability Boundary

`atm-core` must not import `sc-observability` directly.

Instead, `atm-core` defines a sealed `ObservabilityPort` boundary plus ATM-owned event and query models. `atm` implements that port using `sc-observability`.

ATM still owns:
- ATM-specific event naming
- ATM-specific structured fields
- mapping CLI filters to shared query/follow APIs
- ATM doctor projections over shared health models
- the host-scoped retained-log root contract, including `ATM_LOG_DIR` as the
  exact retained-log-directory override
- ATM-owned config semantics for baseline roster, alias resolution, and
  runtime-identity precedence

`sc-observability` should own as much generic functionality as possible:
- emission
- record storage and retention policy
- historical query
- follow/tail
- severity filtering
- structured field filtering
- runtime health reporting

Phase K delivered the ATM-side integration work. Phase L now governs the
remaining release-hardening, boundary cleanup, and validation needed before
initial release.

Initial retained-command integration scope:
- `sc-observability-types`
- `sc-observability`

Deferred from the initial retained-command integration scope:
- `sc-observe`
- `sc-observability-otlp`

Phase W typed observability migration note:
- `DaemonSubsystem` and the typed `emit_subsystem_event(...)` boundary are
  complete on the current line.
- The remaining migration from raw `&'static str` labels to validated
  `ActionName` / `OutcomeLabel` values at `DaemonEvent` fields,
  `SubsystemLogger` helpers, and `SubsystemObservability::event()` call sites
  is intentionally deferred pending upstream `sc-observability-types`
  support for a validated static-construction helper such as
  `validated_static!` or `const new_static()`.
- The deferred scope is still tracked architecture work across roughly
  76 call sites in 10 files; it is not an approved permanent mixed-typing
  end state.

The controlling ATM-side implementation design is:
- [`docs/atm-core/design/sc-observability-integration.md`](./atm-core/design/sc-observability-integration.md)

## 3. Module Layout

Detailed crate/module layout is owned by the crate-level docs:

- [`docs/atm-core/modules/`](./atm-core/modules/)
- [`docs/atm/commands/`](./atm/commands/)

Product-level constraints that remain relevant here:

- no plugin framework
- no daemon client
- no runtime spawning layer
- no separate `tail` command in the initial rewrite
- no separate `status` command in the initial rewrite
- the retained release-critical team recovery surface is limited to:
  - `teams`
  - `members`
  - `teams add-member`
  - `teams backup`
  - `teams restore`
- broader historical team lifecycle/orchestration commands remain out of scope

Supersession note:
- `no daemon client` and `no runtime spawning layer` describe the pre-Phase-Q
  retained CLI/runtime line only
- the current SQLite/daemon architecture in §21 supersedes those constraints with:
  - one explicit daemon runtime
  - no hidden direct SQLite fallback
  - one explicit daemon auto-start path when the daemon is absent

## 4. Core Types

### 4.1 Semantic Newtypes

Per `rust-best-practices`, validated primitives and semantic ids should not remain as raw `String` values across the service boundary.

Required public newtypes:
- `TeamName`
- `AgentName`
- `IdentityName`
- `MessageKey`
- `MessageId`
- `MessageBody`
- `MessageSummary`
- `IsoTimestamp`
- `MailAddress`
- `TaskId`

Required resource/config wrappers:
- `ConnectionCap`
- `QueueDepth`
- `RetryBudget`
- `BusyTimeout`
- `RequestDeadline`
- `HomeDir`
- `AbsolutePath`
- `LogFieldKey`
- `LogFieldValue`

These are required to reduce repeated validation and remove stringly typed command paths.

### 4.2 Workflow State And Display Types

Canonical axis enums:

```rust
pub enum ReadState {
    Unread,
    Read,
}

pub enum AckState {
    NoAckRequired,
    PendingAck,
    Acknowledged,
}

pub enum MessageClass {
    Unread,
    PendingAck,
    Acknowledged,
    Read,
}
```

Display bucket enum:

```rust
pub enum DisplayBucket {
    Unread,
    PendingAck,
    History,
}
```

Selection enum:

```rust
pub enum ReadSelection {
    Actionable,
    UnreadOnly,
    PendingAckOnly,
    ActionableWithHistory,
    All,
}
```

Ack requirement state:

```rust
pub enum AckRequirementState {
    NotRequired,
    RequiredPending,
    RequiredAcknowledged,
}
```

Display mapping is fixed:
- `MessageClass::Unread` -> `DisplayBucket::Unread`
- `MessageClass::PendingAck` -> `DisplayBucket::PendingAck`
- `MessageClass::Acknowledged` -> `DisplayBucket::History`
- displaying a message may mark it read, but it must never promote pending
  acknowledgement; ADR-022 keeps ack state sender-owned and durable
- `MessageClass::Read` -> `DisplayBucket::History`

### 4.3 Typestate Transition Model

Per `rust-best-practices`, legal workflow transitions should be encoded in the type system inside the core pipeline.

Private marker states:

```rust
pub struct UnreadReadState;
pub struct ReadReadState;
pub struct NoAckState;
pub struct PendingAckState;
pub struct AcknowledgedAckState;

pub struct StoredMessage<R, A> {
    // persisted fields + read-state marker + ack-state marker
}

impl StoredMessage<UnreadReadState, NoAckState> {
    pub fn display_without_ack(self) -> StoredMessage<ReadReadState, NoAckState>;
    pub fn display_and_require_ack(self, at: IsoTimestamp) -> StoredMessage<ReadReadState, PendingAckState>;
}

impl StoredMessage<UnreadReadState, PendingAckState> {
    pub fn mark_read_pending_ack(self) -> StoredMessage<ReadReadState, PendingAckState>;
}

impl StoredMessage<ReadReadState, PendingAckState> {
    pub fn acknowledge(self, at: IsoTimestamp) -> StoredMessage<ReadReadState, AcknowledgedAckState>;
}
```

There is no inverse transition on either axis.

The public axis enums and `MessageClass` are for reporting and filtering. The typestate markers enforce legal transitions inside `atm-core`.

### 4.4 Log Query Types

Log query types should remain generic enough to map onto shared `sc-observability` APIs.

Required public types:

```rust
pub enum LogMode {
    Snapshot,
    Tail,
}

pub enum LogLevelFilter {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

pub struct LogFieldMatch {
    pub key: LogFieldKey,
    pub value: LogFieldValue,
}

pub struct LogFieldMap(BTreeMap<LogFieldKey, LogFieldValue>);

pub struct AtmJsonNumber(String);

pub enum LogFieldValue {
    Null,
    Bool(bool),
    String(String),
    Number(AtmJsonNumber),
    Array(Vec<LogFieldValue>),
    Object(LogFieldMap),
}
```

Architectural rules:
- `LogFieldKey` replaces raw field-name strings at the public observability
  boundary
- `AtmJsonNumber` replaces raw numeric `serde_json` values at the public
  observability boundary
- `LogFieldValue` and `LogFieldMap` replace raw `serde_json::Value` /
  `Map<String, Value>` in `LogFieldMatch` and `AtmLogRecord`
- these ATM-owned types must serialize to the same JSON shape the CLI exposes
  today; the boundary cleanup is a Rust API cleanup, not a CLI wire-format
  redesign
- conversion to and from raw `serde_json` values remains centralized inside
  `atm-core`

### 4.5 Observability Construction Contract

`CliObservability` (atm crate) should expose one structured construction path
for initial release, and `CliObservabilityOptions` is also owned by the `atm`
crate:

```rust
pub struct CliObservabilityOptions {
    pub stderr_logs: bool,
}

impl CliObservability {
    pub fn new(home_dir: &Path, options: CliObservabilityOptions) -> Result<Self, AtmError>;
}
```

Architectural rules:
- the top-level `init(stderr_logs)` helper may remain as a CLI convenience, but
  it should delegate to `CliObservability::new(...)`
- dynamic dispatch via `Box<dyn ObservabilityPort + Send + Sync>` remains
  acceptable for initial release
- the current sealed-trait pattern remains acceptable for initial release
- `DoctorCommand` injectability is explicitly deferred unless implementation
  surfaces a concrete need

### 4.6 Identity And Alias Projection

ATM must distinguish canonical routing identity from the Claude-facing sender
projection.

Architectural rules:
- commands that require caller identity/team resolve them according to the
  matrix in `docs/requirements.md` §4.1, never from repo-local
  `[atm].identity` / `[atm].default_team`
- caller-context-owned commands must resolve required context at the CLI
  boundary before any daemon dispatch
- daemon-backed caller-owned request DTOs must carry required resolved caller
  context as request data
- the daemon must execute caller-owned commands against declared request
  caller context only and must never substitute daemon ambient
  `ATM_IDENTITY` / `ATM_TEAM`
- `atm doctor` is diagnostic and remains outside the mandatory caller-context
  path
- ATM-owned aliases are input shorthands that resolve to canonical member names
- same-team messages keep current canonical sender projection behavior
- cross-team messages may project an alias-friendly sender in the persisted
  `from` field for Claude-facing ergonomics
- whenever cross-team alias projection is used, ATM must also persist
  canonical sender identity in SQLite-owned state
- self-send checks, target validation, routing, and audit logic must use the
  canonical sender identity rather than the display-oriented `from` projection
- the shared send-context path rejects canonical same-team self-addressed sends
  only when their destination has no host, before message-body persistence and
  before `dry-run` can report success; a host-qualified destination proceeds
  to ordinary host routing without a locality lookup
- ATM-owned post-send hooks are best-effort recipient-scoped helpers, not part
  of the atomic send boundary
- the hook runs only after a successful non-`dry-run` send or ack; it fires
  after both `atm send` and `atm ack`
- Current runtime addition: the retained hook contract now includes `atm ack` reply
  writes as hook-producing outbound messages, not only `atm send`
- each `[[atm.post_send_hooks]]` rule binds one recipient selector and one
  command argv
- `recipient = "*"` acts as a wildcard match for all recipients
- multiple matching rules all execute, in config order
- relative post-send-hook paths resolve from the discovered `.atm.toml`
  directory and execute with that same directory as the working directory
- bare executable names use normal `PATH` lookup
- the hook receives inherited environment plus one ATM-owned JSON payload in
  `ATM_POST_SEND`
- the payload includes `from`, `to`, `sender`, `recipient`, `team`,
  `message_id`, `requires_ack`, `is_ack` (bool), and optional `task_id`
- Current runtime addition: `is_ack` is the explicit send-vs-ack discriminator for
  daemon-owned hook evaluation and downstream nudge logic
- the hook may optionally emit one structured result object on stdout with a
  declared log level, message, and optional structured fields; ATM parses it
  on a best-effort basis for post-send diagnostics
- absent or invalid hook-result stdout is ignored rather than treated as hook
  failure
- recipient non-match is silent
- retired flat hook keys and `[atm].post_send_hook_members` are configuration
  errors, not compatibility aliases
- hook execution is the direct post-persist emission seam; the accepted
  runtime must not route post-send behavior through `NotificationSink`,
  `DeliveryPlan`, or Claude mailbox compatibility machinery
- hook-decision logging must preserve sender, recipient, matched rule selector,
  and final execution outcome for troubleshooting
- hook failure or timeout never rolls back a successful send

## 5. Persisted Schema

### 5.1 Team Config

The rewrite reuses the existing team config schema where feasible.

Only a small subset is required by the retained surface:
- member roster
- enough member metadata to preserve round-trips when present
- bridge remote host configuration needed for origin-file merge when present

ATM config and team-launch config are distinct concerns:
- ATM-owned config uses the `[atm]` section of `.atm.toml`
- launcher-owned sections such as `[rmux]` and future `[scmux]` remain outside
  the `atm-core` runtime config boundary and are ignored by ATM
- `[atm].default_team` remains a config/bootstrap default only for flows that
  explicitly consume ATM config defaults; it is not a runtime caller-team
  fallback for commands governed by the caller-context matrix
- `[atm].team_members` is the ATM-owned baseline roster for doctor/orchestration
  checks
- `[atm].aliases` is the ATM-owned shorthand map for canonical agent names
- `[[atm.post_send_hooks]]` is the ATM-owned best-effort post-send automation
  surface
- retired flat hook keys and `[atm].post_send_hook_members` must fail fast
  with migration guidance
- `[atm].identity` and the legacy top-level `identity` key are obsolete in the
  retained multi-agent model and must not participate in runtime identity
  resolution

Team config loading must follow a narrow-scope recovery policy:
- compatibility-only schema drift may use deterministic defaults at the schema
  boundary
- malformed member records should be isolated at member scope only when the
  remaining roster is still trustworthy
- missing `config.json` is a distinct `missing-document` condition, not a parse
  error
- root-document corruption or invalid root structure remains a command error
- identity and routing fields must never be guessed to keep commands running

Diagnostics for team config failures must preserve:
- failure class when known
- file path
- member or collection scope when known
- parser line and column when available
- original parser cause for operator repair

### 5.1.1 Deprecated `[atm].identity`

`[atm].identity` and the legacy top-level `identity` key remain
parse-compatible only as obsolete migration fields. They are no longer part of
runtime sender or actor resolution.

Current runtime contract:
- caller-context-owned commands resolve required caller identity/team according
  to the matrix in `docs/requirements.md` §4.1
- if required caller context is unavailable, the CLI fails before daemon
  dispatch
- `atm doctor` remains the explicit identity-free, optional-team exception
- `[atm].identity` and legacy top-level `identity` are ignored for runtime
  resolution even when still present in `.atm.toml`

Deprecation and migration contract:
- `atm doctor` reports stale config identity fields with
  `ATM_WARNING_IDENTITY_DRIFT`
- operator migration path is: remove `[atm].identity` and any legacy top-level
  `identity` key, then set `ATM_IDENTITY` in the active agent environment
  instead
- keeping the obsolete key temporarily is tolerated for migration diagnostics
  only; it must not change runtime behavior

Sample operator-facing repair cases live in
[`persisted-data-repair.md`](./persisted-data-repair.md).

### 5.2 Inbox Message

Current persisted inbox superset may contain:
- Claude-native baseline fields:
  - `from`
  - `text`
  - `timestamp`
  - `read`
  - `summary`
  - optional producer field `color`
- ATM additive compatibility fields:
  - `message_id`
  - `parentMessageId`
  - `threadMode`
  - `taskId`
- unknown fields

Schema ownership split:
- Claude-native baseline fields are documented in
  [`claude-code-message-schema.md`](./claude-code-message-schema.md)
- ATM additive compatibility fields are documented in
  [`atm-message-schema.md`](./atm-message-schema.md)

U.3 body projection rule:
- terminal `add-details` composes predecessor context into the effective body
- terminal `supersede` exposes only the replacement body
- crate-local ownership for the exact projection algorithm lives in
  [`atm-core/architecture.md`](./atm-core/architecture.md)

Architectural rules:
- Claude JSON is a compatibility surface, not ATM-owned durable truth.
- No normal ATM runtime/query path may read machine state from Claude JSON.
- ATM-owned machine state belongs in SQLite-backed state and projections.
- `metadata.atm` is not an approved namespace and must not survive in active
  compatibility output.
- ATM keeps one logical message identity; retained `message_id` is the ULID
  text form of that identity.
- if SQLite persists `message_id`, it stores that same identity in the
  retained ULID text form rather than as a second ATM-owned id.
- compatibility reads may tolerate established historical top-level additive
  fields and `metadata.atm` derivatives, but they remain read-compatible
  inputs rather than the active or forward-write contract
- compatibility writes may preserve only the current approved additive surface
  and must not become the place where new ATM-owned machine state accumulates
- removed compatibility fields such as `source_team`, `pendingAckAt`,
  `acknowledgedAt`, `acknowledgesMessageId`, and `expiresAt` must stay
  SQLite-only or workflow-only even if older inbox files still contain them.

File-ownership rule:
- the private watcher/import/export boundary is the only approved place that
  may read or write the shared Claude inbox surface for ATM-owned behavior
- list/read/ack/clear/send runtime correctness must come from SQLite-owned
  state and boundary-owned projections

Canonical read and ack axes are derived from SQLite-backed persisted state and
not serialized separately in Claude JSON.

## 6. Public Service APIs

### 6.1 Send Service

Supersession note:
- the API shape in this section remains relevant
- the file-append-first ordering details below are compatibility-line behavior
  for the pre-Phase-Q runtime
- the authoritative current send ordering is defined in §21 as:
  `SQLite commit -> Claude export / remote daemon handoff`

Public entrypoint:

`send::send_mail_via_store(request: SendRequest, store: &dyn SendStore, ingress: &dyn SourceIngress, exporter: &dyn ProjectionExport, observability: &dyn ObservabilityPort) -> Result<SendOutcome, AtmError>`

Current runtime note:
- Q.2 replaced the earlier `send_mail(request, observability)` entrypoint with
  `send_mail_via_store(...)`
- the store, ingress, and exporter parameters make the SQLite-first write,
  ingest-before-export, and projection/export boundaries explicit at the public
  service seam

`SendRequest` contains:
- home directory
- current directory
- sender override
- target address input
- team override
- message source
- summary override
- requires-ack flag
- optional task id
- dry-run flag

`SendMessageSource` variants:
- inline text
- stdin text
- file reference

`SendOutcome` fields:

| Field | Type | Description |
| --- | --- | --- |
| `action` | `&'static str` | Stable send action marker. |
| `team` | `String` | Resolved target team. |
| `agent` | `String` | Resolved target recipient. |
| `sender` | `String` | Resolved sender identity. |
| `outcome` | `&'static str` | Delivery result such as `sent` or `dry_run`. |
| `message_id` | `MessageId` | The one logical ATM message identity rendered in retained ULID text form. |
| `requires_ack` | `bool` | Whether the message requires acknowledgement. |
| `task_id` | `Option<String>` | Optional task identifier persisted on the message. |
| `summary` | `Option<String>` | Generated or caller-supplied summary text. |
| `message` | `Option<String>` | Rendered message body for dry-run output. |
| `warnings` | `Vec<String>` | Actionable degraded-mode warnings surfaced when send succeeds under a permitted fallback condition. |
| `dry_run` | `bool` | Whether the send was executed as a dry run. |

The file-reference path may be rewritten through the file policy layer.

The CLI JSON output mirrors the current contract.

Normal send JSON output includes:
- `action = "send"`
- `team`
- `agent`
- `outcome`
- `message_id`
- `requires_ack`
- `task_id`
- `warnings` when send completed in a degraded but permitted mode

For the retained ATM wire shape, `message_id` is the shared ULID identifier
used by ATM-authored messages.

Dry-run send JSON output includes:
- `action = "send"`
- `agent`
- `team`
- `message_id`
- `message`
- `dry_run = true`
- `requires_ack`
- `task_id`
- `warnings` when dry-run surfaces degraded send conditions

Send ordering rules:
- resolve target address, team existence, and agent membership as one address-resolution stage before mailbox path selection
- enter the atomic append boundary before final inbox mutation
- validate message text inside the atomic append boundary
- generate the one logical message identity inside the atomic append boundary
- perform duplicate suppression and final append inside the same atomic append boundary

#### 6.1.1 Idle-Notification Lifecycle

- message classification first attempts to parse the persisted `text` field as
  JSON and treat the message as an idle notification when the parsed object has
  `type == "idle_notification"`
- if parsing fails, or `type` differs, the message is classified as a normal
  message
- when a newly appended message is classified as an idle notification, the
  mailbox append boundary removes any older unread idle notification from the
  same sender in the same inbox before appending the new record
- `atm clear --idle-only` remains manual backlog cleanup, not the primary
  lifecycle path

Deferred follow-on work:
- read-time auto-purge of displayed idle notifications
- daemon-side idle-notification removal behavior

#### 6.1.2 Task-Assignment Classification

- classification uses the same text-field JSON detection pattern and treats a
  message as a task assignment when the parsed object has
  `type == "task_assignment"`
- because the Claude Code schema is fixed, classification must populate
  `extra["task_id"]` and `extra["priority"]` from the parsed text-field JSON
  rather than extending `MessageEnvelope` with new top-level fields
- final field naming and task-subsystem semantics remain coordinated with the
  future `arch-ctask` task subsystem design; see `atm-core` issue `#17`
- task-assignment extraction remains deferred until the `arch-ctask` subsystem
  is defined

Missing-team-config fallback is limited to `send`:
- fallback applies only when `config.json` is missing and the target inbox
  already exists
- malformed `config.json` remains a command error
- fallback must surface an actionable sender warning
- fallback may send a best-effort repair notice to `team-lead`
- repair notices must be deduplicated by unresolved condition so repeated sends
  do not flood inboxes

### 6.2 Queue Inspection Services

Phase S.5 splits queue inspection into two command surfaces:

- `atm list` finds messages through a bounded metadata query
- `atm read` opens one full message

Target service shape:

- `list::list_mail(query: ListQuery, observability: &dyn ObservabilityPort)
  -> Result<ListOutcome, AtmError>`
- `read::read_mail(query: ReadQuery, observability: &dyn ObservabilityPort)
  -> Result<ReadOutcome, AtmError>`

Shared query model:
- home directory
- current directory
- actor override
- optional target address
- team override
- sender filter
- timestamp filter
- task filter
- contains filter
- queue-state filters (`unread`, `pending_ack`, `all`)

`ListQuery` adds:
- optional limit

`ReadQuery` adds:
- optional exact `message_id`
- optional timeout
- read-mutation controls such as seen-state update

`ListOutcome` contains:
- action
- resolved team
- resolved agent
- messages
- count
- bucket_counts

Each list row contains:
- `message_id`
- `summary`
- `from`
- `timestamp`
- `read`
- `pending_ack`
- `task_id`

`ReadOutcome` contains:
- action
- resolved team
- resolved agent
- selected message
- `selected_message_id`
- `match_count`
- `additional_match_count`
- `mutation_applied`
- bucket_counts

Read-mutation output invariants:
- when `mutation_applied = true` and a selected message is present, that
  message and `selected_message_id` must identify the same durable message
- read-side mutation may mark the selected message `read = true`, but it must
  still return that same message in the payload instead of re-running unread
  selection and swapping in a different unread message
- `bucket_counts` must describe the post-mutation mailbox state produced by
  that command execution
- ack-side mutation remains separate; only `atm ack` clears
  `pending_ack_at` and sets `acknowledged_at`

Queue-inspection architectural rules:
- default `atm list` must stay bounded by query behavior rather than
  materializing full mailbox history and truncating it at render time
- bare `atm read` must return one most-recent unread actionable message, with
  pending-ack messages prioritized ahead of non-ack unread messages
- selector-driven `atm read` must return the most recent match and report
  additional matches in metadata rather than returning multiple full bodies
- selector-driven `atm list` and `atm read` operate on logical current
  messages; successor/update chains are collapsed to their terminal node before
  result selection or row shaping
- `--task <task-id>` selection happens after that terminal-node collapse so one
  logical task thread does not surface as several superseded matches
- `--contains` applies to both summary text and full durable message body text
- summary/count queries must remain separable from full-body detail fetch
- metadata-backed `--contains` evaluation must remain summary-first and bounded:
  rows rejected by earlier metadata-only filters or already matched by summary
  must not trigger durable-body reload, and only surviving summary-miss
  candidates may fetch durable body text for the final contains check

Deduplication rule:
- collapse multiple entries with the same non-null `message_id` to the most
  recent entry before bucket selection and output rendering
- when timestamps tie, keep the later encountered inbox record

Read rule:
- durable workflow semantics come from SQLite-backed state, not from ATM-owned
  metadata read back out of Claude JSON

The queue-query services derive `MessageClass` from `(ReadState, AckState)` and
apply display-bucket selection to the derived class, not to raw persisted
fields.

For merged inbox surfaces, any displayed-message mutation must be written back
to the physical inbox file that contributed the displayed record. The merged
view is a read projection, not a synthetic write target.

### 6.3 Ack Service

Public entrypoint:

`ack::ack_mail<S>(request: AckRequest, store: &S, observability: &dyn ObservabilityPort) -> Result<AckOutcome, AtmError> where S: AckStore`

`AckRequest` contains:
- home directory
- current directory
- actor override
- team override
- source message id
- reply body

`AckOutcome` contains:
- action
- resolved team
- resolved agent
- source message id
- optional task id from the acknowledged message
- reply disposition
  - `Sent { reply_message_id, reply_target }` when a reply message was emitted
  - `SuppressedSelfAck` when a historical self-addressed pending-ack was
    acknowledged without emitting a replacement reply
- reply text
- warnings: Vec<String>
- Current runtime addition: `warnings` carries best-effort post-send-hook diagnostics
  for `atm ack` without changing the successful acknowledgement state

The ack service is responsible for the legal transition from `(Read, PendingAck)` to `(Read, Acknowledged)` plus the reply append.

Phase R continuation rules:
- `atm ack` emits exactly one visible reply and that reply must hardcode
  `requires_ack = false`
- historical self-addressed pending-ack messages are the explicit exception:
  they terminate at `(Read, Acknowledged)` with `AckReplyDisposition::SuppressedSelfAck`
  and no replacement reply message
- acknowledgement replies must never request acknowledgement themselves
- compatibility/export surfaces encode successor metadata with
  `parentMessageId` and `threadMode`
- message update chains are linear and terminal-node driven:
  - `add-details` appends context
  - `supersede` replaces the prior message as the effective current one
- the logical-current projection is mode-aware:
  - terminal `add-details` keeps the terminal id but composes the still-valid
    predecessor context into the current body
  - terminal `supersede` keeps only the replacement body
- only the original sender may append successors to the chain
- one acknowledgement clears the chain through the current terminal node
- the root message establishes whether the chain is ack-required and
  successors inherit that ack class
- if a later successor arrives on an already acknowledged ack-required chain,
  the chain becomes pending again until the new terminal node is acknowledged
- ephemeral messages are standalone, time-bounded rows only:
  - they use `expires_at`
  - they are not updatable
  - they may not participate in successor chains
  - they are cleaned up by periodic expiry sweep rather than first-read
    deletion
  - once read, they hide from normal reads but remain visible through
    `--view-all` until expiry

The current SQLite/daemon architecture supersedes the legacy source-file writeback rule: SQLite is the
authoritative durable store for ack state, while inbox/file-surface projection
is deferred to the Q.4 export/runtime path.

### 6.4 Clear Service

Public entrypoint:

`clear::clear_mail(query: ClearQuery, observability: &dyn ObservabilityPort) -> Result<ClearOutcome, AtmError>`

`ClearQuery` contains:
- home directory
- current directory
- actor override
- optional target address
- team override
- optional age filter
- idle-only flag
- dry-run flag

`ClearOutcome` contains:
- action
- resolved team
- resolved agent
- removed total
- remaining total
- removal counters by class

Clear eligibility is computed from the two-axis model:
- clearable: `(Read, NoAckRequired)` and `(Read, Acknowledged)`
- non-clearable: every other combination

### 6.5 Observability Boundary

The observability boundary is a sealed `ObservabilityPort` (or equivalent injected interface) defined in `atm-core` and implemented in `atm`.

It is responsible for:
- command lifecycle emission
- log query
- log tail/follow
- observability health projection

The retained boundary must remain ATM-owned and must not leak shared
`sc-observability` types directly into `atm-core` public APIs.

`atm-core` owns the ATM-specific event and query vocabulary needed for ATM’s
messaging workflows, retained-log query/follow, and doctor readiness.

`atm` owns the concrete `sc-observability` integration and CLI-facing routing
decisions such as `--stderr-logs`.

Future hook- or `schooks`-driven observability orchestration remains out of
scope for the initial ATM release and must not be inferred from this boundary.

### 6.6 Log Service

Public entrypoints:

- `ObservabilityPort::query(query: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError>`
- `ObservabilityPort::follow(query: AtmLogQuery) -> Result<LogTailSession, AtmError>`

ATM CLI surfaces such as `atm log snapshot`, `atm log filter`, and `atm log tail`
consume those boundary methods directly rather than routing through a separate
`log::query_logs(...)` or `log::tail_logs(...)` wrapper.
`AtmLogQuery` contains:
- mode
- level filters
- field matches
- time window
- limit

`AtmLogSnapshot` contains:
- returned records
- truncation flag when the shared query source truncates results

`LogTailSession` is an owning stateful object that yields matching records from the shared observability follow API without exposing a public callback trait.

Ordering rules:
- snapshot queries return newest-first records before CLI output limits are rendered
- tail sessions yield records in follow arrival order

ATM must not parse daemon log files directly in this service.

### 6.7 Doctor Service

Public entrypoint:

`doctor::run_doctor(query: DoctorQuery, observability: &dyn ObservabilityPort) -> Result<DoctorReport, AtmError>`

`DoctorQuery` contains:
- home directory
- current directory
- team override

`DoctorReport` contains:
- summary
- findings
- recommendations
- environment override visibility
- current team member roster projected from canonical ATM roster truth and
  ordered against the live `config.json` baseline
- observability health
- informational post-send configuration and recipient delivery-path projection
  with redacted matcher/argv/config-root fields only
- distinct caller-context and daemon-process version/identity visibility for
  compatibility diagnosis
- aggregate-only subsystem doctor output from:
  - `MailStoreDoctor`
  - `RosterStoreDoctor`
  - `ConfigDoctor`

Current-state caveat:
- the historical task-store doctor surface was removed during `AC.6`; future
  task storage, if approved later, starts from canonical Claude-code schema
  rather than from a preserved speculative doctor contract

`DoctorFinding` contains:
- severity
- code
- message
- remediation

The report model should reuse the current doctor command’s severity/finding
structure where useful, but in the current SQLite/daemon architecture it must include
daemon/runtime checks rather than assuming a daemon-free local-only model.
Daemon/CLI orchestration stays aggregate-only: those top-level paths may
compose the `MailStoreDoctor`, `RosterStoreDoctor`, and `ConfigDoctor` reports,
but they must not reimplement backend-specific store investigation logic.

Phase AF compatibility rule:
- following local-IPC connection and before a write-shaped dispatch, clients
  perform the ADR-027 `CompatibilityPreflight`; an incompatible verdict is a
  typed `ATM_CLIENT_DAEMON_VERSION_INCOMPATIBLE` response with no write
- this compatibility check composes after ADR-026 host-runtime admission; it
  cannot select an alternate daemon endpoint, state root, or transport path

Roster output rules:
- show all current `config.json` members in doctor output
- show baseline `[atm].team_members` first
- show `team-lead` first among the baseline members when present
- show extra runtime members after the baseline set
- snapshot `~/.claude/teams/*/inboxes/*.lock` at doctor start and end; any lock
  path present in both snapshots is stale and should surface as
  `ATM_WARNING_STALE_MAILBOX_LOCK` with recovery guidance that explicitly marks
  the lock as a transitional compatibility diagnostic rather than a current-runtime
  mail-correctness dependency

### 6.8 Team Recovery Services

The retained release-critical local team surface is intentionally narrow.

ATM-owned public entrypoints should cover:
- local team discovery
- local member listing
- local `add-member`
- local `update-member`
- local team backup
- local team restore

Architectural rules:
- these services are local file/config/inbox operations; they must not depend
  on daemon orchestration or runtime spawning
- `teams` list is discovery-oriented and should remain deterministic over the
  ATM home directory
- `add-member` is the retained local roster-repair path and must reject
  duplicates before mutating config
- `add-member` persists the member's durable `home_dir` on the canonical ATM
  roster row and projects that same `home_dir` into compatibility
  `config.json.members`
- `update-member` is the retained local roster-metadata repair path for
  existing members and must not create new members implicitly
- accepted terminology must distinguish:
  - `home_dir` = durable SQL-backed agent-home directory for the member; for
    worktree-backed members it preserves the worktree home and the canonical
    association back to the owning main repo
  - `live_cwd` = runtime-only working-directory overlay for the invoking ATM
    member when the active CLI/doctor process can bind `ATM_IDENTITY` to that
    displayed member; it is not durable roster metadata
  - `launch_cwd` = startup-only current-directory snapshot emitted to ATM CLI
    startup logs; it is not durable roster metadata
- operator repair paths may repair `home_dir` but must not treat `live_cwd` or
  `launch_cwd` as durable roster metadata
- accepted implementations must prefer direct roster-row and runtime-roster
  fields over new directory-state coordinator structs
- `backup` snapshots current team config, the ATM-owned `.atm-state` workflow
  compatibility state, a team-scoped export from the host-scoped SQLite
  database at `~/.atm/db/mail.db`, inboxes, and the ATM team task bucket into
  a timestamped snapshot directory
- inbox backup excludes transient mailbox `*.lock` sentinels, dotfiles, and
  restore markers
- `restore` is a local recovery path and must:
  - preserve the current team-lead entry and `leadSessionId`
  - restore only missing non-lead members
  - clear runtime-only restored-member state before persistence
  - restore the ATM-owned `.atm-state` workflow compatibility state from the
    chosen snapshot when present
  - restore the selected team's durable records into the host-scoped SQLite
    database from the chosen snapshot
  - restore non-lead inboxes from the chosen snapshot
  - treat stale mailbox `*.lock` sentinels as compatibility-only diagnostics;
    restore must not require sweeping them in order to restore durable ATM
    state or inbox compatibility files
  - recompute `.highwatermark` from the maximum restored task id
  - support a dry-run path without making changes
- Claude Code project task-list restoration remains separate from the retained
  ATM team backup/restore surface

### 6.9 Members Service

The retained `members` surface is a local roster inspection service.

Architectural rules:
- it must succeed without daemon or hook-only state; live runtime enrichment is
  best-effort and the command falls back to the durable local roster when the
  daemon is unavailable
- it must load the roster from local team config
- it should order members deterministically, with `team-lead` first when
  present
- it may surface persisted member metadata already present in config
- daemon-sourced session, pid, state, and timestamp enrichment may be layered
  on without changing the base local verification purpose of the command; the
  enrichment is diagnostic telemetry and is never required for roster
  inspection to succeed
- this daemon-free fallback is the CLI-side half of the runtime-health
  observation boundary described in Section 21.6.3: `MembersCommand::run`
  renders the retained roster even when `runtime_snapshot` cannot obtain a
  daemon response, while any returned observation remains telemetry only

## 7. Read Pipeline

Historical note:
- the earlier file-backed/reconcile-fed line is historical only
- the accepted runtime does not use `ingest/reconcile -> SQLite projection` as
  a live read pipeline
- AD.4 removed the remaining daemon watch/reconcile lane from the accepted
  runtime and retired the corresponding daemon/core boundary traits

The accepted read pipeline stages are:
1. resolve caller identity and target mailbox from the accepted CLI/runtime
   contract
2. load durable message state from the authoritative ATM store
3. classify read axis, ack axis, and derived message class
4. apply sender, timestamp, selection-mode, and seen-state filters
5. sort newest-first and apply limit
6. apply legal read/seen mutations for displayed messages
7. persist any read/seen state changes atomically
8. return outcome

Architectural rules:
- no accepted read path depends on watcher events, reconcile completion, or
  mailbox-file ingest
- durable ATM state, not merged mailbox-file truth, is authoritative for read
- any retained mailbox-file compatibility readers are historical or
  repair-only surfaces and do not redefine the accepted read contract

## 8. Ack Pipeline

The ack pipeline stages are:
1. resolve actor identity and own inbox
2. load the merged inbox surface and locate the source message
3. classify the source message into read and ack axes
4. require pending acknowledgement before mutation
5. resolve the reply target inbox from the source envelope
6. atomically apply the ack transition and append the reply
7. emit command lifecycle records
8. return outcome

This stage list describes the pre-SQLite compatibility line. The current
target pipeline is superseded by the SQLite SSOT and daemon-boundary design in
Section 21.

## 9. Clear Pipeline

The clear pipeline stages are:
1. resolve actor identity and target inbox
2. load the persisted inbox surface
3. classify each message into read axis and ack axis
4. compute clear eligibility from the two-axis read and acknowledgement model
5. apply optional age and idle-only filters
6. atomically persist the kept set when not in dry-run mode
7. emit command lifecycle records
8. return outcome

This stage list describes the pre-SQLite compatibility line. The current
target pipeline is superseded by the SQLite SSOT and daemon-boundary design in
Section 21.

## 10. Log Pipeline

The log pipeline stages are:
1. resolve the injected observability port implementation
2. map CLI filters into shared query/follow filters
3. query or follow records through the observability port
4. project ATM-owned record fields for CLI rendering
5. return records to the CLI layer

Shared `sc-observability` should own record storage, filtering, and follow mechanics. ATM should own only ATM-specific query defaults and field projections.

## 11. Doctor Pipeline

The doctor pipeline stages are:
1. resolve config and environment overrides
2. resolve optional diagnostic team scope and inspect caller-context visibility
3. inspect ATM config for obsolete fields such as `[atm].identity`
4. verify local team/mailbox/config paths
5. verify caller-context visibility and invalid override situations without
   making caller identity/team mandatory
6. compare baseline `[atm].team_members` against `config.json.members`
7. verify observability initialization and health
8. verify observability query readiness for `atm log`
9. assemble findings, recommendations, and ordered roster output
10. render report

## 12. Mailbox Storage

Supersession note:
- this section describes the retained mailbox/file-storage line
- the current SQLite/daemon architecture supersedes it with SQLite durable truth
  and Claude inbox files as compatibility ingress/export only
- any mailbox-lock or file-truth rule in this section is transitional unless
  restated in §21

The mailbox layer owns:
- tolerant reads
- atomic append
- duplicate suppression
- conflict merge
- origin-inbox merge
- atomic workflow-state updates
- atomic clear-set replacement
- sender-scoped idle-notification dedup inside the atomic append boundary

The mailbox layer does not own selection policy, display buckets, output formatting, log query behavior, or doctor diagnostics.

### 12.1 Atomic Full-Rewrite Semantics

All inbox modifications use atomic full-rewrite for durability and consistency:

**Atomic write pattern:**
1. Acquire per-inbox file lock before any read
2. Read and deserialize the full inbox document (JSON array or JSONL)
3. Apply modification in memory (append message, update workflow state, replace clear set)
4. Write to a temporary file with fsync to guarantee data durability
5. Atomically rename temp file over original (single filesystem operation on POSIX; platform-equivalent on Windows)
6. Release lock after rename completes

This pattern ensures:
- crash-safety: partial writes never corrupt the original file
- consistency: concurrent ATM processes never lose updates due to race conditions
- idempotency: replay of the same operation twice (e.g., after daemon restart) produces the same state

The lock is held from step 1 through step 5 to prevent concurrent read-modify-write races. Full-rewrite applies to all inbox operations: `append_message`, read-state writeback, ack transition, and clear set replacement.

### 12.2 Repair and Rebuild Seam Scope

**Repair/rebuild is reserved for malformed mailbox state. Normal healthy mailbox operations never trigger repair.**

When an inbox file is encountered:

Claude mailbox JSON is historical only in the accepted runtime.

Architectural rule:
- retained send/read/ack behavior must not depend on current Claude inbox JSON
  arrays or JSONL mailbox exports
- if historical compatibility readers remain temporarily during deletion work,
  they are not the governing runtime path and must not influence live send/read
  semantics

Repair guidance for operators is documented separately in [`persisted-data-repair.md`](./persisted-data-repair.md).

## 13. Identity And File Policy

### 13.1 Hook Matching

When `ATM_POST_SEND` is set for a configured post-send hook, the payload must
contain:
- `sender`
- `recipient`
- `team`
- `from`
- `message_id`
- `description`
- `task_id` as a string; it may be empty when no task is associated
- `requires_ack`
- `is_ack`
- optional `to`
- optional `recipient_pane_id` when ATM already knows the authoritative pane
  mapping for the recipient

The post-send hook runs only after a successful outbound mailbox write from
`atm send` or `atm ack`. It executes once when recipient matching succeeds,
uses `is_ack = false` for `atm send` and `is_ack = true` for `atm ack`, may
optionally emit one structured stdout result for observability, and never rolls
back a successful message write on failure or timeout.

Hook configuration lookup note:
- send/ack must resolve post-send hook configuration from the sender's
  authoritative ATM roster `home_dir` metadata

Current runtime hook-note:
- once roster and pane mapping truth move to SQLite, the send path should place
  the authoritative recipient pane id into `ATM_POST_SEND.recipient_pane_id`
- post-send hook implementations should prefer that payload field over local
  file rediscovery when it is present
- external hook commands consume `ATM_POST_SEND`
- any retained built-in `atm internal-nudge` helper consumes one separate
  `ATM_INTERNAL_NUDGE` envelope carrying the canonical event, sink target,
  resolved template kind, and resolved template body or explicit disabled
  state; the live production built-in path remains in-process
- retained compatibility helpers must treat committed `.atm.toml` pane ids as
  non-authoritative and use roster/payload pane truth or explicit `--pane`
  only

Supported structured hook-result levels remain:
- `debug`
- `info`
- `warn`
- `error`

### 13.2 Caller Context Resolution

Caller-owned command context is not guessed.

The authoritative command-by-command caller-context matrix lives in
`docs/requirements.md` §4.1.

The accepted command contract is:
- commands that require caller identity resolve it from explicit override when
  supported, otherwise from invoking-shell `ATM_IDENTITY`
- commands that require caller team resolve it from explicit override when
  supported, otherwise from invoking-shell `ATM_TEAM`
- `atm peek` and `atm list` are inspection-only mailbox/message surfaces and
  may inspect another member only through the documented `--as` override path
- `atm send`, `atm read`, `atm ack`, and `atm clear` are owner-only mutating
  surfaces and must not expose caller impersonation
- if required caller context is unavailable, the CLI fails before daemon
  dispatch or retained command execution
- downstream caller-owned request DTOs carry required resolved caller context
  as request data
- the daemon never treats hook files, repo-local config, roster state, or
  daemon ambient `ATM_IDENTITY` / `ATM_TEAM` as fallback caller context
- `atm doctor` is the explicit exception and may run without caller identity
  or caller team while still honoring optional `--team` diagnostic scoping

An obsolete `[atm].identity` field may be diagnosed by doctor, but it must not
control sender/actor resolution.

The accepted mailbox split is explicit:
- `atm peek` inspects one selected message without mutating mailbox state
- `atm list` inspects queue metadata without mutating mailbox state
- `atm read` is the owner-only mutating detail view
- mailbox inspection paths must not change read, seen, or acknowledgement
  state

### 13.3 File Policy

The current `send --file` behavior is retained:
- inspect Claude settings permissions when available
- if the referenced file is allowed, send a direct file reference
- otherwise copy to ATM share storage and rewrite the message body accordingly

## 14. Observability

`atm-core::observability` defines ATM event/query models plus the sealed `ObservabilityPort` boundary.

`atm` provides the concrete `sc-observability` implementation and injects it into core services.

Initialization:
- `atm` initializes logging once at process startup
- `atm` constructs the concrete observability port after startup initialization
- logging failures degrade to best-effort behavior for explicit mail commands

Required ATM event classes:
- command start
- command success
- command failure
- mailbox record skipped

Required ATM event fields:
- command
- team
- actor
- target
- task id
- outcome
- error class when applicable
- stable error code when applicable
- message count when applicable
- transition count when applicable

For explicit observability consumer commands:
- `atm log` depends on shared query/follow APIs
- `atm doctor` depends on shared health APIs
- failures in those consumer paths are command errors, not silently dropped events

### 14.1 Concrete Integration Shape

The retained implementation uses an ATM-owned emit/query/follow/health boundary
that projects shared observability behavior into ATM-owned types:

- ATM-owned `AtmLogQuery`
- ATM-owned `AtmLogRecord`
- ATM-owned `AtmLogSnapshot`
- ATM-owned `AtmObservabilityHealth`
- an ATM-owned synchronous `LogTailSession`

Required boundary responsibilities:

- `ObservabilityPort::emit(...)`
- `ObservabilityPort::query(...)`
- `ObservabilityPort::follow(...)`
- `ObservabilityPort::health(...)`

The exact ATM-owned projected types and object-safe follow-session split are
defined in:
- [`docs/atm-core/design/sc-observability-integration.md`](./atm-core/design/sc-observability-integration.md)

Initial-release boundary rulings:
- this boundary is intentionally ATM-local; it does not attempt to model future
  hook-driven or `schooks`-orchestrated observability concerns
- the health contract remains intentionally closed at:
  - `Healthy`
  - `Degraded`
  - `Unavailable`
- public ATM observability projections must not expose raw
  `serde_json::Value` / `Map<String, Value>` directly
- the concrete `sc-observability` adapter is queue-backed as of Phase `AA.6`;
  ATM uses `Logger::log()` for blocking admission, treats `flush()` /
  `shutdown()` as the only durability barriers, and projects queue/writer/
  maintenance state through ATM-owned health detail rather than leaking raw
  shared types across the public boundary

### 14.2 Shared Crate Usage Rules

Implementation rules:

- `atm-core` remains concrete-crate-neutral and consumes only the injected
  boundary
- `atm` initializes the shared logger exactly once per process
- the shared file sink is the authoritative retained log store for `atm log`
- the default ATM-owned retained log file is in the host-scoped retained-log
  root governed by ADR-011; it is not selected by workspace `ATM_HOME`
- `ATM_LOG_DIR` overrides the exact retained log directory
- without `ATM_LOG_DIR`, the retained log path is derived from that host-scoped
  retained-log root
- under planned ADR-026, the invocation directory and `ATM_HOME` are not
  daemon/socket/lock/database selectors; the OS-user `HostRuntimeScope` owns
  those runtime and durable-state paths, while `ATM_HOME` remains only an
  approved workspace/config discovery input
- the shared console sink remains opt-in so it does not contaminate normal
  command output
- the initial-release dependency is the published crates.io version
  `sc-observability = "1.0.0"`
- the default retained logger baseline must include:
  - daemon lifecycle `info!` events
  - every subsystem `warn!` event
  - every subsystem `error!` event
- the daemon event-emission hot path must not perform per-event retained file
  reopen/append/flush work inline; retained JSONL persistence must cross one
  bounded in-memory queue into a background maintenance worker instead
- the synchronous daemon success path is budgeted for one bounded in-memory
  handoff only; retained logging must not delay request/lifecycle completion on
  file reopen, append, flush, rotate, or prune work
- retained-log rotation and pruning may run only on that background
  maintenance worker, not on the synchronous daemon event-emission path
- retained-log pruning must use a bounded work budget per maintenance tick; it
  must not rely on an unbounded "scan until wall-clock deadline" strategy

### 14.3 Failure Diagnostic Rules

Required diagnostic behavior:

- CLI bootstrap failures must be logged before process exit
- CLI parse/validation failures that occur before a core service runs must be
  logged before process exit
- retained command-service failures must emit structured failure diagnostics
  with stable ATM-owned error codes
- degraded recovery warnings that continue the command must also log stable
  error codes
- command success-only logging is insufficient for the retained architecture

### 14.4 Smoke And Coverage Reporting Architecture

Smoke automation is a repo-owned execution/reporting surface, not an ad hoc
operator script bundle.

Ownership and layout:

- operator skill surface:
  - `.claude/skills/smoke-test/`
- smoke implementation:
  - `scripts/smoke/`
- smoke report templates:
  - `templates/smoke-report/`
- smoke report artifacts:
  - `reports/smoke/`
- coverage implementation:
  - `scripts/coverage/`
- coverage report templates:
  - `templates/coverage-report/`
- coverage report artifacts:
  - `reports/coverage/`

Command architecture:

- `just smoke`
  - defaults to the normal smoke lane
- `just smoke fast`
  - runs the clean-room happy-path lane
- `just smoke thorough`
  - runs the full CLI/checklist lane
- `just test coverage`
  - runs coverage reporting only and must remain separate from plain
    `just test`

Artifact architecture:

- smoke and coverage each keep tracked latest markdown reports
- smoke and coverage each also write gitignored timestamped artifacts using
  the same timestamp convention
- smoke execution produces one canonical JSON payload per run and renders the
  human-readable markdown reports from that payload
- coverage execution produces one canonical JSON payload per host-platform run,
  renders the matching tracked latest markdown report, and leaves the other
  tracked platform report unchanged unless only a placeholder exists
- Linux coverage reporting is an explicit deferred/unsupported platform in the
  current Phase Z line; the coverage runner must fail clearly on Linux instead
  of emitting misleading tracked-latest artifacts

Logging architecture:

- smoke/debug mode may enable detailed lifecycle/send/read/ack/nudge event
  visibility so retained-log analysis can prove the happy path explicitly
- routine production logging remains on the normal retained baseline and must
  not log every ordinary send/read/ack success at default operator verbosity

## 15. Error Model

**Current implementation.** `AtmError` is defined in `atm-storage` as ATM's
sole serializable error contract. The Phase AI HTTP response body uses this
same stable `{ code, message, cause? }` shape; it does not introduce a second
public error model.

Root public error:

```rust
pub struct AtmError {
    code: AtmErrorCode,
    message: String,
    cause: Option<String>,
}
```

```rust
pub enum AtmErrorCode {
    // single central registry re-exported from atm-storage
}
```

Required families:
- config
- missing document
- address
- identity
- team not found
- agent not found
- store
- mailbox read
- mailbox write
- file policy
- validation
- serialization
- timeout
- observability emit
- observability query
- observability health

Every public error must include:
- a stable ATM-owned error code
- a stable class
- human-readable cause
- recovery guidance when the user can act

The single source of truth for ATM-owned error codes is:
- [`atm-error-codes.md`](./atm-error-codes.md)

Persisted-data errors should additionally carry file/entity/parser context so
CLI surfaces can report the exact failing document and scope.

Current runtime error-model rules:
- `AtmErrorCode` must not use wildcard or catch-all variants where a more
  specific code can be named
- every documented `AtmErrorCode` must carry one recoverability classification
  in the central registry so CLI, daemon, and doctor surfaces can reason about
  retry vs operator-action vs fail-closed behavior
- pattern matches over `AtmErrorCode` at module/crate boundary surfaces must be
  exhaustive; wildcard `_` match arms are not permitted

## 16. Trait Policy

The initial rewrite should avoid public extension traits.

If a trait becomes necessary:
- prefer a sealed trait
- verify object safety before stabilization

Current runtime boundary rule:
- all I/O-owning boundary traits are sealed by default
- opening a boundary for external implementation requires explicit design
  review and crate-level documentation of the exception

## 17. Testing Strategy

`atm-core` tests:
- address parsing
- config precedence
- tolerant team-config parsing for compatibility-only schema drift
- precise persisted-data diagnostics for non-recoverable config failures
- bridge hostname resolution for merged inbox reads
- settings resolution
- caller identity precedence and missing-identity rejection
- file policy behavior
- team membership validation
- tolerant inbox parsing
- origin-inbox merge
- atomic append behavior
- duplicate suppression
- read-time duplicate collapse by `message_id`
- workflow axis classification
- workflow axis transitions
- task-linked ack-required classification
- seen-state behavior
- timeout behavior
- ack transition behavior
- clear eligibility behavior
- pending-ack clear override behavior
- observability port emission behavior
- observability port query/filter behavior
- observability port failure behavior
- doctor health projection behavior

`atm` tests:
- clap parsing
- JSON output shape
- human-readable output snapshots
- send/read/ack/clear integration behavior
- `atm log` integration behavior
- `atm doctor` integration behavior
- `atm teams` integration behavior
- `atm members` integration behavior


## 18. Mailbox File Locking (Phase M)

### 18.1 Problem Statement

`append_message` in `mailbox/mod.rs:23-27` performs an unlocked read-modify-write:

1. `read_messages(path)` — reads and deserializes the full inbox
2. `messages.push(envelope)` — appends the new record in memory
3. `atomic::write_messages(path, &messages)` — writes to temp file, fsyncs, renames over original

Step 3 is atomic with respect to partial writes but not concurrent callers. Two concurrent
callers can both complete step 1 before either reaches step 3; the later rename silently
overwrites the earlier, losing its appended message. The same race affects read writeback,
ack transition, and clear set replacement.

### 18.2 Locking Primitive Decision

**Decision: Use the `fs2` crate.**

Rationale:
- `fs2` provides `FileExt::lock_exclusive()` and `FileExt::try_lock_exclusive()` which map
  to `flock(2)` on Unix and `LockFileEx` on Windows
- 98M+ downloads, maintained, compatible with the project's MSRV
- avoids maintaining separate `cfg(unix)` / `cfg(windows)` implementations
- the current `atm-core` Cargo.toml already carries `libc` and `windows-sys`, but
  only as low-level building blocks, not as a cross-platform mailbox-locking API

Alternative rejected: direct `libc::flock` + `windows-sys::LockFileEx` — more control but
duplicates what `fs2` already provides correctly.

### 18.3 Lock Architecture

```
                      +-----------------------+
                      |   MailboxLockGuard     |
                      |  (RAII, Drop releases) |
                      +----------+------------+
                                 |
                      +----------v------------+
                      |   lock.rs::acquire()   |
                      |  open/create sentinel  |
                      |  fs2::try_lock_excl()  |
                      +----------+------------+
                                 |
             +-------------------+-------------------+
             |                                       |
    Unix: flock(fd, LOCK_EX)           Windows: LockFileEx(handle)
```

- **Sentinel**: `{inbox_path}.lock` — pid-bearing runtime artifact, created lazily,
  removed on `MailboxLockGuard` drop, and best-effort evicted when the recorded pid
  is no longer alive
- **Granularity**: per-inbox-file — concurrent sends to different recipients never contend
- **Lock lifetime**: acquired before `read_messages`, held through `atomic::write_messages`
  durability boundary (temp-file write, rename, and any parent-directory sync),
  then the sentinel is unlinked and the guard is released
- **Timeout**: bounded retry loop with `try_lock_exclusive()` + 50ms sleep, default 5s;
  on expiry returns `AtmError { code: MailboxLockTimeout }`
- **Error classification**: only genuine "lock busy" results participate in the
  retry loop. Non-contention I/O and OS failures from the lock path fail fast as
  `MailboxLockFailed` with filesystem/permissions recovery guidance instead of
  being collapsed into a timeout.
- **Cooperative limitation**: `fs2` locks are advisory and only coordinate ATM
  processes that participate in the same locking protocol. Direct file edits or
  other tools that bypass ATM locking are outside the protection boundary. This
  is an accepted limitation for the ATM shared-inbox model.

### 18.3.1 Stale-Sentinel Sweep Predicate

The current `path.extension() == "lock"` filter is too narrow because it misses
rotated sentinels such as `inbox.json.lock.old`. The executed P.10 design must
match only filenames that still carry the sentinel suffix chain:

```rust
let is_lock_sentinel_candidate = path
    .file_name()
    .and_then(|name| name.to_str())
    .is_some_and(|name| name.ends_with(".lock") || name.contains(".lock."));
```

Why this exact predicate:
- `ends_with(".lock")` preserves the ordinary live sentinel path
- `contains(".lock.")` catches rotated forms such as `.lock.old` and
  `.lock.replaced`
- basename-only matching avoids broad false positives from parent directories
- rejecting generic `contains("lock")` avoids matching unrelated files such as
  `locksmith.txt`

Eviction remains conservative:
- read the candidate contents as the documented `pid[:token]` owner record
- if parsing fails, leave the file in place
- if `process_is_alive(pid)` is true, leave the file in place
- only then attempt removal

This is still best-effort cleanup, not a second ownership protocol. The actual
authority boundary remains the later `fs2` advisory lock plus the existing
`lock_path_matches_file(...)` identity recheck after acquisition.

Platform note:
- Windows may not permit renaming a live locked sentinel the same way Unix
  does, so the broadened sweep is not a live-handoff mechanism
- the predicate exists to clean up crash leftovers, repair leftovers, or
  externally rotated sentinel artifacts that otherwise evade the old exact
  `.lock` extension test

### 18.3.2 Read-Only Filesystem Classification

P.10 should add a dedicated read-only-filesystem mailbox-lock code instead of
overloading the generic non-contention lock failure bucket.

Required platform mapping:
- Linux: `libc::EROFS` (`30`)
- macOS: `libc::EROFS` (`30`)
- Windows: `windows_sys::Win32::Foundation::ERROR_WRITE_PROTECT` (`19`)

The classification helper belongs at the lock-path error-conversion boundary,
not duplicated ad hoc at individual call sites. The intended shape is:

```rust
fn is_readonly_filesystem_error(error: &io::Error) -> bool
```

and then a shared mapper such as:

```rust
fn mailbox_lock_path_error(
    operation: &'static str,
    lock_path: &Path,
    error: io::Error,
) -> AtmError
```

Call-graph decisions:
- `open_lock_file(...)` maps read-only failures directly to
  `MailboxLockReadOnlyFilesystem`
- `write_lock_owner_record(...)` maps both truncate and write failures through
  the same helper
- `remove_lock_sentinel_with_retry(...)` explicitly does not retry read-only
  failures before the current permission-denied/backoff logic
- public `sweep_stale_lock_sentinels(...)` surfaces the read-only diagnostic to
  the caller rather than logging and continuing
- pre-acquisition stale eviction inside `acquire(...)` propagates the
  read-only diagnostic when the cleanup path hits it, because subsequent owner
  record writes cannot succeed on the same mount; this early-exit happens
  before any later `try_lock_exclusive()` attempt
- each retry iteration must classify raw OS errors before consulting the
  timeout budget: `EROFS` / `ERROR_WRITE_PROTECT` exits immediately as
  `MailboxLockReadOnlyFilesystem`, while non-contention path failures such as
  `ENOSPC`, `EMFILE`, and `ESTALE` exit immediately as `MailboxLockFailed`
- `MailboxLockGuard::drop` still warns only, because the successful mailbox
  mutation has already completed and `Drop` cannot change the command result

Recommended recovery text:
- message includes the attempted operation and lock path
- recovery tells the operator to remount or move the ATM home to a writable
  filesystem before retrying, not merely to wait for another process

Reason for a new code instead of enriching `MailboxLockFailed`:
- read-only filesystem state is a stable, operator-actionable class with
  different remediation from ACL failures or transient path I/O
- the retry policy must branch on this distinction
- QA and integration tests need a stable machine-readable contract for it

### 18.4 Integration: Single-File Helper + Multi-File Lock Set

`append_message` is a true single-file read-modify-write and should use one shared helper:

```rust
pub fn locked_read_modify_write<F>(
    path: &Path,
    timeout: Duration,
    mutate: F,
) -> Result<(), AtmError>
where
    F: FnOnce(&mut Vec<MessageEnvelope>) -> Result<(), AtmError>,
{
    let _guard = lock::acquire(path, timeout)?;
    let mut messages = read_messages(path)?;
    mutate(&mut messages)?;
    atomic::write_messages(path, &messages)
}
```

That helper is the right shape for:
- `append_message`
- the missing-config team-lead notice path, because it also calls `append_message`

It is **not** sufficient by itself for `read`, `ack`, and `clear`, because those
commands call `load_source_files(...)` and compute a merged surface across the
requested inbox plus any origin inboxes before writing back. To make those paths
concurrency-safe, Phase M needs a second abstraction:

```rust
pub fn acquire_many_sorted(
    paths: impl IntoIterator<Item = PathBuf>,
    timeout: Duration,
) -> Result<Vec<MailboxLockGuard>, AtmError>
```

Required usage:
- discover the full source-file set first
- dedupe paths and sort them deterministically by canonical path string
- source-file discovery must finish before the first inbox read
- legitimately absent inbox paths at discovery time are excluded from the lock
  set rather than locked speculatively
- source discovery must fail closed for mutation commands: unreadable
  `read_dir(...)` entries or equivalent enumeration faults are treated as source
  set instability, not as warnings that can be skipped
- source discovery faults abort the command before lock acquisition; mutation
  commands never attempt a partial lock set after a discovery failure
- acquire all locks against one total timeout budget
- if any acquisition fails, drop every earlier lock immediately and abort before
  any source-file read
- if a discovered file disappears or becomes unreadable after lock planning but
  before `load_source_files(...)` completes, abort without persisting any
  partial state; this remains a normal operator-actionable file-read failure,
  not a partial-lock degraded mode
- then call `load_source_files(...)`
- hold every guard until every source writeback completes

This intentionally preserves a single logical merged-surface decision boundary
for `read`, `ack`, and `clear`. Those commands are not allowed to degrade into
partial-lock best-effort mutation, because doing so would mix snapshots from
different logical times and make writeback correctness nondeterministic.

### 18.4.1 Cooperative Locking Contract For `ack_mail`

`ack_mail` sometimes needs to mutate a source inbox set and append the reply to
another inbox that was not part of the initial actor-source set. The accepted
implementation does not use a subset-lock then upgrade-to-superset sequence.
Instead it uses:

1. an unlocked observational snapshot of the actor-source set
2. unlocked validation of the pending-ack state and reply inbox path
3. one final acquisition of the full sorted superset that includes the reply
   inbox
4. re-discovery of source paths, reload of current source files, and
   re-validation of the pending-ack state under that final lock set
5. persistence of both the updated source message and reply while the superset
   locks are still held

This avoids the deadlock risk of trying to expand a held subset into a larger
sorted lock set. The unlocked preflight is acceptable only because `ack_mail`
does not mutate from that preflight snapshot: the shared commit helper reloads
and re-validates both the source-path set and the pending-ack state under the
final superset lock before writing anything. If the state drifted, `ack_mail`
aborts instead of mutating a stale snapshot.

| Caller | Lock required |
|--------|--------------|
| `append_message` | `locked_read_modify_write` |
| `send` missing-config notice append | `append_message` coverage |
| source discovery fault (`read` / `ack` / `clear`) | abort before lock acquisition; no partial lock set attempted |
| `read` writeback | initial selection load is unlocked; acquire the multi-file lock set only for the reload + writeback phase |
| `ack` transition + reply | unlocked preflight, then one final cooperative superset lock including reply inbox; see §18.4.1 |
| `clear` set replacement | multi-file lock set held from first read through persist |
| `read_messages` (read-only, no writeback) | No |

### 18.4.2 Read-Only Vs Read-Modify-Write

ATM now treats mailbox access as two distinct patterns:

1. Read-only snapshot:
   - discover source inbox paths
   - load and classify the current merged surface without mailbox locks
   - use this for display-only selection and timeout polling

2. Read-modify-write:
   - re-acquire the deterministic source lock set only when a command is about to
     persist mailbox state
   - re-discover and re-validate the source path set under lock
   - reload the mailbox state, recompute selection, apply transitions, and
     persist while the lock set is still held

This keeps non-mutating reads out of the lock path while preserving a stable
writeback boundary for commands that actually rewrite inbox files.

Executed command mapping:
- `read` uses an unlocked observational snapshot for display selection and
  timeout polling, then enters the shared lock+reload+recompute path only when
  display-state mutation is actually required
- `ack` uses an unlocked preflight to resolve the reply target and candidate
  source message, then acquires one final sorted superset lock and re-validates
  the pending-ack state under that lock set before writing source/reply state
- mutating `clear` acquires the shared lock plan before its mutating reread and
  holds it through removal computation, mailbox replacement, and workflow-state
  updates; `clear --dry-run` remains observational only

### 18.4.3 Executed Mailbox Workflow Migration

Phase P completed the mailbox workflow-state migration. P.4 delivered the
sidecar move, and the current architecture documents the post-P.5 executed
state.

Current executed rule:
- ATM-owned mailbox workflow durability lives in SQLite-backed state.
- mutable per-message mailbox state is owned by the explicit
  `mail_message_states` table rather than split visibility/ack storage.
- `send` commits durable message/content state first.
- `read`, `ack`, and `clear` read and mutate the SQLite-backed state model.
- Claude inbox export is a compatibility projection only.

Current executed requirement:
- filesystem workflow sidecars are retired; SQLite is the exclusive mailbox
  state authority and legacy `.atm-state/workflow` files are ignored.

Unified-state ownership notes:
- `mail_messages` keeps immutable content only
- `mail_message_states` owns mutable mailbox/runtime state behind the
  `message_key` foreign-key relationship
- `expires_at` moved out of `mail_messages` and now lives only on
  `mail_message_states`
- deleted-row visibility is admin-only; normal list/read/count queries must
  exclude rows with `deleted_at`
- the earlier split model (`mail_visibility_states` plus `ack_state`) is
  retired and must not be reintroduced under new names

### 18.4.4 Phase U Provenance Reduction

Phase U removed weak round-trip provenance from the durable mailbox contract.

Current executed rule:
- `imported_from` is removed from `MailStoreMessageRecord` durable truth and is
  no longer part of the mailbox-row schema
- `recorded_at` remains SQLite-owned ingest timing in `atm-rusqlite`, not
  caller-supplied message data

Governing ADR:
- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`

### 18.5 New Error Codes

- `MailboxLockFailed` / `ATM_MAILBOX_LOCK_FAILED` — lock-path creation,
  open, or acquisition failed for a non-contention filesystem or OS reason
- `MailboxLockReadOnlyFilesystem`
  / `ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM` — the lock path or lock sentinel
  lives on a read-only filesystem, so ATM cannot create, update, or remove the
  required mailbox-lock artifact
- `MailboxLockTimeout` / `ATM_MAILBOX_LOCK_TIMEOUT` — lock not acquired within timeout
- New `AtmErrorCode::MailboxLock` code in the central registry

### 18.6 Shared Mutable File Atomicity

Mailbox locking closes the concurrent lost-update race for inbox files, but it
is only one part of the persistence contract. Phase M also treats atomic file
replacement as a repo-wide rule for shared mutable ATM-owned structured state.

Scope:
- live inbox files
- team `config.json`
- ATM-owned task-bucket files restored or rewritten by team recovery
- `.highwatermark`
- shared persisted coordination/state files such as send-alert or
  restore-progress markers when they carry ATM-owned operator state
- any future ATM-owned JSON/JSONL/state file rewritten by more than one ATM
  process or operator workflow

Architectural rule:
- no live shared mutable structured file may be rewritten in place
- writers must use a temp-file + fsync + rename style replacement on the same
  filesystem, or a documented equivalent with the same atomicity guarantee
- for rename-based replacement, the helper must also fsync the parent directory
  after the rename whenever the platform supports directory-sync semantics; this
  is the Phase M crash-durability boundary for mailbox/config/shared-state
  replacement
- `atm-core` must own one shared low-level atomic persistence primitive and a
  small set of typed writer helpers layered on top of it, rather than open-code
  file replacement logic at individual call sites
- existing helpers such as `atomic::write_messages(...)` and
  `write_team_config(...)` are the preferred integration points; new shared
  state added by Phase M should extend that helper pattern with typed helpers
  for task-bucket, highwatermark, and shared coordination files instead of
  open-coding direct `fs::write(...)` mutations

Single-write-path guardrail:
- each live file family should have one owning write boundary
- low-level atomic replacement belongs in `persistence.rs`
- file-family semantics belong in one owner-layer helper such as mailbox or
  team-admin
- command handlers should express intent and call the owner-layer helper rather
  than assemble write mechanics locally
- if a new write precondition appears, the default response should be to extend
  the shared helper or owner-layer helper rather than introducing a parallel
  write path

Current owner-layer boundaries:
- Historical Claude-owned inbox compatibility surface:
  AD.3 retired the Claude inbox append backend and the old nudge/context
  injection path. The retained mailbox commands now cross the
  `RetainedServiceRuntime` seam and delegate through injected store adapters;
  low-level source-file discovery, lock/reload orchestration, and persistence
  remain internal leaf helpers behind that seam during the Phase R store
  transition.
- ATM-owned source-of-truth state:
  `workflow::{load_workflow_state(...), save_workflow_state(...),
  project_envelope(...), remember_initial_state(...),
  apply_projected_state(...), remove_message_state(...)}`,
  `read::seen_state::save_seen_watermark(...)`,
  `send::alert_state::{register_missing_team_config_alert(...),
  clear_missing_team_config_alert(...), save(...)}`, and
  `team_admin::write_team_config(...)`
- ATM-owned restore/task state:
  `team_admin::restore::restore_task_state_from_backup(...)`,
  `team_admin::restore::write_restore_marker(...)`, and
  `team_admin::restore::clear_restore_marker(...)`
- staging/scratch artifacts:
  `team_admin::restore::prepare_restore_workspace(...)` and
  `team_admin::restore::cleanup_restore_workspace(...)`

Current architectural limitation:
- mailbox replacement is atomic and lock-coordinated for concurrent ATM
  writers, but it is not yet compare-and-swap against non-cooperating Claude
  writers
- therefore the current shared-inbox rewrite path is still a compatibility
  boundary, not the ideal long-term source-of-truth architecture for ATM-local
  workflow state
- separately, send-side workflow seeding still lacks a dedicated freshness
  boundary across concurrent same-recipient sends; that is a post-P.5 hardening
  gap rather than a reason to move workflow durability back into Claude-owned
  inbox records

This rule intentionally applies beyond mailbox files so future work does not
reintroduce partial-write or torn-state risks through backup/restore or shared
auxiliary state paths.

### 18.6.1 Deterministic Locking-Test Strategy

The follow-up locking fixes require failure-path tests, but those tests must not
depend on races or hang-prone construction.

Test strategy:
- contention tests use a helper thread/process that acquires the target lock and
  signals readiness through a channel or barrier
- the command under test uses a short bounded lock timeout
- assertions use `recv_timeout(...)`, elapsed-time ceilings, and scoped guard
  teardown instead of indefinite `join()`/sleep loops
- source-discovery fault tests use a deterministic seam (for example, an
  injected directory-entry iterator/fault source) to force an unreadable origin
  entry without depending on filesystem timing or permission quirks
- non-contention lock error tests use a deterministic seam around the lock
  attempt/classifier rather than trying to synthesize platform-specific OS
  failures opportunistically
- durability tests validate helper sequencing and error propagation through
  deterministic seams; they do not attempt literal crash simulation in unit or
  integration test runs

This is intentionally stricter than the Phase M success-path deadlock tests so
CI remains bounded and repeatable across macOS, Linux, and Windows.

## 19. Restore Transaction Atomicity (Phase M)

### 19.1 Problem Statement

`restore_team` in `team_admin.rs` currently mutates in this order:
1. Copy inbox files to the live inbox directory
2. Restore task bucket
3. Recompute highwatermark
4. Write `config.json`

If the process crashes between steps 1 and 4, inbox files for members not in config
exist with no detection mechanism.

### 19.2 Revised Restore Ordering (Config-Last with Staging)

```
1. Validate backup and compute restore plan (no mutations)
2. Write .restore-in-progress marker to team directory
3. Stage inbox files to .restore-staging/inboxes/
4. Move staged files to live inboxes/ (fs::rename — atomic same-filesystem)
5. Restore task bucket
6. Recompute highwatermark
7. Write config.json + fsync (atomic temp+rename via write_team_config)
8. Remove .restore-in-progress marker
```

Key properties:
- crash at steps 2-6: config.json unchanged, extra inbox files harmless, marker signals re-run
- read-only failure during the pre-copy stale-sentinel sweep aborts before live
  inbox replacement begins, preserving the pre-restore team state
- crash at step 7: config write is itself atomic via the existing `write_team_config(...)`
  temp-file + rename path, so no partial config write is possible
- crash at step 8: config is written, stale marker cleaned up by next doctor/restore run

### 19.3 Staging Directory

- location: `{team_dir}/.restore-staging/inboxes/`
- lifecycle: created at step 3, contents moved at step 4, directory removed after config write
- failure path: staging directory cleaned up, no config written

### 19.4 Doctor Integration

New check: scan for `.restore-in-progress` in team directories.
- Severity: warning
- Recovery guidance: "A previous `atm teams restore` was interrupted. Re-run the restore
  command to complete it, or remove the marker file manually if the restore is no longer needed."

If `.restore-staging/` already exists at restore start, the implementation must
either clean it before staging begins or fail with actionable recovery text.
It must never merge old staging contents with the new restore attempt.

## 20. Phase M Minor Architecture Changes

### 20.1 AtmError Display Backtrace

`AtmError` keeps the user-facing `Display` output concise:

- `Display` renders only the primary message and recovery text
- captured backtraces stay available through Debug output and a dedicated
  accessor on `AtmError`

This avoids multi-kilobyte backtrace blobs in normal CLI/log output while
preserving full diagnostic depth for explicit debugging.

### 20.2 resolve_actor_identity Consolidation

Duplicate function in `ack/mod.rs`, `clear/mod.rs`, and `read/mod.rs` moves to
`identity/mod.rs` as `pub(crate) fn resolve_actor_identity(...)`. All three call sites
update to use the shared helper while preserving the existing override -> runtime-env
identity resolution order.

### 20.3 normalize_json_number Panic Removal

`normalize_json_number(...)` must not panic on untrusted numeric text. Phase M
replaces the old panic path with graceful fallback: on exponent parse failure or
unsupported exponent range, return the raw string unchanged and emit `tracing::warn!`.
A library function must not panic on potentially untrusted input.

### 20.4 Error-Surface Audit Methodology

Phase M uses an explicit audit methodology for `REQ-CORE-ERROR-DOC-001` and
`REQ-CORE-ERROR-RECOVERY-001` so signoff does not depend on ad hoc review.

Method:
- grep the production source tree for `expect(` and bare `AtmError`
  construction sites
- review the resulting inventory manually against the explicit Phase M audit
  inventory in the sprint plan
- exclude:
  - test-only code
  - `#[cfg(test)]` modules embedded in production files
  - intentional invariant assertions that do not represent operator-actionable
    failures
- keep the remaining production-path sites in scope for either:
  - `# Errors` documentation updates
  - `.with_recovery()` additions
  - panic removal or other structural correction when the failure mode is not
    acceptable in library code

The initial planning audit identified 16 production-path `expect(...)` sites
requiring review under this methodology. Phase M treats that number as a
starting inventory, not as a substitute for a fresh grep during implementation.

### 20.5 Phase L.7 Build-On Notes

Phase M builds on the already-landed L.7 runtime surface
(`team_members`, `aliases`, `post_send_hook`, doctor identity drift warning).
Phase M does not re-open that feature set; it only adds the remaining concurrency,
restore, and code-review hardening needed for 1.0.

### 20.6 Security Boundaries (Phase O)

Phase O adds three architecture-level hardening decisions:

1. **Address validation is the trust boundary for path construction**
   - team and agent names must be validated before any helper constructs
     `{ATM_HOME}/.claude/teams/{team}` or `{agent}.json`
   - `address.rs` and `home.rs` together form the boundary; downstream code
     must not attempt ad hoc sanitization after path joins are already built

2. **PID-file locking remains conservative by design**
   - the send-alert lock uses a PID-file-style stale-lock check
   - PID reuse is an accepted limitation: a reused PID can make a stale lock
     look alive, so ATM may conservatively preserve that stale lock until
     timeout or manual cleanup
   - this limitation favors false-alive availability loss over false-dead lock
     eviction

3. **Atomic writes must use collision-proof temp names**
   - temp files for atomic replacement must use ULID-based or equivalently
     collision-proof non-UUID suffixes instead of timestamp-only suffixes
   - this keeps same-process rapid writes to the same target path from
     colliding on the temp-file name while preserving the target basename for
     operator debugging

## 21. Current SQLite Runtime Architecture

The current SQLite/daemon architecture supersedes the mailbox-lock architecture as the target design for ATM
mail correctness. The file-based mailbox line remains an interim compatibility
surface only.

### 21.1 Authoritative State

ATM moves to a split state model:

- SQLite is the authoritative durable store for:
  - messages
  - ack/task state
  - read/clear/delete message state
  - team roster
- daemon memory is the authoritative live runtime view for:
  - current agent status
  - `pid`: transient daemon-owned process identity cached as the primary
    liveness field
  - `last_active_at`: daemon-memory-only runtime state used for live overlays

SQLite may persist last-observed status for diagnostics, but that snapshot is
not the live truth.

### 21.1.1 SQLite Schema Contract

The Phase R first implementation uses one authoritative schema contract with
concrete SQLite table names:

- `mail_messages`
  - logical durable message store
  - stores the full `MessageEnvelope` in `envelope_json`
  - also stores queryable message columns:
    - `from_agent`
    - `message_text`
    - `summary`
    - `message_at`
    - compatibility `message_id`
- one canonical mutable message-state table
  - logical `message_state` projection
- `mail_ingest_replay_states`
  - logical `inbox_ingest` replay/high-water projection
- one canonical roster/member table
  - per-member durable projection keyed by `(team_name, agent_name)`

Minimum key rules:
- `message_key` is the canonical ATM durable message identity
- `message_key` must be source-typed:
  - `atm:<ulid>` for ATM-authored rows
  - `ext:<fingerprint>` for imported external rows without ATM ids
- retained `message_id` is the ULID text encoding of the one logical ATM
  message identity

Minimum index/constraint rules:
- unique identity enforcement on `message_key`
- dedupe index for imported external/compatibility identities
- one-successor enforcement on `(team, agent, parent_message_id)` for threaded
  update chains
- lookup indexes for:
  - recipient/team mailbox projection
  - task lookup
  - message-state projection
  - ingest replay/high-water tracking

Minimum canonical roster-member durable fields:
- `team_name`
- `agent_name`
- `member_kind`
- `harness`
- `agent_type`
- `model`
- `metadata_json`
- `recipient_pane_id TEXT NULL`
  - authoritative post-send-hook pane mapping when known

The canonical harness values are `claude-code`, `codex-cli`, `gemini-cli`,
`opencode`, `hermes`, and `python-graft`. `hermes` is the named Hermes Python
gateway integration; `python-graft` is the generic value for any Python host
that receives messages through the `atm-graft` interface. Both Python graft
harnesses use the non-Claude delivery path and do not require a tmux pane.

`pid` is not part of the canonical roster-member durable schema. It remains
transient daemon-owned runtime state only.

Schema-governance rule:
- any SQLite schema change is a contract change
- schema changes require explicit user approval plus synchronized requirements,
  architecture, and boundary doc updates before implementation is accepted

### 21.1.2 SQLite Runtime Invariants

The SQLite runtime contract is part of the architecture, not an implementation
detail.

Required invariants:
- `journal_mode = WAL`
- `foreign_keys = ON`
- schema bootstrap is deterministic, idempotent, and runs once per database
  root before normal command operations use the durable store
- per-operation connection acquisition may reapply runtime pragmas, but must
  not rerun full schema bootstrap on every connection use
- mutating ATM flows use explicit transactions
- no normal command path relies on implicit autocommit as its correctness
  model

### 21.1.3 Crash Recovery

Crash recovery preserves the committed local mailbox. The daemon owns no
remote replay store, deferred outbox, or retry state.

### 21.2 Compatibility Surfaces

Historical Claude-owned shared inbox compatibility previously existed for:
- direct Claude-native writer interoperability
- the prior shared `.json` inbox container shape, whose file container was one
  top-level JSON array of inbox messages

Phase `AD` rule:
- Phase `AD.3` completes retirement of Claude context injection through inbox
  append per `ADR-019`
- no accepted runtime path requires Claude-owned shared inbox files

Architectural rule:
- Claude inbox-append runtime behavior and the former
  `crates/atm-storage-claude` backend are retired from the accepted line
  because Claude Code no longer uses them
- durable SQLite state is ATM's authoritative mail state
- send/ack must not depend on Claude `.json` or `.jsonl` mailbox writes
- the shared backend contract remains required so SQLite stays one backend
  implementation rather than becoming the architecture
- any surviving compatibility readers or repair paths are historical/deletion
  work only and must not redefine current runtime behavior

`config.json` remains a team-ingress surface, but roster truth moves to
SQLite.

### 21.2.1 Delivery Policy Placement

Historical compatibility-export policy and current notification policy must not
remain scattered through command code.

Architectural rules:

- one central delivery-policy coordinator dispatches write-affecting events by:
  - event family
  - canonical roster `harness`
- the coordinator is not a universal mail state machine; it is a dispatcher
  and policy gate
- event legality remains in dedicated event-family state machines
- at minimum the runtime must model:
  - `NewMessageStateMachine`
  - `ThreadUpdateStateMachine`
- `NewMessageStateMachine` must expose two auditable harness paths:
  - Claude harness
  - non-Claude harness
- `ThreadUpdateStateMachine` remains separate because supersede/update legality
  differs materially from standalone send
- write-affecting transitions must emit observable transition records

### 21.3 Information Flow

There are three distinct paths:

1. Claude / compatibility path
   - on the earlier compatibility line, Claude inbox files used one top-level
     JSON-array mailbox document as the shared compatibility shape
   - historical Claude `.json` inbox writes used atomic full-document
     replacement: load existing array, append, write replacement via temp-file
     + rename
   - healthy historical Claude `.json` inboxes previously stayed on that
     compatibility path and did not require repair/rebuild warnings
   - historical ATM-owned `.jsonl` compatibility projections were append-style
     only where ATM explicitly owned that export surface
   - `rebuild_compat_inbox_projection(...)` is reserved for explicit
     malformed-state repair/rebuild and is not part of the ordinary send/ack
     write path
   - ATM imports through one owned inbox-ingress boundary
   - imported records become durable in SQLite
   - replay is idempotent and parseable rows are not silently dropped
   - ATM-authored oversized-body exports replace compatibility-surface `text`
     with exactly `atm read --message-id <id>` while keeping the full body
     durable in SQLite

2. Native agent path
   - native agent/plugin traffic does not use JSONL
   - native agents talk to the local daemon API
   - the daemon commits through the SQLite store boundary

### 21.3.1 New-Message Failure Contract

The accepted daemon + SQLite runtime keeps one direct post-persist rule for new
messages.

Architectural rules:

- send success is durable ATM persistence
- after persistence, ATM may emit one post-send effect when the recipient
  exposes that capability
- the shipped default emitter path is the receiver-only
  `MessageReceivedHookEmitter` delivery path
- the built-in renderer selects exactly one of six named template kinds:
  `delivery`, `delivery_ack`, `delivery_task`, `delivery_task_ack`,
  `acknowledge`, and `acknowledge_task`
- any team-scoped built-in template override row must be resolved through the
  storage-neutral `NudgeTemplateOverrideStore` contract before the built-in
  emitter/render path runs; `atm` and `atm-core` must not perform direct
  SQLite lookup for this feature, and any retained `atm internal-nudge`
  helper must not reopen the lookup after it receives resolved input
- the authoritative Phase AD post-send smoke lane is fixed to five closure
  cases only:
  - external hook success
  - external hook partial failure
  - built-in fallback across both tmux and graft sinks
  - override reset-to-default after deleting a prior stored override row
  - explicit disable behavior when the retained design keeps that state
- resolved built-in template lifecycle is explicit: no row => product
  default, override row => stored text, disabled row => no emission,
  clear/reset => row deletion
- external `[[atm.post_send_hooks]]` commands remain the explicit full-override
  path
- post-send emission failure is logged and returned as a sender-visible warning
- post-send emission is not durable message delivery and does not redefine send
  success
- the accepted compact built-in acknowledge forms are:
  - `<atm kind="ack" from="..." message-id="..."/>`
  - `<atm kind="ack" from="..." message-id="..." task-id="..."/>`
- the accepted seam is a dedicated post-send emitter with optional direct
  notification-log append at the event site, not
  `DeliveryPlan`/`NotificationSink` or a daemon-owned notification
  worker/runtime

### 21.4 One Same-Host Interface

ATM uses one same-host daemon API plus one test transport:

- same-host target: HTTP over Unix UDS or loopback TCP; Windows loopback TCP
- tests: in-process `test-socket`

This is one protocol with multiple implementations, not multiple systems.

Supported-platform parity rule:
- same-host daemon functionality is not complete until the Unix and Windows
  implementations both satisfy the same retained product behavior
- platform-specific implementation differences are allowed only in:
  - same-host local IPC adapter internals
  - lifecycle-control source adapter internals
  - host-ownership adapter internals
- business logic, dispatcher routing, replay/state handling, health
  projection, and runtime-lane behavior must not diverge by operating system
- compile-only support or typed unsupported-path stubs are acceptable only as
  temporary implementation states and must not be documented as final support

Test-transport rule:
- `test-socket` implements the same dispatcher/handler contract without real
  socket I/O so subsystem and daemon-boundary tests can exercise the transport
  boundary in process

### 21.5 Singleton Daemon

The daemon is required at runtime, but it must remain thin.

Hard invariant:
- it must be impossible for two active ATM daemons to run on one host at the
  same time

Daemon responsibilities:
- transport listeners
- route selection
- live status cache
- daemon-facing diagnostics and health queries used by `atm doctor`
- direct post-send emission routing

Daemon non-responsibility:
- it must not become the only home of ATM business logic

Auto-start path:
- production ATM commands first attempt to connect to the already-running
  daemon
- if the daemon is absent, the CLI/runtime path may perform exactly one
  auto-start attempt
- after one auto-start attempt, the CLI/runtime path retries connect once
- daemon startup waits at most `10s` for control-state publication
  (`AUTO_START_PUBLISH_TIMEOUT`)
- if the daemon remains unavailable, the command fails with a typed actionable
  error
- there is no silent fallback from the production path to direct SQLite or
  inbox-file access after auto-start failure

### 21.6 Strict I/O Ownership

The current runtime's key architectural rule is strict ownership of all external I/O.

Required ownership model:
- only the store subsystem touches SQLite
- only the inbox ingress/export subsystem parses or writes inbox JSONL
- only the config-ingress subsystem parses team `config.json`
- only the transport subsystem touches sockets
- only the notifier/plugin subsystem talks to agent processes

This is the architectural mechanism intended to prevent the boundary leakage
that made the old daemon line unmaintainable.

Privacy rule:
- each boundary must expose only the trait or façade needed by callers
- concrete implementations, helper constructors, and storage/transport details
  stay private to the owning module unless a later crate extraction makes the
  boundary stricter

### 21.6.1 Boundary Shapes

Each I/O-owning subsystem needs one explicit architectural boundary.

#### MailStore

Dispatch model:
- synchronous request/response from service code
- transaction-scoped mutating calls

Object-safety rule:
- callers depend on an object-safe store trait or façade, not concrete SQLite
  types

Minimum method set:
- open/bootstrap store
- run transaction
- upsert/load message rows
- upsert/load unified message state
- record/load ingest replay state
- return health/readiness snapshot

Scope rule:
- `MailStore` owns message rows plus unified read/ack/delete/expiry state tied directly to
  message lifecycle
- `MailStore` is not the long-term owner of generic task-orchestration or
  daemon-status domains

#### Task Storage (Deferred)

Phase `AC` closeout note:
- speculative `TaskStore` and `TaskStoreDoctor` surfaces were deleted in
  `AC.6`
- future task storage is out of scope for the current shared storage contract
- if approved later, task storage starts from canonical Claude-code task
  schema plus Pydantic validation rather than from preserved transition
  scaffolding

#### RosterStore

Dispatch model:
- synchronous request/response for roster replacement, lookup, and readiness
  checks

Object-safety rule:
- callers depend on an object-safe roster-store trait or façade, not concrete
  SQLite types

Minimum method set:
- replace/load canonical roster member rows
- query roster membership for routing/validation
- return roster health/readiness snapshot

Ownership rule:
- runtime `pid` continuity is transient daemon-owned state and must not become
  part of durable roster truth
- `config.json` remains an ingress document, not a general runtime-read truth

#### SourceIngress

Dispatch model:
- batch import from one changed inbox source

Object-safety rule:
- callers depend on an object-safe ingress trait or façade, not direct JSONL
  parser structs

Minimum method set:
- import changed inbox source
- compute canonical imported identity/fingerprint
- report degraded/skipped rows with structured diagnostics

#### ProjectionExport

Dispatch model:
- one-way export / re-export after durable commit

Object-safety rule:
- callers depend on an object-safe export trait or façade, not direct file
  writer implementations

Minimum method set:
- export ATM-authored Claude-compatible record
- re-export by durable `message_key`
- return typed export failure / retry-needed result

#### Transport

Dispatch model:
- request/response for same-host daemon traffic
- the same dispatch contract must also support the in-process `test-socket`
  transport used by tests

Object-safety rule:
- callers depend on an object-safe transport trait or façade so the local
  adapter remains replaceable by the test transport

Minimum method set:
- serve local daemon API
- query daemon health
- shut down listener/connection set gracefully
- construct or bind an in-process `test-socket` endpoint for transport-boundary
  tests

Dispatcher rule:
- transport hands off to one injected dispatcher boundary
- the dispatcher owns request-kind routing only
- request-family behavior lives in injectable handlers behind that dispatcher
- adding a new request type must not require embedding business logic into
  local-IPC adapter code

Socket receive loop rule:
- the receive loop must stay intentionally small
- allowed responsibilities:
  - read one framed request
  - parse it into a qualified request enum/value
  - validate/authenticate the transport envelope
  - dispatch immediately to the owning handler boundary
  - serialize one typed response
- forbidden responsibilities inside the receive loop:
  - direct SQL/store logic
  - background watch/reconcile logic
  - direct receiver-side post-send handling logic
  - embedded workflow/business-state transitions

#### Dispatcher

Dispatch model:
- qualified request -> handler routing inside the daemon/runtime service layer

Object-safety rule:
- transport adapters depend on an object-safe dispatcher trait or façade, not
  on concrete request-family handler implementations

Minimum method set:
- dispatch parsed request to the correct request-family handler
- return one typed response or typed error

Boundary rule:
- dispatcher owns routing, not business logic
- request-family behavior lives in injectable handlers behind the dispatcher
- adding a new request family should be an additive handler/registration
  change, not transport-adapter logic growth

#### Watch / Reconcile

Watcher/reconcile is historical only.

Architectural rule:
- the accepted runtime does not own a daemon watch/reconcile subsystem
- no retained send/read/ack path may depend on watcher events, debounce, or
  reconcile completion

#### Plugin / Notifier

Dispatch model:
- one-way notification plus status-reporting callbacks

Object-safety rule:
- callers depend on an object-safe notifier/plugin boundary, not agent-specific
  concrete implementations

Minimum method set:
- notify message/task delivery
- report live status update
- return typed backpressure / unavailable results

Current implementation note:
- the historical `R.17` daemon-owned queued notifier worker was retired by
  `AD.5`
- the accepted runtime must not require a daemon notification queue/worker just
  to append one post-send event or warning
- if notification logging survives, it is a direct append at the event site
  rather than a retained daemon-owned worker subsystem

### 21.6.2 Structured Error And Observability Boundaries

The current runtime must keep production failure handling and observability
structured at compile time.

Architectural rules:
- fallible production paths return typed `Result` / discriminated error enums
  across crate boundaries rather than relying on panic or unwrap
- pattern matches over `AtmErrorCode` at module/crate boundary surfaces must be
  exhaustive; wildcard `_` match arms are not permitted
- adapter layers may translate errors, but must preserve structured identity
- when reviewing transitional compatibility paths, apply these structured-error
  rules together with the pre-Phase-Q pipeline stage lists and their
  supersession notes; see Sections 8 and 9 for the Ack and Clear pipeline stage
  lists and the inline notes that supersede them under the current runtime
- SQLite-specific transaction, busy-timeout, shutdown-checkpoint, and
  `rusqlite` blocking-I/O rules are defined in
  [`docs/atm-rusqlite/architecture.md`](./atm-rusqlite/architecture.md)
  Sections 4, 5, and 6 and are part of this same current-runtime error boundary
- `atm` owns CLI-side `sc-observability` bootstrap and CLI event emission
- `atm-daemon` owns daemon/runtime/transport `sc-observability` emission
- `atm-core` owns ATM event and error models above the shared observability
  boundary
- daemon-side observability remains bottom-of-stack:
  - the shared daemon observability layer imports no daemon subsystem types
  - daemon subsystems emit typed daemon event payloads through a sealed,
    object-safe injected trait
  - `AtmMessageId` and `TaskId` remain typed identifiers in daemon event
    payloads; raw string semantic identifiers are not the Phase V target shape
- native plugins may emit plugin-local diagnostics, but daemon-owned runtime,
  store, ingest, and transport events remain daemon-owned observability sinks
- production runtime diagnostics must not collapse into ad hoc stdout/stderr
  debugging

### 21.6.3 Doctor Health Interface

`atm doctor` remains a CLI command, but the current SQLite/daemon architecture requires one
explicit daemon health interface.

Architectural rules:
- CLI doctor code may answer direct local config/store checks without daemon
  routing, but daemon-owned runtime state still crosses one explicit request /
  response boundary
- the daemon owns collection of runtime-only health such as:
  - heartbeat-driven runtime member state
  - singleton ownership state
  - live status-cache health
  - ingest backlog / degraded-ingest state
- the runtime-health DTO returned across that boundary must carry:
  - liveness
  - readiness
  - singleton-owner pid when known
  - degraded-ingest state
  - aggregate active/idle/offline/unknown member counts
- CLI code must not inspect private daemon state directly to synthesize health
  answers
- Runtime observation (state, pid, session, and timestamps) is daemon-memory
  telemetry. Only heartbeat and successful environment-attested local CLI or
  graft ingress update it; it never selects routing, nudge, retry, admission,
  delivery, notification, or policy behavior.
- Changed trusted pid/session replaces the current observation and emits
  retained diagnostic evidence. It does not reject ingress, create an
  `IdentityConflict` lifecycle state, degrade readiness, or alter cache policy.

Phase AA target doctor split:
- daemon health remains a separate explicit request/response boundary for
  daemon-owned runtime state
- direct local doctor checks that only require config or store access do not
  need daemon routing
- SQLite/store readiness has been removed from daemon-owned health collection
  in `AA.3`; `RuntimeStatusSnapshot` carries no store-specific readiness
  fields
- store readiness then lives in direct local diagnostics or other subsystem
  doctor reports assembled above the backend, not in the daemon runtime DTO

### 21.6.4 Shutdown, Signals, Timeouts, And Resource Caps

The daemon runtime must use one documented operational contract.

Daemon singleton is requirement `#1`.

Architectural rules:
- only one `atm-daemon` process may exist anywhere on the host for the
  supported runtime model
- singleton enforcement uses at least:
  - a pre-spawn launch gate before fork/exec
  - a daemon-side startup gate before serving state
  - a static lint/CI gate that rejects daemon-spawn patterns in ordinary tests
- no test, tool, alternate socket path, or alternate `ATM_HOME` value is
  exempt from the singleton rule

Phase R operational defaults:
- graceful shutdown drain deadline: `5s`
- force-cancel deadline: `10s` total
- daemon auto-start publish deadline: `10s`
  (`AUTO_START_PUBLISH_TIMEOUT`)
- same-host daemon request deadline: `3s`
- SQLite `busy_timeout`: `5000ms`
  - authoritative since `R.5`; supersedes the pre-`R.5` `1500ms` baseline
- ingest batch processing slice: `2s`
- doctor health query deadline: `3s`

Required caps:
- max concurrent accepted connections: `64`
- max per-connection inflight requests: `32`
- ingest queue depth: `1024`
- SQLite handle budget: `1..=4`
- status-cache cap: `4096`

Required runtime-control behavior:
- install the host runtime-control source before listeners accept
- Unix may use `SIGINT` / `SIGTERM` / `SIGHUP`
- Windows may use console-control or service-control equivalents
- the graceful-shutdown control path enters the same bounded drain/checkpoint
  sequence on every platform
- the reload control path triggers bounded rescan/reload without dropping
  singleton ownership

Phase R daemon implementation notes:
- per-connection inflight cap `32` is documented now, but the current daemon
  still processes one request per accepted connection, so the inflight count
  is structurally `1` until framed multiplexing is introduced
- bounded `SIGHUP` config/roster reload now lands in `R.18`, including
  last-known-good preservation on invalid reload input

### 21.7 Test Strategy

The daemon is not the test strategy.

The target daemon-runtime test architecture must keep:
- core service logic testable in-process
- transport/watch/runtime logic testable through fakes or harnesses
- daemon process spawning out of the core test path
- default correctness suites free of:
  - daemon spawn
  - socket publication timing
  - retry sleeps
  - environment mutation races
  - auto-start side effects

Required test tiers:
- `FakeClientTransport` for deterministic CLI/composition tests
- in-process loopback transport for request/handler integration
- a narrow daemon-runtime suite for true singleton/startup/shutdown/recovery
  requirements only

If a capability cannot be tested without real daemon spawning, that is treated
as a design smell rather than the default approach.

### 21.8 Lock Elimination

The lock-release gate proved the file-based line is acceptable only as interim
relief. The current SQLite/daemon architecture removes mailbox-lock dependence from
ATM mail correctness by moving durable state ownership to SQLite and treating
JSONL as compatibility ingress/egress only.

### 21.9 Five-Stage Migration Model

The migration to the current SQLite/daemon architecture followed five architectural stages:

1. store and boundary foundation
2. compatibility ingest/export
3. ack/task migration
4. read/clear cutover plus thin daemon runtime
5. lock retirement and production gate

This ordering is intentional:
- durable truth moves first
- compatibility paths stay owned and explicit
- daemon runtime arrives only after service boundaries are proven
- lock retirement closes the phase after the daemon/runtime and store model are
  already in place
