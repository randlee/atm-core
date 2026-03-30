# File Migration Plan

**Lifecycle**: Temporary migration artifact

This document is authoritative only for the rewrite/migration program. Once the
rewrite is complete and the retained source layout is stable, this document
should be retired rather than maintained as permanent product documentation.

This is the authoritative implementation plan.

Format:
- `copy <source> -> <destination>` means reuse the existing file as the starting point
- `do not copy <source>` means the file was reviewed and intentionally excluded

Every file listed below is either:
- necessary for retained functionality
- directly usable as a starting point
- or explicitly reviewed and rejected

## 1. CLI Shell

### 1.1 `copy crates/atm/src/main.rs -> crates/atm/src/main.rs`

Keep:
- process entrypoint shape
- clap bootstrap
- one-time logging bootstrap pattern
- command dispatch handoff

Change:
- remove event-log observer hook logic
- remove GH teardown hooks
- remove daemon logging-health reads
- initialize the concrete `sc-observability` implementation of the injected observability port
- inject that implementation into retained command execution
- emit only retained command lifecycle events
- keep only the startup and exit behavior needed for `send`, `read`, `ack`, `clear`, `log`, and `doctor`

### 1.2 `copy crates/atm/src/commands/mod.rs -> crates/atm/src/commands/mod.rs`

Keep:
- clap `Cli`
- subcommand dispatch structure
- `command_name()`
- `execute()`

Change:
- keep only `Send`, `Read`, `Ack`, `Clear`, `Log`, and `Doctor`
- remove every other module import
- remove every other command variant
- remove dispatch for every non-retained command

### 1.3 `copy crates/atm/src/commands/send.rs -> crates/atm/src/commands/send.rs`

Keep:
- `SendArgs`
- current CLI flag names that remain in scope
- recipient resolution flow
- team existence and membership checks
- human and JSON output intent

Change:
- replace command-local business logic with `atm_core::send::send_mail` plus the injected observability port
- keep `--file`, `--stdin`, `--dry-run`, `--json`, and `--from`
- add retained `--requires-ack` and `--task-id`
- retire daemon-backed `--offline-action`
- preserve current non-daemon send behavior
- keep the existing file-reference body shape
- create pending-ack messages at send time for ack-required or task-linked mail

Move helpers out:
- `get_message_text` -> `crates/atm-core/src/send/input.rs`
- `process_file_reference` -> `crates/atm-core/src/send/file_policy.rs`
- `generate_summary` -> `crates/atm-core/src/send/summary.rs`
- `build_inbox_message` -> `crates/atm-core/src/send/mod.rs`

Delete in place:
- `register_sender_hint`
- `recipient_has_dead_session*`
- `resolve_offline_action`
- `should_warn_self_send*`
- `touch_sender_session_heartbeat*`
- runtime/backend detection helpers
- daemon state enrichment in output

### 1.4 `copy crates/atm/src/commands/read.rs -> crates/atm/src/commands/read.rs`

Keep:
- `ReadArgs`
- current read flag surface
- own-inbox default behavior
- queue-oriented output intent
- bucket-oriented human and JSON rendering

Change:
- replace command-local business logic with `atm_core::read::read_mail` plus the injected observability port
- preserve the two-axis workflow model plus the derived four-class queue projection
- preserve three display buckets
- preserve `--history` as “actionable queue plus history”
- preserve `--all` as “show all and bypass seen-state filtering”
- preserve origin-inbox visibility

Move helpers out:
- selection/filter helpers -> `crates/atm-core/src/read/filters.rs`
- workflow-state helpers -> `crates/atm-core/src/read/state.rs`
- seen-state helpers -> `crates/atm-core/src/read/seen_state.rs`
- wait logic -> `crates/atm-core/src/read/wait.rs`
- relative-time formatting -> `crates/atm/src/output.rs`
- hostname-registry merge setup -> `crates/atm-core/src/read/mod.rs`

Delete in place:
- daemon session lookup
- daemon-oriented event-log emission

Behavior notes:
- keep default queue buckets and hidden-history line
- keep `bucket_counts` and `history_collapsed` JSON fields
- make `read = true` the base mutation for any displayed message
- keep read-triggered ack activation for `(Unread, NoAckRequired)` when reading your own inbox with marking enabled
- keep sender-created pending-ack messages actionable until acknowledgement

### 1.5 `copy crates/atm/src/commands/ack.rs -> crates/atm/src/commands/ack.rs`

Keep:
- `AckArgs`
- atomic reply-plus-ack transaction shape
- reply target resolution from the source envelope
- human and JSON output intent

Change:
- replace command-local business logic with `atm_core::ack::ack_mail` plus the injected observability port
- keep the retained reply contract and `acknowledgesMessageId` behavior
- align validation with the two-axis workflow model

### 1.6 `copy crates/atm/src/commands/inbox.rs -> crates/atm/src/commands/clear.rs`

Keep:
- clear command mutation shape
- dry-run reporting intent
- JSON summary intent
- age-filter and idle-only filter concepts

Change:
- expose retained command surface as `atm clear`
- replace command-local business logic with `atm_core::clear::clear_mail` plus the injected observability port
- make the default clear set exactly the two-axis clearable set
- never clear pending-ack messages
- never clear unread messages
- keep only the clear subcommand behavior; drop inbox summary/watch behavior

### 1.7 `copy crates/atm/src/commands/logs.rs -> crates/atm/src/commands/log.rs`

Keep:
- historical log query shape
- `--level`
- `--since`
- `--limit`
- `--json`

Change:
- rename the command surface from `logs` to `log`
- replace `--follow` with `--tail`
- add repeatable `--match key=value` filters
- delete direct log-file path resolution from the CLI
- replace `agent_team_mail_core::log_reader::*` with `atm_core::log::*`
- treat human output as ATM-owned rendering over shared log records

### 1.8 `copy crates/atm/src/commands/doctor.rs -> crates/atm/src/commands/doctor.rs`

Keep:
- CLI report/rendering shape
- summary/finding/recommendation presentation intent
- JSON report output pattern

Change:
- replace command-local business logic with `atm_core::doctor::run_doctor`
- simplify the retained CLI surface to local daemon-free diagnostics
- delete daemon/session/plugin/GH-specific checks
- keep severity-based reporting and remediation output

### 1.9 `copy crates/atm/src/commands/wait.rs -> crates/atm-core/src/read/wait.rs`

Keep:
- timeout-based file watching
- polling fallback
- message-count comparison logic

Change:
- keep origin-inbox counting because merged inbox visibility is retained
- preserve current queue-first timeout semantics: return immediately when the current selection already contains actionable messages, and wait only when the selection is empty
- return core read-layer errors instead of command-local errors

### 1.10 `do not copy crates/atm/src/commands/bridge.rs`

Decision:
- bridge management CLI is out of scope

### 1.11 `do not copy crates/atm/src/commands/broadcast.rs`

Decision:
- not part of the initial retained command surface

### 1.12 `do not copy crates/atm/src/commands/cleanup.rs`

Decision:
- not part of the initial retained command surface

### 1.13 `do not copy crates/atm/src/commands/config_cmd.rs`

Decision:
- no dedicated config command in the initial retained command surface

### 1.14 `do not copy crates/atm/src/commands/daemon.rs`

Decision:
- daemon removed

### 1.15 `do not copy crates/atm/src/commands/gh.rs`

Decision:
- GH monitoring removed from the initial retained command surface

### 1.16 `do not copy crates/atm/src/commands/init.rs`

Decision:
- onboarding/init command not retained in the initial retained command surface

### 1.17 `do not copy crates/atm/src/commands/launch.rs`

Decision:
- runtime launch removed

### 1.18 `do not copy crates/atm/src/commands/logging_health.rs`

Decision:
- daemon logging-health surface removed

### 1.19 `do not copy crates/atm/src/commands/mcp.rs`

Decision:
- MCP surface removed

### 1.20 `do not copy crates/atm/src/commands/members.rs`

Decision:
- member management command not retained in the initial retained command surface

### 1.21 `do not copy crates/atm/src/commands/monitor.rs`

Decision:
- monitor command not retained in the initial retained command surface

### 1.22 `do not copy crates/atm/src/commands/register.rs`

Decision:
- registration command not retained in the initial retained command surface

### 1.23 `do not copy crates/atm/src/commands/request.rs`

Decision:
- not part of the initial retained command surface

### 1.24 `do not copy crates/atm/src/commands/runtime_adapter.rs`

Decision:
- runtime adapter surface removed

### 1.25 `do not copy crates/atm/src/commands/spawn.rs`

Decision:
- runtime spawn removed

### 1.26 `do not copy crates/atm/src/commands/status.rs`

Decision:
- daemon/status surface removed

### 1.27 `do not copy crates/atm/src/commands/subscribe.rs`

Decision:
- subscription surface removed

### 1.28 `do not copy crates/atm/src/commands/tail.rs`

Decision:
- replaced by `atm log --tail` over shared observability follow APIs

### 1.29 `do not copy crates/atm/src/commands/teams.rs`

Decision:
- team management command not retained in the initial retained command surface

### 1.30 `do not copy crates/atm/src/consts.rs`

Decision:
- fold `MESSAGE_MAX_LEN` into `crates/atm-core/src/send/summary.rs`

## 2. CLI Utilities

### 2.1 `copy crates/atm/src/util/addressing.rs -> crates/atm-core/src/address.rs`

Keep:
- parsing of `agent`
- parsing of `agent@team`
- empty-token validation

Change:
- explicit `@team` suffix must win over `--team`
- return semantic types instead of raw strings where practical
- surface structured address errors

### 2.2 `do not copy crates/atm/src/util/settings.rs`

Decision:
- wrapper file is unnecessary

Replacement:
- use `crates/atm-core/src/home.rs` directly

### 2.3 `copy crates/atm/src/util/file_policy.rs -> crates/atm-core/src/send/file_policy.rs`

Keep:
- repo-root detection
- Claude settings permission checks
- share-copy fallback behavior

Change:
- convert to core error types
- remove direct dependence on CLI-local utilities
- keep behavior aligned with current `send --file`

### 2.4 `copy crates/atm/src/util/hook_identity.rs -> crates/atm-core/src/identity/hook.rs`

Keep:
- hook file schema
- parent-pid lookup
- `read_hook_file`
- `read_hook_file_identity`

Change:
- keep only hook identity resolution needed by `send` and `read`
- delete session-file scanning logic
- delete daemon/runtime ambiguity helpers

### 2.5 `copy crates/atm/src/util/state.rs -> crates/atm-core/src/read/seen_state.rs`

Keep:
- seen-state file schema
- load/save helpers
- update/get helpers

Change:
- convert to core error model
- make path resolution use core home helpers only

### 2.6 `do not copy crates/atm/src/util/caller_identity.rs`

Decision:
- daemon/runtime ambiguity resolver
- not needed in a daemon-free rewrite

### 2.7 `do not copy crates/atm/src/util/member_labels.rs`

Decision:
- not required by the retained command surface

### 2.8 `do not copy crates/atm/src/util/mod.rs`

Decision:
- utility module index will be replaced by the smaller core module tree

### 2.9 `copy crates/atm/src/commands/send.rs -> crates/atm-core/src/send/input.rs`

Keep:
- `get_message_text`

Change:
- convert from CLI-local `SendArgs` handling to a core request-source parser
- return `AtmError`

### 2.10 `copy crates/atm/src/commands/send.rs -> crates/atm-core/src/send/summary.rs`

Keep:
- `generate_summary`

Change:
- move summary-length constants here
- keep current summary behavior stable for send output

### 2.11 `copy crates/atm/src/commands/send.rs -> crates/atm-core/src/send/mod.rs`

Keep:
- `build_inbox_message`
- overall send flow shape

Change:
- build `SendRequest` / `SendOutcome`
- own the send service orchestration
- keep team/agent membership checks inside address resolution before mailbox path selection
- validate message text inside the atomic append boundary
- set `pendingAckAt` inside the send boundary for ack-required or task-linked mail
- persist `taskId` when present
- call mailbox append and the injected observability port directly

### 2.12 `copy crates/atm/src/commands/read.rs -> crates/atm-core/src/read/state.rs`

Keep:
- current bucket-classification knowledge

Change:
- convert bucket rules into read-axis, ack-axis, and derived message-class classification
- add display-bucket mapping
- define typestate transitions for both axes

### 2.13 `copy crates/atm/src/commands/read.rs -> crates/atm-core/src/read/filters.rs`

Keep:
- sender filter behavior
- timestamp filter behavior
- selection-mode behavior
- limit behavior

Change:
- separate canonical state filtering from display bucket selection
- encode `--history` and `--all` semantics explicitly

### 2.14 `copy crates/atm/src/commands/read.rs -> crates/atm-core/src/read/mod.rs`

Keep:
- overall read flow shape

Change:
- build `ReadQuery` / `ReadOutcome`
- own merged inbox loading, selection, mutation, and seen-state update sequencing
- make `read = true` the base display mutation
- activate pending-ack only for displayed `(Unread, NoAckRequired)` messages when marking is enabled in the caller's own inbox
- remove CLI formatting concerns

### 2.15 `copy crates/atm/src/commands/read.rs -> crates/atm/src/output.rs`

Keep:
- relative-time formatting
- bucket rendering
- queue header and hidden-history rendering

Change:
- consume `ReadOutcome` instead of raw inbox messages
- keep human output text stable

### 2.16 `copy crates/atm/src/commands/ack.rs -> crates/atm-core/src/ack/mod.rs`

Keep:
- atomic ack transaction shape
- reply target resolution logic
- reply-message construction shape

Change:
- build `AckRequest` / `AckOutcome`
- validate the source message against the two-axis model
- preserve `acknowledgesMessageId` behavior
- emit retained observability lifecycle events through the injected port

### 2.17 `copy crates/atm/src/commands/inbox.rs -> crates/atm-core/src/clear/mod.rs`

Keep:
- clear-set filtering loop
- dry-run and result-count concepts
- age-filter logic

Change:
- reduce the source to clear-only behavior
- compute clear eligibility from the two-axis model instead of ad hoc flags
- preserve idle-only filtering as an optional narrower mode
- never remove pending-ack or unread messages

### 2.18 `copy crates/atm/src/commands/logs.rs -> crates/atm-core/src/log/mod.rs`

Keep:
- historical query concepts
- time-window parsing concepts
- level filtering concepts
- human-versus-JSON rendering boundary

Change:
- replace file-based log reading with the injected observability port
- define `LogQuery`, `LogSnapshot`, and `LogTailSession`
- move ATM-specific field filtering into core log query translation over the injected observability port

### 2.19 `copy crates/atm/src/commands/logs.rs -> crates/atm-core/src/log/filters.rs`

Keep:
- level parsing
- since/limit filter concepts

Change:
- add structured `key=value` match parsing
- normalize ATM-owned structured-field filters before handing them to the observability port

### 2.20 `copy crates/atm/src/commands/doctor.rs -> crates/atm-core/src/doctor/mod.rs`

Keep:
- report/finding/recommendation structure
- severity model

Change:
- replace daemon/session/plugin/GH diagnostics with local ATM checks
- own `DoctorQuery` and `DoctorReport`
- project shared observability health from the injected observability port into ATM doctor output

### 2.21 `copy crates/atm/src/commands/doctor.rs -> crates/atm-core/src/doctor/report.rs`

Keep:
- summary and finding report shapes that remain useful in a daemon-free diagnostic report

Change:
- strip daemon-only fields
- keep local env/config/path and observability health findings

## 3. Core Home And Text

### 3.1 `copy crates/atm-core/src/home.rs -> crates/atm-core/src/home.rs`

Keep:
- `get_home_dir`
- `claude_root_dir_for`
- `teams_root_dir_for`
- `team_dir_for`
- `team_config_path_for`
- `inbox_path_for`
- `atm_config_dir_for`

Change:
- remove helper paths that only exist for daemon/runtime surfaces

### 3.2 `copy crates/atm-core/src/text.rs -> crates/atm-core/src/text.rs`

Keep:
- message validation
- Unicode-safe truncation helpers
- max message byte constant

Change:
- keep error strings aligned with the retained `send` UX

## 4. Config And Settings

### 4.1 `copy crates/atm-core/src/config/mod.rs -> crates/atm-core/src/config/mod.rs`

Keep:
- config module exports

Change:
- retain `bridge.rs`
- remove exports for daemon/plugin features

### 4.2 `copy crates/atm-core/src/config/bridge.rs -> crates/atm-core/src/config/bridge.rs`

Keep:
- `BridgeConfig`
- `RemoteConfig`
- `HostnameRegistry`
- alias resolution for origin hostnames

Change:
- keep only the parts required to discover and merge origin inbox files
- remove any framing that implies plugin hosting is retained

### 4.3 `copy crates/atm-core/src/config/discovery.rs -> crates/atm-core/src/config/discovery.rs`

Keep:
- config file discovery
- merge order
- env overrides
- repo-local config lookup
- `resolve_settings`
- repo-local Claude settings lookup

Change:
- delete plugin-config location support
- delete daemon-oriented config discovery
- keep only what `send`, `read`, bridge origin merge, and file policy need

### 4.4 `copy crates/atm-core/src/config/types.rs -> crates/atm-core/src/config/types.rs`

Keep:
- `Config`
- `CoreConfig`
- `DisplayConfig`
- `OutputFormat`
- aliases and roles maps

Change:
- retain bridge config
- delete plugin config tables
- delete retention and daemon-only config
- keep only settings needed for the retained command surface

### 4.5 `copy crates/atm-core/src/config/aliases.rs -> crates/atm-core/src/config/aliases.rs`

Keep:
- `resolve_alias`
- `resolve_identity`

Change:
- apply semantic newtypes internally without changing external alias-resolution behavior

## 5. Schema

### 5.1 `copy crates/atm-core/src/schema/mod.rs -> crates/atm-core/src/schema/mod.rs`

Keep:
- re-exports required by retained modules

Change:
- remove unrelated schema exports

### 5.2 `copy crates/atm-core/src/schema/inbox_message.rs -> crates/atm-core/src/schema/inbox_message.rs`

Keep:
- persisted message fields
- unknown-field preservation
- workflow helper accessors for `pendingAckAt` and `acknowledgedAt`
- `mark_pending_ack`
- `mark_acknowledged`

Change:
- add an accessor for `acknowledgesMessageId`
- add an accessor for `taskId`
- keep schema focused on persisted representation, not command policy
- treat `pendingAckAt` as the persisted ack-axis activation timestamp whether it originated at send-time or read-time
- move canonical workflow-axis classification into `read/state.rs`

### 5.3 `copy crates/atm-core/src/schema/team_config.rs -> crates/atm-core/src/schema/team_config.rs`

Keep:
- team config structure
- unknown-field preservation

Change:
- none beyond trimming unused docs or exports

### 5.4 `copy crates/atm-core/src/schema/agent_member.rs -> crates/atm-core/src/schema/agent_member.rs`

Keep:
- member structure used by `TeamConfig`
- member-name compatibility

Change:
- keep round-trip compatibility even for fields not used by the initial retained command surface
- strip daemon-oriented documentation text only; do not remove persisted fields from the schema

Dependency:
- copy `crates/atm-core/src/model_registry.rs` unchanged enough for `external_model` round-trips

### 5.5 `copy crates/atm-core/src/schema/settings.rs -> crates/atm-core/src/schema/settings.rs`

Keep:
- Claude settings schema

Change:
- none

### 5.6 `copy crates/atm-core/src/schema/permissions.rs -> crates/atm-core/src/schema/permissions.rs`

Keep:
- permissions schema used by file policy

Change:
- none

### 5.7 `do not copy crates/atm-core/src/schema/task.rs`

Decision:
- standalone task files are not part of the retained command surface yet

Replacement:
- carry task-linked message metadata in `InboxMessage.taskId`

### 5.8 `do not copy crates/atm-core/src/schema/version.rs`

Decision:
- not required by the retained command surface

## 6. Mailbox I/O

### 6.1 `copy crates/atm-core/src/io/mod.rs -> crates/atm-core/src/mailbox/mod.rs`

Keep:
- mailbox module organization

Change:
- export only retained mailbox primitives

### 6.2 `copy crates/atm-core/src/io/inbox.rs -> crates/atm-core/src/mailbox/store.rs`

Keep:
- atomic append logic
- tolerant file parsing
- conflict merge
- duplicate suppression
- merged inbox reads
- in-place update helper

Change:
- remove spool fallback
- fail explicitly on lock timeout instead of queueing
- keep origin-inbox merge behavior
- convert errors and logging to new core boundaries
- add workflow-aware read update helpers
- add clear-set replacement helpers for retained `atm clear`

### 6.3 `copy crates/atm-core/src/io/error.rs -> crates/atm-core/src/error.rs`

Keep:
- inbox error cases that still apply

Change:
- fold into the root `AtmError` model
- remove spool-only errors

### 6.4 `copy crates/atm-core/src/io/atomic.rs -> crates/atm-core/src/mailbox/atomic.rs`

Keep:
- atomic swap helper

### 6.5 `copy crates/atm-core/src/io/hash.rs -> crates/atm-core/src/mailbox/hash.rs`

Keep:
- conflict-detection hashing

### 6.6 `copy crates/atm-core/src/io/lock.rs -> crates/atm-core/src/mailbox/lock.rs`

Keep:
- lock acquisition and lock guard types

### 6.7 `do not copy crates/atm-core/src/io/spool.rs`

Decision:
- spool behavior existed to compensate for deferred/background delivery
- not required in the daemon-free rewrite

## 7. Core Exports And Supporting Files

### 7.1 `copy crates/atm-core/src/lib.rs -> crates/atm-core/src/lib.rs`

Keep:
- core module export structure

Change:
- remove daemon, control, retention, spawn, context, log-reader, and GH exports
- export only retained modules

### 7.2 `copy crates/atm-core/src/model_registry.rs -> crates/atm-core/src/model_registry.rs`

Keep:
- `ModelId`
- parser and serde support

Change:
- none

### 7.3 `copy crates/atm-core/src/observability.rs -> crates/atm-core/src/observability.rs`

Keep:
- reusable `sc-observability`-neutral ATM event and query types

Change:
- replace direct library-owned integration with the sealed `ObservabilityPort` boundary:
  - command lifecycle emission methods
  - record query/filter methods
  - follow/tail methods
  - health projection methods for `atm doctor`
- do not import `sc-observability` in `atm-core`
- remove daemon-specific health assumptions

### 7.4 `create crates/atm/src/observability.rs`

Purpose:
- implement the injected observability port using `sc-observability`
- translate ATM event/query models into shared observability calls
- keep the concrete backend dependency in `atm`, not `atm-core`

### 7.5 `do not copy crates/atm-core/src/consts.rs`

Decision:
- constants file is daemon-heavy
- retain only values that survive via narrower modules

### 7.6 `do not copy crates/atm-core/src/context/mod.rs`

Decision:
- not required by the retained command surface

### 7.7 `do not copy crates/atm-core/src/context/*`

Decision:
- not required by the retained command surface

### 7.8 `do not copy crates/atm-core/src/context/platform.rs`

Decision:
- not required by the retained command surface

### 7.9 `do not copy crates/atm-core/src/context/repo.rs`

Decision:
- not required by the retained command surface

### 7.10 `do not copy crates/atm-core/src/context/system.rs`

Decision:
- not required by the retained command surface

### 7.11 `do not copy crates/atm-core/src/control.rs`

Decision:
- daemon/control surface not retained

### 7.12 `do not copy crates/atm-core/src/daemon_client.rs`

Decision:
- daemon-only

### 7.13 `do not copy crates/atm-core/src/daemon_stream.rs`

Decision:
- daemon-only

### 7.14 `do not copy crates/atm-core/src/event_log.rs`

Decision:
- replace direct event-log API with the retained observability port boundary

### 7.15 `do not copy crates/atm-core/src/gh_command.rs`

Decision:
- not part of retained surface

### 7.16 `do not copy crates/atm-core/src/log_reader.rs`

Decision:
- replace daemon/log file scanning with shared observability query/follow APIs

### 7.17 `do not copy crates/atm-core/src/logging.rs`

Decision:
- replace via the retained observability port boundary

### 7.18 `do not copy crates/atm-core/src/logging_event.rs`

Decision:
- replace via the retained observability port boundary

### 7.19 `do not copy crates/atm-core/src/pid.rs`

Decision:
- only needed by session-file scanning, which is not retained

### 7.20 `do not copy crates/atm-core/src/retention.rs`

Decision:
- not part of retained surface

### 7.21 `do not copy crates/atm-core/src/spawn.rs`

Decision:
- runtime launch not retained

### 7.22 `do not copy crates/atm-core/src/team_config_store.rs`

Decision:
- direct team-config loading is sufficient for the initial retained command surface

## 8. Test Files

### 8.1 `copy crates/atm/tests/integration_send.rs -> crates/atm/tests/integration_send.rs`

Keep:
- retained send command coverage

Change:
- delete daemon-related assertions
- preserve file-policy and inbox-write expectations

### 8.2 `copy crates/atm/tests/integration_read.rs -> crates/atm/tests/integration_read.rs`

Keep:
- retained read command coverage
- bucket behavior expectations
- seen-state expectations
- pending-ack lifecycle expectations

Change:
- delete daemon-related assertions
- keep output-contract expectations aligned with the documented rewrite
- add task-linked pending-ack coverage

### 8.3 `copy crates/atm/tests/integration_read_timeout.rs -> crates/atm/tests/integration_read_timeout.rs`

Keep:
- timeout wait coverage

Change:
- keep origin-inbox visibility because the retained read timeout path watches the same merged inbox surface
- keep queue-first timeout behavior: existing actionable messages satisfy the read immediately; only an empty initial selection should block

### 8.4 `copy crates/atm/tests/integration_ack.rs -> crates/atm/tests/integration_ack.rs`

Keep:
- retained ack command coverage
- reply emission expectations
- message-id matching expectations

Change:
- delete daemon-related assertions
- align expected state transitions with the two-axis model

### 8.5 `copy crates/atm/tests/integration_inbox.rs -> crates/atm/tests/integration_clear.rs`

Keep:
- clear mutation coverage
- dry-run expectations
- retained result-count expectations

Change:
- narrow the source suite to clear-only behavior
- align expected removals with the two-axis clearable set
- add negative tests for pending-ack and unread messages

### 8.6 `copy crates/atm/tests/integration_auto_identity.rs -> crates/atm/tests/integration_auto_identity.rs`

Keep:
- hook identity resolution coverage for send/read

Change:
- delete cases that depend on daemon-era session ambiguity resolution

### 8.7 `copy crates/atm/tests/integration_discovery.rs -> crates/atm/tests/integration_discovery.rs`

Keep:
- config discovery coverage relevant to the retained command surface

Change:
- delete GH, daemon, plugin, or CI-specific coverage

### 8.8 `copy crates/atm/tests/integration_conflict_tests.rs -> crates/atm/tests/integration_conflict_tests.rs`

Keep:
- mailbox append and merge behavior relevant to the retained command surface

Change:
- delete spool fallback expectations

### 8.9 `copy crates/atm/tests/hook-scripts/test_atm_identity_cleanup.py -> crates/atm/tests/hook-scripts/test_atm_identity_cleanup.py`

Keep:
- retained hook identity support test

### 8.10 `copy crates/atm/tests/hook-scripts/test_atm_identity_write.py -> crates/atm/tests/hook-scripts/test_atm_identity_write.py`

Keep:
- retained hook identity support test

### 8.11 `do not copy crates/atm/tests/hook-scripts/test_gate_agent_spawns.py`

Decision:
- spawn/runtime behavior not retained

### 8.12 `copy crates/atm/tests/support/env_guard.rs -> crates/atm/tests/support/env_guard.rs`

Keep:
- test env isolation helper

### 8.13 `do not copy crates/atm/tests/support/daemon_process_guard.rs`

Decision:
- daemon-only

### 8.14 `do not copy crates/atm/tests/support/daemon_test_registry.rs`

Decision:
- daemon-only

### 8.15 `copy crates/atm-core/tests/home_dir_audit.rs -> crates/atm-core/tests/home_dir_audit.rs`

Keep:
- retained home/path contract tests

### 8.16 `do not copy crates/atm-core/tests/daemon_writer_fan_in.rs`

Decision:
- daemon logging path removed

### 8.17 `do not copy crates/atm-core/tests/logging_identity_contract.rs`

Decision:
- current contract is daemon-era logging-specific and will be replaced by retained observability tests

### 8.18 `do not copy crates/atm-core/tests/retention_tests.rs`

Decision:
- retention surface removed

### 8.19 `do not copy crates/atm/tests/integration_backup_restore.rs`

Decision:
- not part of the initial retained command surface

### 8.20 `do not copy crates/atm/tests/integration_broadcast.rs`

Decision:
- broadcast removed from the initial retained command surface

### 8.21 `do not copy crates/atm/tests/integration_daemon_autostart.rs`

Decision:
- daemon removed

### 8.22 `do not copy crates/atm/tests/integration_daemon_autostart_observability.rs`

Decision:
- daemon removed

### 8.23 `do not copy crates/atm/tests/integration_daemon_autostart_windows.rs`

Decision:
- daemon removed

### 8.24 `do not copy crates/atm/tests/integration_e2e_workflows.rs`

Decision:
- broad workflow suite must be replaced by smaller retained-command coverage

### 8.25 `do not copy crates/atm/tests/integration_external_member.rs`

Decision:
- external member management not retained in the initial retained command surface

### 8.26 `do not copy crates/atm/tests/integration_gh.rs`

Decision:
- GH surface removed from the initial retained command surface

### 8.27 `do not copy crates/atm/tests/integration_init_onboarding.rs`

Decision:
- init/onboarding command not retained in the initial retained command surface

### 8.28 `do not copy crates/atm/tests/integration_logging_health_schema.rs`

Decision:
- daemon logging-health surface removed

### 8.29 `do not copy crates/atm/tests/integration_mcp.rs`

Decision:
- MCP removed from the initial retained command surface

### 8.30 `do not copy crates/atm/tests/integration_monitor.rs`

Decision:
- monitoring surface removed

### 8.31 `do not copy crates/atm/tests/integration_multiteam_isolation.rs`

Decision:
- not required for the initial retained command surface

### 8.32 `do not copy crates/atm/tests/integration_otel_traces.rs`

Decision:
- replace with smaller retained observability coverage

### 8.33 `do not copy crates/atm/tests/integration_register.rs`

Decision:
- registration surface removed

### 8.34 `do not copy crates/atm/tests/integration_spawn_folder.rs`

Decision:
- spawn/runtime surface removed

### 8.35 `do not copy crates/atm/tests/integration_team_join.rs`

Decision:
- team join/management not retained in the initial retained command surface

### 8.36 `create crates/atm-core/tests/observability_port.rs`

Purpose:
- verify ATM event emission through the retained observability port boundary
- verify level, field, and time-window query translation
- verify follow/tail translation behavior
- verify best-effort emit failures do not become retained mail-command correctness failures
- verify explicit query or health failures return structured observability errors

### 8.37 `create crates/atm/tests/integration_log.rs`

Purpose:
- verify `atm log` clap parsing
- verify snapshot output order and `--limit` behavior
- verify `--level` filtering
- verify repeatable `--match key=value` filtering
- verify `--tail` follow behavior
- verify JSON output shape

### 8.38 `create crates/atm/tests/integration_doctor.rs`

Purpose:
- verify `atm doctor` clap parsing
- verify config/path/identity findings in human output
- verify JSON report shape
- verify critical findings produce non-zero exit status
- verify observability readiness appears in the report

### 8.39 `do not copy crates/atm/tests/integration_teams_cleanup_dry_run.rs`

Decision:
- team cleanup surface removed

### 8.40 `do not copy crates/atm/tests/integration_transient_registration.rs`

Decision:
- transient registration surface removed

## 9. Implementation Order

Use this order:
1. `home.rs`
2. `text.rs`
3. config files
4. settings and permissions schema
5. addressing
6. hook identity
7. file policy
8. schema files
9. mailbox files
10. send helpers
11. read helpers
12. observability port boundary
13. log helpers
14. doctor helpers
15. CLI command files
16. retained and new tests
17. core `lib.rs`

## 10. Review Standard

This file is ready to implement from when:
- every retained file has a `copy source -> dest` entry
- every reviewed non-retained file has an explicit `do not copy` entry
- no retained behavior is removed without a specific note
- workflow-state behavior is described consistently with `requirements.md`, `architecture.md`, and `read-behavior.md`
