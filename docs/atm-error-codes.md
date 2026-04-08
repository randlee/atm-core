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
- `ATM_MAILBOX_RECORD_SKIPPED`
- `ATM_MESSAGE_VALIDATION_FAILED`
- `ATM_SERIALIZATION_FAILED`

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
