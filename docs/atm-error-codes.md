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

- `crates/atm-core/src/error_codes.rs`

Required rules:

- `AtmError` carries one `AtmErrorCode`
- CLI bootstrap and parse/validation failure logging must also use
  `AtmErrorCode`
- warning diagnostics emitted during degraded recovery must also use
  `AtmErrorCode`
- logs must not hardcode free-form code strings outside the central registry

## 4. Naming Rules

ATM-owned error codes use:

- prefix: `ATM_`
- uppercase snake case
- stable semantic meaning across CLI rendering, structured logs, and tests

Error codes should describe the failure class, not a specific prose message.

## 5. Registry

### 5.1 Config And Identity

- `ATM_CONFIG_HOME_UNAVAILABLE`
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
- `ATM_MAILBOX_RECORD_SKIPPED`
- `ATM_MESSAGE_VALIDATION_FAILED`
- `ATM_SERIALIZATION_FAILED`

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
- `ATM_WARNING_IDENTITY_DRIFT`
- `ATM_WARNING_BASELINE_MEMBER_MISSING`
- `ATM_WARNING_MAILBOX_RECORD_SKIPPED`
- `ATM_WARNING_MALFORMED_ATM_FIELD_IGNORED`
- `ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED`
- `ATM_WARNING_ORIGIN_INBOX_ENTRY_SKIPPED`
- `ATM_WARNING_MISSING_TEAM_CONFIG_FALLBACK`
- `ATM_WARNING_SEND_ALERT_STATE_DEGRADED`

### 5.8 Post-Send Hook

- `ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY`
- `ATM_WARNING_HOOK_SKIPPED`
- `ATM_WARNING_HOOK_EXECUTION_FAILED`

#### 5.8.1 `ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY`

- code: `ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY`
- description: `.atm.toml` contains the retired `post_send_hook_members` key
  instead of the explicit `post_send_hook_senders` /
  `post_send_hook_recipients` keys
- HTTP status: `400 Bad Request`
- context:
  - emitted during ATM config loading before send execution proceeds
  - requires migration guidance that explains sender- versus
    recipient-triggered hook filters and the `*` wildcard
  - expected message template:
    ```text
    error: '.atm.toml' field 'post_send_hook_members' is no longer supported.
    Use 'post_send_hook_senders' (match on sender identity) and/or
    'post_send_hook_recipients' (match on recipient name) under [atm].
    Use '*' to match all senders or all recipients.
    ```
  - must not be downgraded to a warning because the old key is ambiguous under
    the redesigned contract

#### 5.8.2 `ATM_WARNING_HOOK_SKIPPED`

- code: `ATM_WARNING_HOOK_SKIPPED`
- description: a post-send hook was configured, but neither the sender nor the
  recipient trigger filters matched the current send
- HTTP status: `200 OK`
- context:
  - emitted as a warning/diagnostic only after a successful send
  - should include the resolved sender, resolved recipient, and configured
    sender/recipient filter values to make the mismatch actionable
  - expected message template:
    ```text
    post-send hook skipped: sender {sender} not in post_send_hook_senders {senders}
    and recipient {recipient} not in post_send_hook_recipients {recipients}
    ```
  - delivery channel: user-visible `warn!` / stderr via normal tracing log
    routing; not debug-only and not suppressible
  - covers explicit no-match outcomes only when at least one sender or
    recipient filter list is configured; it is not used for hook process
    failures or for a hook that is configured-but-disabled with both lists
    omitted/empty

#### 5.8.3 `ATM_WARNING_HOOK_EXECUTION_FAILED`

- code: `ATM_WARNING_HOOK_EXECUTION_FAILED`
- description: a configured post-send hook failed to start, exited non-zero,
  timed out, or otherwise failed during best-effort execution
- HTTP status: `200 OK`
- context:
  - emitted as a warning/diagnostic only after the mailbox send has already
    succeeded
  - must not roll back or convert a successful send into a command failure
  - may be accompanied by lower-level OS/process details and any structured
    hook result that was successfully parsed before failure

## 6. Mapping Rules

Required mapping rules:

- every `AtmErrorKind` maps to one or more specific `AtmErrorCode` values
- the code is more specific than the coarse `AtmErrorKind`
- warnings that do not become `AtmError` still use a registry code
- tests should assert the stable code, not only the human-readable message

## 7. Evolution Rules

- Add new codes here before implementation lands.
- Do not reuse an existing code for a different failure meaning.
- If a code must be retired, leave it documented as deprecated rather than
  silently removing history.
