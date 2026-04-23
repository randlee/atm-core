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
- `ATM_CONFIG_RETIRED_LEGACY_HOOK_KEYS`
- `ATM_WARNING_HOOK_SKIPPED` (retired for filter non-match)
- `ATM_WARNING_HOOK_EXECUTION_FAILED`

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
- description: `.atm.toml` contains the retired flat post-send-hook keys
  `[atm].post_send_hook`, `[atm].post_send_hook_senders`, or
  `[atm].post_send_hook_recipients` instead of one or more explicit
  `[[atm.post_send_hooks]]` rules
- HTTP status: `400 Bad Request`
- context:
  - emitted during ATM config loading before send execution proceeds
  - applies to the legacy flat-key hook shape as a whole, even when only one
    of the retired keys is present
  - `{config_path}` resolves to the discovered `.atm.toml` path that contained
    the retired key set
  - expected output split:
    - message:
      ```text
      error: '{config_path}' uses retired post-send hook keys. Use [[atm.post_send_hooks]] with recipient and command entries instead.
      ```
    - recovery:
      ```text
      Replace [atm].post_send_hook, [atm].post_send_hook_senders, and [atm].post_send_hook_recipients with one or more [[atm.post_send_hooks]] rules, each containing recipient = "name-or-*" and command = ["argv", ...].
      ```
  - the rendered CLI output may display the message and recovery together, but
    ATM stores them as separate fields on the structured error
  - must not be downgraded to a generic config parse failure because callers
    and tests need a stable migration-specific code

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

## 6. Mapping Rules

Required mapping rules:

- every `AtmErrorKind` maps to one or more specific `AtmErrorCode` values
- the code is more specific than the coarse `AtmErrorKind`
- warnings that do not become `AtmError` still use a registry code
- tests should assert the stable code, not only the human-readable message

| `AtmErrorKind` | Default `AtmErrorCode` | Additional implemented codes in the same kind |
| --- | --- | --- |
| `Config` | `ATM_CONFIG_PARSE_FAILED` | `ATM_CONFIG_HOME_UNAVAILABLE`, `ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY`, `ATM_CONFIG_RETIRED_LEGACY_HOOK_KEYS`, `ATM_CONFIG_TEAM_PARSE_FAILED` |
| `MissingDocument` | `ATM_CONFIG_TEAM_MISSING` | none |
| `Address` | `ATM_ADDRESS_PARSE_FAILED` | none |
| `Identity` | `ATM_IDENTITY_UNAVAILABLE` | none |
| `TeamNotFound` | `ATM_TEAM_NOT_FOUND` | `ATM_TEAM_UNAVAILABLE` |
| `AgentNotFound` | `ATM_AGENT_NOT_FOUND` | none |
| `MailboxLock` | `ATM_MAILBOX_LOCK_FAILED` | `ATM_MAILBOX_LOCK_TIMEOUT` |
| `MailboxRead` | `ATM_MAILBOX_READ_FAILED` | none |
| `MailboxWrite` | `ATM_MAILBOX_WRITE_FAILED` | none |
| `FilePolicy` | `ATM_FILE_POLICY_REJECTED` | `ATM_FILE_REFERENCE_REWRITE_FAILED` |
| `Validation` | `ATM_MESSAGE_VALIDATION_FAILED` | `ATM_ACK_INVALID_STATE`, `ATM_CLEAR_INVALID_STATE` |
| `Serialization` | `ATM_SERIALIZATION_FAILED` | none |
| `Timeout` | `ATM_WAIT_TIMEOUT` | none |
| `ObservabilityEmit` | `ATM_OBSERVABILITY_EMIT_FAILED` | none |
| `ObservabilityBootstrap` | `ATM_OBSERVABILITY_BOOTSTRAP_FAILED` | none |
| `ObservabilityQuery` | `ATM_OBSERVABILITY_QUERY_FAILED` | none |
| `ObservabilityFollow` | `ATM_OBSERVABILITY_FOLLOW_FAILED` | none |
| `ObservabilityHealth` | `ATM_OBSERVABILITY_HEALTH_FAILED` | `ATM_OBSERVABILITY_HEALTH_OK`, `ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED` |

## 7. Evolution Rules

- Add new codes here before implementation lands.
- Do not reuse an existing code for a different failure meaning.
- If a code must be retired, leave it documented as deprecated rather than
  silently removing history.
