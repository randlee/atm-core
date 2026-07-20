# ADR-028A — Unified Error Type

| Field | Value |
| --- | --- |
| ID | ADR-028A |
| Status | **Accepted** |
| Date | 2026-07-20 |
| Deciders | Rand Lee |
| Relates to | ADR-028B (closed protocol), ADR-028C (cross-host implementation) |

## Decision

One error struct, one code enum, one message per code, everywhere — local
calls and wire responses alike.

```rust
pub struct AtmError {
    pub code: AtmErrorCode,
    pub message: String,
}
```

- No `kind` field. No `recovery` field. No `source`. No `backtrace`.
- Every failure is logged via `tracing::error!` at the point it's
  constructed — that is where debugging context belongs, not in the error
  value itself.
- No separate wire-serialized error type. `AtmError` is trivially
  serializable and *is* the wire type; there is no build → strip → send →
  reconstruct step at any layer.
- Each `AtmErrorCode` variant has **exactly one** constructor function,
  defined once, owning both the code and its canonical message template.
  Call sites supply only dynamic values — none author message text, none
  construct `AtmErrorCode::Variant` inline.

```rust
// atm-core/src/errors.rs — the only file that authors error text
impl AtmErrorCode {
    pub fn mailbox_write_failed(inboxes_dir: &Path, team: &TeamName, cause: Option<&str>) -> AtmError {
        let message = match cause {
            Some(c) => format!("mailbox write failed for '{team}' at {}: {c}", inboxes_dir.display()),
            None => format!("mailbox write failed for '{team}' at {}", inboxes_dir.display()),
        };
        AtmError { code: Self::MailboxWriteFailed, message }
    }
}
```

`observability::ErrorCode(String)` is **not** merged into `AtmErrorCode` — it
is a shared, cross-project diagnostic-name type (`sc_observability_types`)
used for things that aren't ATM errors at all (e.g. sink-health
diagnostics). It is bridged instead, so every logged identifier matches the
wire code's name exactly, with no second hand-typed string to drift:

```rust
impl From<AtmErrorCode> for observability::ErrorCode {
    fn from(code: AtmErrorCode) -> Self {
        Self::new_owned(code.as_str()).expect("AtmErrorCode variants are valid names")
    }
}
```

## Punchlist — every error type found, and its disposition

| Type | Location | Disposition |
| --- | --- | --- |
| `AtmError` (struct: `code`, `kind`, `message`, `recovery: Vec<String>`, `source: Option<Box<dyn StdError + Send + Sync>>`, `backtrace: Backtrace`) | `atm-storage/src/error.rs` (re-exported via `atm-core/src/error.rs`) | **Reduced** to `{ code, message }`. All other fields deleted. |
| `AtmErrorKind` (enum: `Config`, `MissingDocument`, `Address`, `Identity`, `DaemonUnavailable`, `TeamNotFound`, `AgentNotFound`, `MailboxLock`, `MailboxRead`, `MailboxWrite`, `FilePolicy`, `Internal`, `Validation`, `Serialization`, `Timeout`, …) | `atm-storage/src/error.rs` | **Deleted.** Provably redundant — `error_kind_for_code(code)` already derives it from `AtmErrorCode`. Any caller needing coarse grouping derives the same mapping locally. |
| `AtmErrorCode` (enum, ~40+ variants) | `atm-storage/src/error_codes.rs` (re-exported via `atm-core/src/error_codes.rs`) | **Kept.** This is the one real taxonomy. |
| `ProtocolErrorEnvelope` (struct: `code`, `message`, `recovery`) | `atm-core/src/protocol.rs` | **Deleted.** Existed purely because `AtmError` couldn't cross the wire (non-serializable `source`/`backtrace`). With those fields gone, `AtmError` serializes directly — no wrapper, no `into_atm_error()` reconstruction step. |
| `observability::ErrorCode(String)` | `atm-core/src/observability.rs` | **Kept**, bridged via `From<AtmErrorCode>` (above). Not merged — it legitimately represents non-`AtmError` diagnostics too. |
| `UnknownAtmErrorCode(String)` | `atm-storage/src/error_codes.rs` | Unaffected — parse-error helper for the code enum itself, orthogonal to this consolidation. |
| `AtmMessageIdParseError(String)` | `atm-storage/src/schema/inbox_message.rs` | Unaffected — unrelated (ULID parse error, not a failure-reporting type). |

## Verified sprawl (spot-checked, not assumed)

Construction-site count per code, grep'd directly, non-test:

| `AtmErrorCode` variant | Construction sites found |
| --- | --- |
| `InternalError` | 4 |
| `DaemonUnavailable` | 12 |
| `ClientDaemonVersionIncompatible` | 4 |
| `MailboxWriteFailed` | 11 |
| `MessageValidationFailed` | 9 |

Concrete duplicate-message example: `AtmErrorCode::MailboxWriteFailed` has two
different hand-typed messages within `doctor/mod.rs` alone —

- `"inbox directory is missing at {path} for '{team}'"` (line 644)
- `"inbox directory is not writable at {path}: {error}"` (line 660)

— and 9 more call sites elsewhere, each free to author its own wording.
Post-consolidation, all 11 call to one constructor function; none type
message text again.

## Enforcement (mechanical, not documentation)

A lint/grep-based CI check enforcing **exactly one** non-test construction
site per `AtmErrorCode` variant — the constructor function itself. This is
the same category of fix as the `io_forbidden` gap identified in ADR-028C: a
build failure, not a convention someone has to remember to follow.

## Consequences

- Deleting `AtmErrorKind` and `ProtocolErrorEnvelope` removes an entire
  round-trip re-wrapping step from every request/response cycle.
- Message text for any given failure is singular and greppable — "what does
  `MailboxWriteFailed` say" has one answer, not up to 12.
- Deep debugging context (source chain, backtrace) moves entirely to
  `tracing` logs at construction time; the error value itself is no longer
  expected to carry it.
