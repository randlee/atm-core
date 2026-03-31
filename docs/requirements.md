# ATM CLI Requirements

## 1. Product Definition

Product requirement ID:
- `REQ-P-PRODUCT-001` The retained daemon-free ATM product surface consists of
  `send`, `read`, `ack`, `clear`, `log`, and `doctor`.

Satisfied by:
- intentionally undecomposed product requirement; this governs overall retained
  product scope rather than a single crate-local obligation

The product is a local command-line tool named `atm`.

This rewrite removes daemon architecture. It does not intentionally remove core non-daemon ATM functionality.

The retained product surface is:
- `atm send`
- `atm read`
- `atm ack`
- `atm clear`
- `atm log`
- `atm doctor`

The rewritten system must preserve usable non-daemon behavior already present in the retained commands unless these requirements explicitly retire or change it.

The system uses structured logging through `sc-observability`.

## 1.1 Documentation Structure

Documentation organization is defined in
[`documentation-guidelines.md`](./documentation-guidelines.md).

Top-level product docs in `docs/` remain the product source of truth.
Crate-local ownership docs live under:

- [`docs/atm/requirements.md`](./atm/requirements.md)
- [`docs/atm/architecture.md`](./atm/architecture.md)
- [`docs/atm-core/requirements.md`](./atm-core/requirements.md)
- [`docs/atm-core/architecture.md`](./atm-core/architecture.md)

During the cleanup/restructure phase, product requirements stay here while
crate-local ownership is moved out of this file into the crate directories.

## 2. Scope

Product requirement ID:
- `REQ-P-SCOPE-001` The rewrite retains the documented command surface and
  removes daemon architecture without intentionally removing retained
  functionality.

Satisfied by:
- intentionally undecomposed product requirement; this governs overall rewrite
  scope and is enforced across the workspace rather than by one crate-local ID

### 2.1 In Scope

- one binary: `atm`
- one primary library: `atm-core`
- file-based mail delivery
- file-based inbox reads
- file-based acknowledgement workflow
- file-based inbox clearing
- configuration resolution
- hook-based identity fallback
- file-reference policy handling for `send --file`
- origin-inbox merge for `read` when bridge remotes are configured
- seen-state tracking for `read`
- timeout-based waiting for `read --timeout`
- structured logging through `sc-observability`
- log query and follow through `sc-observability`
- local diagnostics through `atm doctor`
- task metadata carried in the mail envelope
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

Product requirement ID:
- `REQ-P-CONTRACT-001` External path/config/store/observability contracts must
  match the documented daemon-free behavior.

Satisfied by:
- `REQ-CORE-CONFIG-001` for daemon-free home/path/config resolution aspects
- `REQ-CORE-MAILBOX-001` for daemon-free mail-store persistence aspects
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

Product requirement ID:
- `REQ-P-IDENTITY-001` Identity resolution must follow the documented command
  precedence rules.

Satisfied by:
- `REQ-CORE-CONFIG-001` for daemon-free identity resolution policy

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

Product requirement ID:
- `REQ-P-ADDRESS-001` Address resolution must support the documented
  `agent`/`agent@team` forms and precedence rules.

Satisfied by:
- `REQ-CORE-CONFIG-002` for address parsing, alias/role rewrite, and
  team/member validation policy

Supported address forms:
- `agent`
- `agent@team`

Resolution order:
1. explicit `agent@team`
2. bare `agent` plus `--team`
3. bare `agent` plus configured default team

An explicit `@team` suffix takes precedence over `--team`.

Roles and aliases are resolved after splitting `agent@team`, so only the agent token is rewritten.

## 6. Idle Notification Lifecycle

Product requirement ID:
- `REQ-P-IDLE-001` ATM must treat idle notifications as a non-actionable
  notification class and retain at most one unread idle notification per
  sender in any inbox.

Satisfied by:
- `REQ-CORE-MAILBOX-001` for sender-scoped idle-notification deduplication in
  the atomic mailbox append boundary
- `REQ-CORE-SEND-001` for send-path classification and delivery behavior once
  the envelope is identified as an idle notification

Required behavior:
- detect an idle notification by parsing the persisted message `text` field as
  JSON and checking for `type == "idle_notification"`
- if parsing fails or the parsed `type` differs, treat the record as a normal
  message
- when a new idle notification from sender `S` arrives at inbox `I`, ATM must
  atomically remove any older unread idle notification from sender `S` in
  inbox `I` before appending the new record
- idle notifications are non-actionable and are not part of the unread or
  pending-ack work queues
- idle-notification lifecycle rules shall not apply to non-idle message kinds

Deferred from this sprint:
- read-time auto-purge of displayed idle notifications
- daemon-side idle-notification removal behavior

## 7. `atm send`

Product requirement ID:
- `REQ-P-SEND-001` `atm send` must satisfy the documented send contract.

Satisfied by:
- `REQ-ATM-CMD-001` for CLI entry, parsing, and dispatch aspects
- `REQ-ATM-OUT-001` for human-readable and JSON output aspects
- `REQ-CORE-CONFIG-002` for address resolution and target-validation aspects
- `REQ-CORE-SEND-001` for send-path message construction and classification
  aspects
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
- resolve roles and aliases before mailbox lookup
- verify target team existence and target agent membership as part of address resolution before mailbox path selection
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
- write a non-null `message_id` on every ATM-authored message
- generate `message_id` as a UUID v4 at send time
- when the outgoing envelope is classified as an idle notification, apply the
  sender-scoped idle-notification deduplication rule inside the same atomic
  mailbox append boundary before appending the new record

`message_id` is required on every message written by `atm send`.

`message_id` is optional in the persisted schema (§12.1) only to support
legacy messages written by older clients, but `atm send` never omits it.

Recipients use `message_id` for:
- duplicate suppression
- read-time duplicate collapse
- acknowledgement targeting

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

## 8. `atm read`

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

## 9. `atm ack`

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

## 10. `atm clear`

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

## 11. `atm log`

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

## 12. `atm doctor`

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

Run local ATM diagnostics for the retained daemon-free system.

`atm doctor` in the rewrite is a local diagnostics command. It is not a daemon-health report.

### 11.2 Supported Flags

- `--team <name>`
- `--json`

### 11.3 Required Checks

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

### 11.4 Output Contract

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

## 13. Message And Workflow Model

Product requirement ID:
- `REQ-P-WORKFLOW-001` The message/workflow model must satisfy the documented
  persisted-field, two-axis, and legal-transition rules.

Satisfied by:
- `REQ-CORE-WORKFLOW-001` for the canonical two-axis model and legal
  transitions

### 12.1 Persisted Message Fields

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

Unknown fields must be preserved.

For ATM-authored messages:
- `message_id` is mandatory
- `message_id` must be UUID v4
- `message_id` must not be null or blank

Legacy or externally imported records may still omit `message_id`; the rewrite
must preserve such records without inventing synthetic ids during read.

### 12.2 Two-Axis Canonical Model

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

### 12.3 Required State Transitions

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

### 12.4 Task Metadata Rule

Messages with `taskId` are task-linked messages.

Required rules:
- every task-linked message must require acknowledgement
- a task-linked message remains actionable until acknowledged
- a task-linked message must continue to appear in `atm read` until acknowledged
- a task-linked message must never be removed by `atm clear` before acknowledgement

## 14. Observability Requirements

Product requirement ID:
- `REQ-P-OBS-001` ATM observability must satisfy the documented best-effort
  emit behavior and shared query/follow/health expectations.

Satisfied by:
- `REQ-ATM-OBS-001` for CLI bootstrap/injection aspects
- `REQ-CORE-LOG-001` for ATM log query/follow service aspects
- `REQ-CORE-DOCTOR-001` for observability health reporting aspects
- `REQ-CORE-OBS-001` for ATM event and query-model boundary aspects

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
- task id when known
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

## 15. Error Requirements

Product requirement ID:
- `REQ-P-ERROR-001` Public command failures must satisfy the documented
  structured error requirements.

Satisfied by:
- intentionally undecomposed product requirement; crate-local error ownership
  remains derived from command and service requirements rather than a dedicated
  crate requirement ID in this pass

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

## 16. Reliability Requirements

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
- missing inbox files are treated as empty inboxes
- seen-state races must not corrupt mailbox data
- observability emission failures must not corrupt command behavior

## 17. Testing Requirements

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
- retained mail-command correctness when observability emission fails
- clear eligibility behavior

The implementation must include:
- `atm-core` tests for observability port behavior using test doubles
- CLI integration tests for `atm log`
- CLI integration tests for `atm doctor`
- CLI integration tests for `atm ack`
- CLI integration tests for `atm clear`

## 18. Acceptance Criteria

Product requirement ID:
- `REQ-P-ACCEPTANCE-001` The rewrite is complete only when the documented
  acceptance criteria are met.

Satisfied by:
- intentionally undecomposed product requirement; this defines overall product
  completion gates rather than a single crate-local obligation

The rewrite is ready when:
- `atm send` works without daemon support
- `atm read` works without daemon support
- `atm ack` works without daemon support
- `atm clear` works without daemon support
- `atm log` works through shared `sc-observability` APIs
- `atm doctor` works as a local diagnostics command
- retained commands preserve documented non-daemon behavior
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
