# ATM Error Codes

## 1. Purpose

This document is the single source of truth for ATM-owned error codes.

All public ATM failures and ATM-emitted warning/error diagnostics must use a
code from this registry. No command, service, or logging path may invent ad hoc
error-code strings outside this document and its corresponding source registry.

## 2. Ownership

ATM owns these codes.

This document does not define:

- Claude Code-native schemas
- `sc-observability` shared error codes
- raw transport or OS error identifiers

Those may appear as causes or nested context, but ATM logs and user-facing
errors must still map them onto an ATM-owned code from this registry.

## 3. Source Enforcement

The corresponding source registry must live in one place:

- `crates/atm-error/src/error_codes.rs` (`atm-storage` and `atm-core`
  re-export this registry)

Required rules:

- `AtmError` carries one `AtmErrorCode`
- CLI bootstrap and parse/validation failure logging must also use
  `AtmErrorCode`
- warning diagnostics emitted during degraded recovery must also use
  `AtmErrorCode`
- logs must not hardcode free-form code strings outside the central registry
- the registry is centralized and read-only from the perspective of
  feature/service code; subsystems consume codes from this registry and do not
  mint local alternatives

## 4. Naming Rules

ATM-owned error codes use:

- prefix: `ATM_`
- uppercase snake case
- stable semantic meaning across CLI rendering, structured logs, and tests

Error codes should describe the failure class, not a specific prose message.

## 5. Registry

### 5.1 Config And Identity

- `ATM_CONFIG_HOME_UNAVAILABLE`
- `ATM_HOME_UNRESOLVED`
- `ATM_CONFIG_PARSE_FAILED`
- `ATM_CONFIG_TEAM_PARSE_FAILED`
- `ATM_CONFIG_TEAM_MISSING`
- `ATM_IDENTITY_UNAVAILABLE`

### 5.2 Address And Target Resolution

- `ATM_ADDRESS_PARSE_FAILED`
- `ATM_TEAM_UNAVAILABLE`
- `ATM_TEAM_NOT_FOUND`
- `ATM_AGENT_NOT_FOUND`

### 5.3 Mailbox And Message Validation

- `ATM_MAILBOX_READ_FAILED`
- `ATM_MAILBOX_WRITE_FAILED`
- `ATM_MAILBOX_LOCK_FAILED`
- `ATM_MAILBOX_LOCK_TIMEOUT`
- `ATM_MESSAGE_VALIDATION_FAILED`
- `ATM_SELF_ADDRESSED_SEND_INVALID`
- `ATM_SERIALIZATION_FAILED`
- `TEMPLATE_CONTENT_NOT_UTF8` — template catalog registration rejected raw
  content that has no strict UTF-8 projection; replace the source with UTF-8
  text and retry. This is checked before either a catalog row or a decomposed
  message row can become durable.
- `TEMPLATE_LOAD_FAILED` — the selected template could not be loaded.
- `TEMPLATE_HASH_API_FAILED` — the approved template inspection/hash adapter
  could not produce a durable SHA/hash identity.
- `TEMPLATE_INSPECTION_PARSE_FAILED` — the approved template inspection parser
  rejected frontmatter or body/directive syntax before any identity or message
  row could be admitted.
- `TEMPLATE_REQUIRED_VARIABLE_MISSING` — a required frontmatter variable was
  absent after deterministic merge.
- `TEMPLATE_RENDER_VERIFICATION_FAILED` — rendering or the adapter-owned
  checked-emission verification failed before a body can be admitted, sent, or
  returned.  For malformed JSON the retained cause is redacted and identifies
  the diagnostic location without echoing rendered values.
- `TEMPLATE_INCLUDE_UNRESOLVED` — a detected include/import could not be
  resolved inside the declared root; no message or catalog row was written.
- `TEMPLATE_CLASSIFICATION_INVALID` — category, tag, or content-format input
  failed template-message classification validation.
- `TEMPLATE_WORKFLOW_INVALID` — template `metadata.tags` or
  `metadata.workflow` is malformed, partial, dynamic, duplicated, or otherwise
  outside the durable workflow declaration contract. Correct the immutable
  template source and register its resulting SHA again; no catalog or message
  row is written for the rejected declaration.
- `TEMPLATE_WORKFLOW_VALUE_INVALID` — a declared workflow scope or iteration
  variable is missing, null, non-scalar, blank, or exceeds the bounded stored
  value limit after deterministic merge. Supply a valid merged value before
  decomposed admission; the storage capability is not called.
- `TEMPLATE_TAG_RESERVED` — a sender/instance or template-authored tag uses an
  ATM-reserved derived prefix (`template-type:`, `content-format:`, or one of
  the `workflow-*:` prefixes). Remove the spoofed tag; only ATM derives those
  classifications during workflow-aware admission.
- `DECOMPOSED_TEMPLATE_INCLUDE_FORBIDDEN` — a stored decomposed template was
  inspected during render-on-read and still declares an include, import, or
  from-import. Re-register the same SHA from a dependency-free source; ATM
  rejects the render before any resolver or loader can run.

#### 5.3.1 `ATM_MAILBOX_LOCK_TIMEOUT`

- code: `ATM_MAILBOX_LOCK_TIMEOUT`
- description: mailbox lock acquisition exceeded the total timeout budget before
  ATM could obtain the required exclusive lock set
- HTTP status: `503 Service Unavailable`
- context:
  - emitted by single-file mailbox mutations when one inbox lock remains
    contended past the configured deadline
  - emitted by multi-source `read`, `ack`, and `clear` when the full sorted lock
    set cannot be acquired under the shared timeout budget
  - signals a retriable contention condition; ATM must abort before persisting
    partial mailbox state

### 5.4 File Policy And Attachments

- `ATM_FILE_POLICY_REJECTED`
- `ATM_FILE_REFERENCE_REWRITE_FAILED`

### 5.5 Workflow And Timeouts

- `ATM_WORKFLOW_QUERY_INVALID` — a local workflow lifecycle projection used an
  empty selector or an impossible time range. Supply at least one exact
  start/end selector field and an ordered RFC 3339 range.
- `ATM_WORKFLOW_TELEMETRY_CONFIG_INVALID` — configured telemetry worker
  capacity or timeout is outside its bounded range. ATM remains available with
  telemetry disabled; repair the configuration and restart the daemon.
- `ATM_WORKFLOW_TELEMETRY_DROPPED` — the best-effort telemetry sink could not
  accept a record during a full queue, timeout, failure, or bounded shutdown.
  Inspect runtime diagnostics; this never changes admission, routing, or a
  query result.
- `ATM_WAIT_TIMEOUT`
- `ATM_ACK_INVALID_STATE`
- `ATM_CLEAR_INVALID_STATE`

### 5.6 Observability

- `ATM_OBSERVABILITY_HEALTH_OK`
- `ATM_OBSERVABILITY_EMIT_FAILED`
- `ATM_OBSERVABILITY_QUERY_FAILED`
- `ATM_OBSERVABILITY_FOLLOW_FAILED`
- `ATM_OBSERVABILITY_HEALTH_FAILED`
- `ATM_OBSERVABILITY_BOOTSTRAP_FAILED`

### 5.7 Recovery / Degradation Warnings

- `ATM_WARNING_INVALID_TEAM_MEMBER_SKIPPED`
- `ATM_WARNING_RESTORE_IN_PROGRESS`
- `ATM_WARNING_STALE_MAILBOX_LOCK`
- `ATM_WARNING_IDENTITY_DRIFT`
- `ATM_WARNING_BASELINE_MEMBER_MISSING`
- `ATM_WARNING_MAILBOX_RECORD_SKIPPED`
- `ATM_WARNING_MALFORMED_ATM_FIELD_IGNORED`
- `ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED`
- `ATM_WARNING_ORIGIN_INBOX_ENTRY_SKIPPED`
- `ATM_WARNING_MISSING_TEAM_CONFIG_FALLBACK`
- `ATM_WARNING_SEND_ALERT_STATE_DEGRADED`

#### 5.7.1 `ATM_WARNING_STALE_MAILBOX_LOCK`

- code: `ATM_WARNING_STALE_MAILBOX_LOCK`
- description: `atm doctor` observed the same mailbox `.lock` sentinel at the
  start and end of the run, so the lock is likely stale
- HTTP status: `200 OK`
- context:
  - emitted as a warning finding during `atm doctor`
  - the message should include the persisted lock path
  - recovery guidance should tell the user to confirm no live ATM process owns
    the mailbox and then run `rm -f <path>`

### 5.8 Post-Send Hook

- `ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY`
- `ATM_WARNING_HOOK_SKIPPED` (retired for filter non-match)
- `ATM_WARNING_HOOK_EXECUTION_FAILED`
- `ATM_POST_SEND_PANE_MISSING`
- `ATM_POST_SEND_TMUX_SEND_FAILED`

#### 5.8.1 `ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY`

- code: `ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY`
- description: `.atm.toml` contains the retired `post_send_hook_members` key
  instead of one or more explicit `[[atm.post_send_hooks]]` rules
- HTTP status: `400 Bad Request`
- context:
  - emitted during ATM config loading before send execution proceeds
  - requires migration guidance that explains the recipient-scoped rule shape
    and the `*` wildcard
  - `{config_path}` resolves to the discovered `.atm.toml` path that contained
    the retired key
  - expected output split:
    - message:
      ```text
      error: '{config_path}' field 'post_send_hook_members' is no longer supported.
      ```
    - recovery:
      ```text
      Replace 'post_send_hook_members' with one or more [[atm.post_send_hooks]]
      rules, each containing recipient = "name-or-*" and command = ["argv", ...].
      ```
  - the rendered CLI output may display the message and recovery together, but
    ATM stores them as separate fields on the structured error
  - must not be downgraded to a warning because the old key is ambiguous under
    the redesigned contract

#### 5.8.2 `ATM_CONFIG_RETIRED_LEGACY_HOOK_KEYS`

- code: `ATM_CONFIG_RETIRED_LEGACY_HOOK_KEYS`
- description: `.atm.toml` still uses the retired flat legacy post-send-hook
  filter keys
- HTTP status: `400 Bad Request`
- context:
  - emitted during ATM config loading before send execution proceeds
  - requires migration guidance that explains the recipient-scoped
    `[[atm.post_send_hooks]]` rule shape
  - flat legacy keys are ambiguous because they attempted to split sender and
    recipient filtering outside the new one-rule-per-recipient contract
  - must not be downgraded to a warning because the config cannot be safely
    interpreted under the current hook model

#### 5.8.3 `ATM_WARNING_HOOK_SKIPPED`

- code: `ATM_WARNING_HOOK_SKIPPED`
- description: retired for the hook filter non-match path; retained only as a
  historical registry entry for pre-fix behavior
- HTTP status: `200 OK`
- context:
  - hook filter non-match is expected behavior, not an operator-facing warning
  - delivery channel for filter non-match is debug-only structured diagnostics;
    it is not a caller-visible `warn!`, stderr warning, or send-result warning
    entry
  - the old warning template is retired for the filter non-match case and must
    not be emitted after this fix
  - actual caller-visible hook warnings now live only under
    `ATM_WARNING_HOOK_EXECUTION_FAILED`

#### 5.8.4 `ATM_WARNING_HOOK_EXECUTION_FAILED`

- code: `ATM_WARNING_HOOK_EXECUTION_FAILED`
- description: a configured post-send hook failed to start, exited non-zero,
  timed out, or otherwise failed during best-effort execution
- HTTP status: `200 OK`
- context:
  - emitted as a warning/diagnostic only after the mailbox send has already
    succeeded
  - this is the sole remaining caller-visible post-send-hook warning
  - must not roll back or convert a successful send into a command failure
  - may be accompanied by lower-level OS/process details and any structured
    hook result that was successfully parsed before failure

#### 5.8.5 `ATM_POST_SEND_PANE_MISSING`

- code: `ATM_POST_SEND_PANE_MISSING`
- description: a recipient marked for local tmux-backed post-send emission has
  no authoritative pane id in canonical roster state
- HTTP status: `200 OK`
- context:
  - emitted only after durable message persistence succeeds
  - must be logged with sender, recipient, recipient team, and message id
  - must surface as a sender-visible warning rather than rolling back send/ack
  - recovery should direct the operator to repair pane metadata through
    `atm teams update-member`

#### 5.8.6 `ATM_POST_SEND_TMUX_SEND_FAILED`

- code: `ATM_POST_SEND_TMUX_SEND_FAILED`
- description: ATM attempted local tmux-backed post-send emission but `tmux
  send-keys` failed or rejected the target pane
- HTTP status: `200 OK`
- context:
  - emitted only after durable message persistence succeeds
  - must be logged with sender, recipient, recipient team, pane id, and tmux
    failure detail

#### 5.8.7 `ATM_POST_SEND_GRAFT_UNAVAILABLE`

- code: `ATM_POST_SEND_GRAFT_UNAVAILABLE`
- description: ATM attempted graft-backed post-send emission but no graft
  advisory/session delivery surface was available
- HTTP status: `200 OK`
- context:
  - emitted only after durable message persistence succeeds
  - must surface as a sender-visible warning rather than rolling back send/ack
  - must preserve sender, recipient, recipient team, and message id in warning
    context so the degraded graft handoff is auditable
  - recovery should direct the operator to restore the graft advisory/session
    path before relying on automatic graft nudges

#### 5.8.8 `ATM_GRAFT_RECEIVER_ALREADY_ACTIVE`

- code: `ATM_GRAFT_RECEIVER_ALREADY_ACTIVE`
- description: a graft receiver activation conflicted with a live owner of the
  same canonical graft root, team, and agent endpoint
- HTTP status: `409 Conflict`
- context:
  - the existing receiver remains authoritative; the conflicting activation
    must not replace its socket or endpoint record
  - context identifies the canonical root, team, and agent but never includes
    the receiver capability
  - recovery is to stop or repair the competing graft session, then retry

#### 5.8.9 `ATM_POST_SEND_ADVISORY_DELIVERY_FAILED`

- code: `ATM_POST_SEND_ADVISORY_DELIVERY_FAILED`
- description: ATM reached the graft advisory/session handoff but delivery of
  the post-send event still failed
- HTTP status: `200 OK`
- context:
  - emitted only after durable message persistence succeeds
  - must surface as a sender-visible warning rather than rolling back send/ack
  - must preserve sender, recipient, recipient team, and message id in warning
    context so the failed graft advisory handoff is auditable
  - recovery should direct the operator to investigate the graft receiver
    availability or advisory transport health before retrying automated nudges
  - must surface as a sender-visible warning rather than rolling back send/ack
  - recovery should direct the operator to verify the pane still exists and
    repair stale pane metadata through `atm teams update-member`

### 5.9 Mailbox Lock Read-Only Filesystem

- `ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM`

#### 5.9.1 `ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM`

- code: `ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM`
- description: ATM could not create, update, or remove the mailbox lock
  sentinel because the underlying filesystem is read-only
- HTTP status: `500 Internal Server Error`
- context:
  - emitted for lock-path `open`, owner-record truncate/write, or stale-sentinel
    removal when the underlying OS reports a read-only filesystem
  - this is more specific than `ATM_MAILBOX_LOCK_FAILED` because the operator
    remediation is to restore writability or move the ATM home, not to wait for
    lock contention or adjust discretionary permissions
  - required platform classification:
    - Linux/macOS: raw OS error `EROFS` (`30`)
    - Windows: raw OS error `ERROR_WRITE_PROTECT` (`19`)
  - required output split:
    - message:
      ```text
      error: mailbox lock {operation} failed for {lock_path}: filesystem is read-only.
      ```
    - recovery:
      ```text
      Remount or move the ATM home to a writable filesystem, then retry the ATM command.
      ```

### 5.10 Runtime Families

The following families are part of the current runtime line. Store codes are
already materialized in `crates/atm-error/src/error_codes.rs`; the remaining
families stay documented here as the shared contract for the later runtime
surface.

#### 5.10.1 Store

- `ATM_STORE_OPEN_FAILED`
- `ATM_STORE_BOOTSTRAP_FAILED`
- `ATM_STORE_MIGRATION_FAILED`
- `ATM_STORE_QUERY_FAILED`
- `ATM_STORE_BUSY`
- `ATM_STORE_CONSTRAINT_VIOLATION`
- `ATM_STORE_TRANSACTION_FAILED`

#### 5.10.2 Ingest / Export

- `ATM_INGEST_FAILED`
- `ATM_WARNING_INGEST_BACKPRESSURE`
- `ATM_WARNING_INGEST_RECORD_SKIPPED`
- `ATM_EXPORT_FAILED`
- `ATM_EXPORT_REPLAY_FAILED`

#### 5.10.3 Transport

- `ATM_TRANSPORT_CONNECT_FAILED`
- `ATM_TRANSPORT_TIMEOUT`
- `ATM_TRANSPORT_PROTOCOL_FAILED`
- `ATM_TRANSPORT_REMOTE_UNREACHABLE`

#### 5.10.4 Daemon Runtime / Singleton / Client

- `ATM_DAEMON_ALREADY_RUNNING`
- `ATM_DAEMON_SINGLETON_RELEASE_FAILED`
- `ATM_DAEMON_RUNTIME_OVER_CAPACITY`
- `ATM_DAEMON_SHUTDOWN_TIMEOUT`
- `ATM_DAEMON_SIGNAL_RELOAD_FAILED`
- `ATM_DAEMON_UNAVAILABLE`
- `ATM_RUNTIME_ROOT_INVALID`
- `ATM_RUNTIME_BOOTSTRAP_REFUSED`
- `ATM_DAEMON_CLIENT_TIMEOUT`
- drop-time best-effort cleanup may log `ATM_DAEMON_CLIENT_TIMEOUT` as a
  warning because the mailbox command has already succeeded, but public
  acquisition/sweep paths must return the structured error directly
- `ATM_DAEMON_LAUNCH_GATE_REJECTED`
- `ATM_DAEMON_SERVING_STATE_REJECTED`
- `ATM_DAEMON_STALE_OWNER_RECOVERY_FAILED`
- `ATM_DAEMON_AUTO_START_FAILED`
- `ATM_TEST_FAKE_TRANSPORT_INJECTION_FAILED`

## 6. Mapping Rules

Required mapping rules:

- every `AtmErrorKind` maps to one or more specific `AtmErrorCode` values
- the code is more specific than the coarse `AtmErrorKind`
- warnings that do not become `AtmError` still use a registry code
- tests should assert the stable code, not only the human-readable message

| `AtmErrorKind` | Default `AtmErrorCode` | Additional implemented codes in the same kind |
| --- | --- | --- |
| `Config` | `ATM_CONFIG_PARSE_FAILED` | `ATM_CONFIG_HOME_UNAVAILABLE`, `ATM_HOME_UNRESOLVED`, `ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY`, `ATM_CONFIG_RETIRED_LEGACY_HOOK_KEYS`, `ATM_CONFIG_TEAM_PARSE_FAILED` |
| `MissingDocument` | `ATM_CONFIG_TEAM_MISSING` | none |
| `Address` | `ATM_ADDRESS_PARSE_FAILED` | none |
| `Identity` | `ATM_IDENTITY_UNAVAILABLE` | none |
| `TeamNotFound` | `ATM_TEAM_NOT_FOUND` | `ATM_TEAM_UNAVAILABLE` |
| `AgentNotFound` | `ATM_AGENT_NOT_FOUND` | none |
| `MailboxLock` | `ATM_MAILBOX_LOCK_FAILED` | `ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM`, `ATM_MAILBOX_LOCK_TIMEOUT` |
| `MailboxRead` | `ATM_MAILBOX_READ_FAILED` | none |
| `MailboxWrite` | `ATM_MAILBOX_WRITE_FAILED` | none |
| `FilePolicy` | `ATM_FILE_POLICY_REJECTED` | `ATM_FILE_REFERENCE_REWRITE_FAILED` |
| `Validation` | `ATM_MESSAGE_VALIDATION_FAILED` | `ATM_SELF_ADDRESSED_SEND_INVALID`, `ATM_ACK_INVALID_STATE`, `ATM_CLEAR_INVALID_STATE` |
| `Serialization` | `ATM_SERIALIZATION_FAILED` | none |
| `Timeout` | `ATM_WAIT_TIMEOUT` | none |
| `Store` | `ATM_STORE_QUERY_FAILED` | `ATM_STORE_OPEN_FAILED`, `ATM_STORE_BOOTSTRAP_FAILED`, `ATM_STORE_MIGRATION_FAILED`, `ATM_STORE_BUSY`, `ATM_STORE_CONSTRAINT_VIOLATION`, `ATM_STORE_TRANSACTION_FAILED` |
| `ObservabilityEmit` | `ATM_OBSERVABILITY_EMIT_FAILED` | none |
| `ObservabilityBootstrap` | `ATM_OBSERVABILITY_BOOTSTRAP_FAILED` | none |
| `ObservabilityQuery` | `ATM_OBSERVABILITY_QUERY_FAILED` | none |
| `ObservabilityFollow` | `ATM_OBSERVABILITY_FOLLOW_FAILED` | none |
| `ObservabilityHealth` | `ATM_OBSERVABILITY_HEALTH_FAILED` | `ATM_OBSERVABILITY_HEALTH_OK`, `ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED` |

Store-facing rusqlite layers classify persistence failures first, then command
adapters preserve those codes under the caller-visible `AtmErrorKind::Store`
coarse kind.

## 7. Recoverability Classification

Every documented `AtmErrorCode` must carry one recoverability classification.

Allowed classes:
- `retryable`
- `operator_actionable`
- `fail_closed`
- `warning_only`

Classification rules:
- warning-prefixed codes default to `warning_only`
- configuration and validation failures that require user change default to
  `operator_actionable`
- transport/store/runtime saturation and timeout failures default to
  `retryable` unless the documented recovery requires operator intervention
- invariant-preserving hard stops default to `fail_closed`

### 7.1 Classification By Code

| `AtmErrorCode` | Classification |
| --- | --- |
| `ATM_CONFIG_HOME_UNAVAILABLE` | `operator_actionable` |
| `ATM_HOME_UNRESOLVED` | `operator_actionable` |
| `ATM_CONFIG_PARSE_FAILED` | `operator_actionable` |
| `ATM_CONFIG_TEAM_PARSE_FAILED` | `operator_actionable` |
| `ATM_CONFIG_TEAM_MISSING` | `operator_actionable` |
| `ATM_IDENTITY_UNAVAILABLE` | `operator_actionable` |
| `ATM_ADDRESS_PARSE_FAILED` | `operator_actionable` |
| `ATM_TEAM_UNAVAILABLE` | `operator_actionable` |
| `ATM_TEAM_NOT_FOUND` | `operator_actionable` |
| `ATM_AGENT_NOT_FOUND` | `operator_actionable` |
| `ATM_MAILBOX_READ_FAILED` | `operator_actionable` |
| `ATM_MAILBOX_WRITE_FAILED` | `operator_actionable` |
| `ATM_MAILBOX_LOCK_FAILED` | `operator_actionable` |
| `ATM_MAILBOX_LOCK_TIMEOUT` | `retryable` |
| `ATM_MESSAGE_VALIDATION_FAILED` | `operator_actionable` |
| `ATM_SELF_ADDRESSED_SEND_INVALID` | `operator_actionable` |
| `ATM_SERIALIZATION_FAILED` | `fail_closed` |
| `TEMPLATE_CONTENT_NOT_UTF8` | `operator_actionable` |
| `ATM_FILE_POLICY_REJECTED` | `operator_actionable` |
| `ATM_FILE_REFERENCE_REWRITE_FAILED` | `operator_actionable` |
| `ATM_WAIT_TIMEOUT` | `retryable` |
| `ATM_ACK_INVALID_STATE` | `operator_actionable` |
| `ATM_CLEAR_INVALID_STATE` | `operator_actionable` |
| `ATM_OBSERVABILITY_HEALTH_OK` | `warning_only` |
| `ATM_OBSERVABILITY_EMIT_FAILED` | `operator_actionable` |
| `ATM_OBSERVABILITY_QUERY_FAILED` | `operator_actionable` |
| `ATM_OBSERVABILITY_FOLLOW_FAILED` | `operator_actionable` |
| `ATM_OBSERVABILITY_HEALTH_FAILED` | `operator_actionable` |
| `ATM_OBSERVABILITY_BOOTSTRAP_FAILED` | `operator_actionable` |
| `ATM_WARNING_INVALID_TEAM_MEMBER_SKIPPED` | `warning_only` |
| `ATM_WARNING_RESTORE_IN_PROGRESS` | `warning_only` |
| `ATM_WARNING_STALE_MAILBOX_LOCK` | `warning_only` |
| `ATM_WARNING_IDENTITY_DRIFT` | `warning_only` |
| `ATM_WARNING_BASELINE_MEMBER_MISSING` | `warning_only` |
| `ATM_WARNING_MAILBOX_RECORD_SKIPPED` | `warning_only` |
| `ATM_WARNING_MALFORMED_ATM_FIELD_IGNORED` | `warning_only` |
| `ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED` | `warning_only` |
| `ATM_WARNING_ORIGIN_INBOX_ENTRY_SKIPPED` | `warning_only` |
| `ATM_WARNING_MISSING_TEAM_CONFIG_FALLBACK` | `warning_only` |
| `ATM_WARNING_SEND_ALERT_STATE_DEGRADED` | `warning_only` |
| `ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY` | `operator_actionable` |
| `ATM_CONFIG_RETIRED_LEGACY_HOOK_KEYS` | `operator_actionable` |
| `ATM_WARNING_HOOK_SKIPPED` | `warning_only` |
| `ATM_WARNING_HOOK_EXECUTION_FAILED` | `warning_only` |
| `ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM` | `operator_actionable` |
| `ATM_STORE_OPEN_FAILED` | `operator_actionable` |
| `ATM_STORE_BOOTSTRAP_FAILED` | `operator_actionable` |
| `ATM_STORE_MIGRATION_FAILED` | `operator_actionable` |
| `ATM_STORE_QUERY_FAILED` | `operator_actionable` |
| `ATM_STORE_BUSY` | `retryable` |
| `ATM_STORE_CONSTRAINT_VIOLATION` | `fail_closed` |
| `ATM_STORE_TRANSACTION_FAILED` | `retryable` |
| `ATM_INGEST_FAILED` | `operator_actionable` |
| `ATM_WARNING_INGEST_BACKPRESSURE` | `warning_only` |
| `ATM_WARNING_INGEST_RECORD_SKIPPED` | `warning_only` |
| `ATM_EXPORT_FAILED` | `operator_actionable` |
| `ATM_EXPORT_REPLAY_FAILED` | `retryable` |
| `ATM_TRANSPORT_CONNECT_FAILED` | `retryable` |
| `ATM_TRANSPORT_TIMEOUT` | `retryable` |
| `ATM_TRANSPORT_PROTOCOL_FAILED` | `fail_closed` |
| `ATM_TRANSPORT_REMOTE_UNREACHABLE` | `retryable` |
| `ATM_DAEMON_ALREADY_RUNNING` | `operator_actionable` |
| `ATM_DAEMON_SINGLETON_RELEASE_FAILED` | `operator_actionable` |
| `ATM_DAEMON_RUNTIME_OVER_CAPACITY` | `retryable` |
| `ATM_DAEMON_SHUTDOWN_TIMEOUT` | `operator_actionable` |
| `ATM_DAEMON_SIGNAL_RELOAD_FAILED` | `operator_actionable` |
| `ATM_DAEMON_UNAVAILABLE` | `operator_actionable` |
| `ATM_RUNTIME_ROOT_INVALID` | `operator_actionable` |
| `ATM_RUNTIME_BOOTSTRAP_REFUSED` | `fail_closed` |
| `ATM_DAEMON_CLIENT_TIMEOUT` | `retryable` |
| `ATM_DAEMON_LAUNCH_GATE_REJECTED` | `fail_closed` |
| `ATM_DAEMON_SERVING_STATE_REJECTED` | `fail_closed` |
| `ATM_DAEMON_STALE_OWNER_RECOVERY_FAILED` | `operator_actionable` |
| `ATM_DAEMON_AUTO_START_FAILED` | `operator_actionable` |
| `ATM_TEST_FAKE_TRANSPORT_INJECTION_FAILED` | `operator_actionable` |
| `ATM_IDENTITY_INVALID` | `operator_actionable` |
| `ATM_IDENTITY_CONFLICT` | `operator_actionable` |
| `ATM_MEMBER_ALREADY_EXISTS` | `operator_actionable` |
| `ATM_MEMBER_NOT_FOUND` | `operator_actionable` |
| `ATM_SOCKET_OVERRIDE_FORBIDDEN` | `fail_closed` |
| `ATM_DAEMON_MAY_HAVE_EXECUTED` | `fail_closed` |
| `ATM_DAEMON_LIFECYCLE_WEDGE` | `operator_actionable` |
| `ATM_DAEMON_CONNECTION_SATURATED` | `retryable` |
| `REMOTE_DELIVERY_UNCONFIRMED` | `retryable` |
| `ATM_PEER_CONFIG_VALIDATION_FAILED` | `operator_actionable` |
| `ATM_CERTIFICATE_OPERATION_FAILED` | `operator_actionable` |
| `ATM_BIND_PREFLIGHT_FAILED` | `operator_actionable` |
| `ATM_CLIENT_DAEMON_VERSION_INCOMPATIBLE` | `operator_actionable` |
| `ATM_TEAM_INVALID` | `operator_actionable` |
| `ATM_INTERNAL_ERROR` | `fail_closed` |
| `ATM_SEARCH_LOCAL_ONLY` | `operator_actionable` |
| `ATM_LOCAL_HTTP_CAPABILITY_INVALID` | `operator_actionable` |
| `ATM_LOCAL_HTTP_ENDPOINT_MISSING` | `operator_actionable` |
| `ATM_LOCAL_HTTP_ENDPOINT_NON_LOOPBACK` | `fail_closed` |
| `ATM_LOCAL_HTTP_RUNTIME_DIRECTORY_MISSING` | `operator_actionable` |
| `ATM_LOCAL_HTTP_CAPABILITY_REVOKED` | `fail_closed` |
| `ATM_MESSAGE_ID_CONFLICT` | `fail_closed` |
| `ATM_NUDGE_TEMPLATE_BODY_EMPTY` | `operator_actionable` |
| `ATM_CALLER_CONTEXT_REQUEST_INVALID` | `operator_actionable` |
| `DECOMPOSED_TEMPLATE_INCLUDE_FORBIDDEN` | `operator_actionable` |
| `TEMPLATE_LOAD_FAILED` | `operator_actionable` |
| `TEMPLATE_HASH_API_FAILED` | `operator_actionable` |
| `TEMPLATE_INSPECTION_PARSE_FAILED` | `operator_actionable` |
| `TEMPLATE_REQUIRED_VARIABLE_MISSING` | `operator_actionable` |
| `TEMPLATE_RENDER_VERIFICATION_FAILED` | `fail_closed` |
| `TEMPLATE_INCLUDE_UNRESOLVED` | `operator_actionable` |
| `TEMPLATE_CLASSIFICATION_INVALID` | `operator_actionable` |
| `TEMPLATE_WORKFLOW_INVALID` | `operator_actionable` |
| `TEMPLATE_WORKFLOW_VALUE_INVALID` | `operator_actionable` |
| `TEMPLATE_TAG_RESERVED` | `operator_actionable` |
| `ATM_WARNING_SQLITE_HEALTH_DEGRADED` | `warning_only` |
| `ATM_WARNING_ROSTER_DRIFT` | `warning_only` |
| `ATM_POST_SEND_PANE_MISSING` | `retryable` |
| `ATM_POST_SEND_TMUX_SEND_FAILED` | `retryable` |
| `ATM_POST_SEND_GRAFT_UNAVAILABLE` | `retryable` |
| `ATM_GRAFT_RECEIVER_ALREADY_ACTIVE` | `operator_actionable` |
| `ATM_POST_SEND_ADVISORY_DELIVERY_FAILED` | `retryable` |
| `ATM_HELP_TOPIC_NOT_FOUND` | `operator_actionable` |

## 8. Evolution Rules

- Add new codes here before implementation lands.
- Do not reuse an existing code for a different failure meaning.
- If a code must be retired, leave it documented as deprecated rather than
  silently removing history.
