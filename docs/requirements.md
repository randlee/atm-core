# ATM CLI Requirements

## 1. Product Definition

The product is a local command-line tool named `atm`.

This rewrite removes daemon architecture. It does not intentionally remove core non-daemon ATM functionality.

The retained product surface is:
- `atm send`
- `atm read`
- `atm log`
- `atm doctor`

The rewritten system must preserve usable non-daemon behavior already present in the retained commands unless these requirements explicitly retire or change it.

The system uses structured logging through `sc-observability`.

## 2. Scope

### 2.1 In Scope

- one binary: `atm`
- one primary library: `atm-core`
- file-based mail delivery
- file-based inbox reads
- configuration resolution
- hook-based identity fallback
- file-reference policy handling for `send --file`
- origin-inbox merge for `read` when bridge remotes are configured
- seen-state tracking for `read`
- timeout-based waiting for `read --timeout`
- structured logging through `sc-observability`
- log query and follow through `sc-observability`
- local diagnostics through `atm doctor`
- JSON output mode
- human-readable output mode

### 2.2 Out Of Scope

- daemon support
- daemon sockets
- daemon launch or supervision
- plugin host framework
- CI monitoring
- TUI and MCP features
- runtime spawning and launch commands
- `atm status` in the initial rewrite
- separate `atm tail` command in the initial rewrite
- team lifecycle management outside what the retained commands need

## 3. External Contracts

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

### 3.2 Team Mail Store

Per-team layout:
- `{ATM_HOME}/.claude/teams/{team}/config.json`
- `{ATM_HOME}/.claude/teams/{team}/inboxes/{agent}.json`
- optional origin inbox files:
  - `{ATM_HOME}/.claude/teams/{team}/inboxes/{agent}.{origin}.json`

The rewrite retains origin-file merge behavior for read and wait paths because it is part of the current file-based mail surface and does not require the daemon.

### 3.3 Configuration Resolution

Configuration resolution order:
1. CLI flags
2. environment variables
3. repo-local `.atm.toml`
4. global `{ATM_HOME}/.config/atm/config.toml`
5. defaults

Required config fields:
- default team
- identity

Supported optional config fields:
- roles map
- aliases map
- output format
- color
- bridge remotes and hostname aliases used by origin-inbox merge

### 3.4 Claude Settings Resolution

The system must resolve Claude settings for file-reference policy checks.

Resolution order:
1. explicit settings path override when provided internally
2. repo-local `.claude/settings.local.json`
3. repo-local `.claude/settings.json`
4. global `{ATM_HOME}/.claude/settings.json`

### 3.5 Observability Shared API Prerequisite

ATM depends on `sc-observability` providing a shared logging surface that supports:
- structured log emission
- historical query of retained records
- follow/tail of new matching records
- filtering by severity
- filtering by structured key/value fields
- filtering by time window
- limit/order controls
- health reporting for the logging runtime

This prerequisite is handled by an early ATM planning/coordination sprint:
- `OBS-GAP-1`

ATM must not implement a parallel ad hoc log-query engine when shared `sc-observability` APIs can own the behavior.

## 4. Identity Resolution

### 4.1 Send Identity Resolution Order

1. `--from`
2. hook-file identity
3. `ATM_IDENTITY`
4. config identity

### 4.2 Read Identity Resolution Order

1. `--as`
2. hook-file identity
3. `ATM_IDENTITY`
4. config identity

### 4.3 Doctor Identity Resolution

`atm doctor` uses the same config and hook-resolution paths as the retained mail commands, but it must not fail immediately only because hook identity is absent. Missing hook identity is a diagnostic finding unless identity resolution is explicitly required for a requested check.

If command identity cannot be determined where required, the command must fail with a structured recovery-oriented error.

## 5. Address Resolution

Supported address forms:
- `agent`
- `agent@team`

Resolution order:
1. explicit `agent@team`
2. bare `agent` plus `--team`
3. bare `agent` plus configured default team

An explicit `@team` suffix takes precedence over `--team`.

Roles and aliases are resolved after splitting `agent@team`, so only the agent token is rewritten.

## 6. `atm send`

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

Retired from the current implementation:
- `--offline-action`
  - this flag exists only to cooperate with daemon-backed liveness checks and is not retained

### 6.3 Required Behavior

- resolve sender identity using the defined precedence
- resolve recipient address using the defined precedence
- resolve roles and aliases before mailbox lookup
- verify target team exists
- verify target agent exists in team config
- validate message text before write
- generate summary when not explicitly provided
- generate message id for ATM-authored messages
- append atomically to the inbox file
- create inbox file if absent
- preserve duplicate-suppression behavior for message ids
- support dry-run without mutation

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

### 6.5 Output Contract

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

Dry-run JSON output must include:
- `action = "send"`
- `agent`
- `team`
- `message`
- `dry_run = true`

## 7. `atm read`

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
- classify each message into a canonical workflow state
- map canonical states into display buckets
- support filtering by sender and timestamp
- support selection by queue mode
- preserve origin-inbox visibility when bridge remotes are configured
- sort newest-first before limiting
- mutate only the caller’s own displayed unread messages when marking is enabled
- support optional wait mode with timeout
- support optional seen-state filtering and updates

### 7.4 Display Buckets

The CLI exposes three display buckets:
- `unread`
- `pending_ack`
- `history`

Bucket mapping from canonical message state:
- `Unread` -> `unread`
- `PendingAck` -> `pending_ack`
- `Read` -> `history`
- `Acknowledged` -> `history`

The display buckets are a presentation contract. They are not the canonical state machine.

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

### 7.7 Mutation Rules

Read-triggered mutation happens only when:
- the caller is reading their own inbox
- `--no-mark` is not set
- the message is displayed
- the message is currently `Unread`

Required transition on read:
- `Unread -> PendingAck`

No read-triggered mutation happens when:
- reading another agent’s inbox
- `--no-mark` is set
- the message is already `PendingAck`
- the message is already `Acknowledged`
- the message is already `Read`

### 7.8 Processing Order

1. resolve actor and target inbox
2. load messages from the merged inbox surface (including building the hostname registry for configured origin inboxes, to resolve which origin files belong to each remote)
3. classify canonical state
4. apply sender filter
5. apply timestamp filter
6. apply seen-state filter when enabled and selection is not `--all`
7. apply selection mode
8. sort newest-first
9. apply limit
10. if enabled, mutate displayed unread messages through the workflow state machine
11. persist read-triggered state changes atomically
12. update seen-state when enabled
13. render output

### 7.9 Output Contract

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

## 8. `atm log`

### 8.1 Purpose

Inspect ATM observability records through shared `sc-observability` query/follow APIs.

`atm log` replaces the old daemon-log viewing model. It must not depend on daemon-owned log files, daemon status, or tmux fallback behavior.

### 8.2 Supported Flags

- `--tail`
- `--level <trace|debug|info|warn|error>`
- `--match <key=value>` repeatable
- `--since <iso8601|duration>`
- `--limit <n>`
- `--json`

Deferred from the current source repo:
- direct `--file` selection of arbitrary ATM log files
- separate `atm tail` command

### 8.3 Required Behavior

- query existing ATM records through an `atm-core` observability adapter over `sc-observability`
- support follow mode through the same adapter
- support filtering by level
- support filtering by structured key/value fields
- support filtering by time window
- support limit/order controls for non-tail mode
- default to snapshot mode when `--tail` is not set
- return snapshot results newest-first before applying output limits
- return followed records in arrival order while `--tail` is active

### 8.4 ATM Log Fields

The retained ATM event vocabulary must include enough structure to filter on:
- command
- team
- actor
- target
- outcome
- error class

This ATM field set is ATM-owned even when the underlying query/follow/filter mechanics are shared in `sc-observability`.

### 8.5 Output Contract

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

## 9. `atm doctor`

### 9.1 Purpose

Run local ATM diagnostics for the retained daemon-free system.

`atm doctor` in the rewrite is a local diagnostics command. It is not a daemon-health report.

### 9.2 Supported Flags

- `--team <name>`
- `--json`

### 9.3 Required Checks

The initial doctor implementation must cover:
- config file discovery and parse health
- effective team resolution
- identity resolution inputs and fallbacks
- team directory existence
- team config existence and parse health
- inbox directory existence and writability
- hook identity availability
- `ATM_HOME`, `ATM_TEAM`, and `ATM_IDENTITY` override visibility
- `sc-observability` initialization health
- `sc-observability` query-health readiness for `atm log`

### 9.4 Output Contract

Human output must provide:
- overall status summary
- findings grouped by severity
- concrete remediation guidance when the user can act

JSON output must provide:
- summary
- findings
- recommendations
- environment override visibility
- observability health snapshot

Each doctor finding must expose at least:
- severity
- code
- message
- remediation when available

Critical findings must cause a non-zero exit status.

## 10. Message And Workflow Model

### 10.1 Persisted Message Fields

Required fields:
- `from`
- `text`
- `timestamp`
- `read`

Optional fields:
- `source_team`
- `summary`
- `message_id`
- `pendingAckAt`
- `acknowledgedAt`
- `acknowledgesMessageId`

Unknown fields must be preserved.

### 10.2 Canonical Workflow States

Canonical states:
- `Unread`
- `PendingAck`
- `Acknowledged`
- `Read`

Classification order:
1. `acknowledgedAt` present => `Acknowledged`
2. else `pendingAckAt` present => `PendingAck`
3. else `read = false` => `Unread`
4. else `read = true` => `Read`

The canonical state machine is distinct from the read command’s display buckets.

### 10.3 Required State Transitions

```text
Send
  -> Unread

Read own inbox with marking enabled
  Unread -> PendingAck

Read own inbox with --no-mark
  Unread -> Unread

Read other inbox
  Unread -> Unread
  PendingAck -> PendingAck
  Acknowledged -> Acknowledged
  Read -> Read

Ack workflow
  PendingAck -> Acknowledged
  and emit a reply message that references the original message id
```

Disallowed transitions:
- `Read -> Unread`
- `Acknowledged -> PendingAck`
- `Acknowledged -> Read`
- any transition that skips the legal workflow graph

The implementation must encode legal transitions in code structure, not only in comments or tests.

## 11. Observability Requirements

ATM must emit structured records through `sc-observability`.

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
- result
- error class on failure
- count when applicable
- transition count when applicable

Emission is best-effort:
- logging failures must never block retained command behavior
- command correctness takes priority over observability delivery

`atm log` and `atm doctor` are not best-effort features in the same sense:
- they are explicit observability consumers
- if shared query/health APIs are unavailable, they must fail with clear structured errors

## 12. Error Requirements

All user-visible failures must use structured errors with recovery guidance.

Minimum error categories:
- configuration
- address
- identity resolution
- team not found
- agent not found
- mailbox read
- mailbox write
- message validation
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

## 13. Reliability Requirements

- mailbox writes must be atomic
- concurrent appends must not silently lose messages
- duplicate message ids must not be appended twice
- corrupt records should be skipped individually when possible
- missing inbox files are treated as empty inboxes
- seen-state races must not corrupt mailbox data
- observability emission failures must not corrupt command behavior

## 14. Testing Requirements

Because `sc-observability` is newly introduced into ATM, the rewrite must add explicit test coverage for:
- ATM event emission through the observability adapter
- best-effort emission failure behavior
- log query by severity
- log query by structured field match
- log follow/tail behavior
- doctor observability-health reporting
- retained mail-command correctness when observability emission fails

The implementation must include:
- `atm-core` tests for observability adapter behavior
- CLI integration tests for `atm log`
- CLI integration tests for `atm doctor`

## 15. Acceptance Criteria

The rewrite is ready when:
- `atm send` works without daemon support
- `atm read` works without daemon support
- `atm log` works through shared `sc-observability` APIs
- `atm doctor` works as a local diagnostics command
- retained commands preserve documented non-daemon behavior
- workflow state classification is correct
- workflow state transitions are encoded in implementation structure
- display buckets are derived consistently from canonical state
- observability integration is exercised by automated tests
- the file-by-file migration plan is complete enough to implement directly
