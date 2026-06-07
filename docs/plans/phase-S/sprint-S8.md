# Phase S.8 — Claude JSONL Compatibility Envelope

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.8"
status: in-review
estimated_scope: M
```

## Goal

Implement the ATM-authored Claude JSONL compatibility-envelope rules from
ADR-010 so oversized ATM-authored bodies stay durable in SQLite without
flooding Claude-facing inbox surfaces or causing watcher churn.

## Governing Requirements

- `REQ-CORE-COMPAT-001`
- `REQ-CORE-MAILBOX-001`
- `REQ-P-RELIABILITY-001`
- `REQ-CORE-CONFIG-001`

## Governing ADRs

- `docs/adr/ADR-009-bounded-queue-query-surface.md`
- `docs/adr/ADR-010-claude-jsonl-compatibility-envelope.md`

## Hard Dependencies

- S.7 bounded queue-query implementation is merged
- the durable message source of truth remains SQLite
- watcher/reconcile import/export stays behind the documented daemon and
  inbox-export boundaries

## Required Work

1. Implement ATM-authored JSONL body export limits.
1.1 Add config parsing and validation for
    `[atm].claude_jsonl_body_export_max_bytes`.
1.2 Default the export cap to `128 KiB`.
1.3 Support `0` as stub-only ATM-authored export.

2. Implement oversized ATM-authored compatibility projection.
2.1 When an ATM-authored body exceeds the configured export limit, replace the
    JSONL `text` field with exactly:
   - `atm read --message-id <id>`
2.2 Preserve summary text and ATM metadata while keeping the full body durable
    in SQLite.
2.3 Never rewrite Claude-native inbound messages into retrieval stubs.

3. Make watcher/reconcile projection-aware.
3.1 Treat ATM-authored compatibility projection updates as idempotent for the
    same logical message.
3.2 Prevent self-induced churn loops when the same retrieval stub for the same
    `message_id` is re-observed.

4. Align the export and reconcile paths across core and daemon adapters.
4.1 Keep config ownership in `atm-core`.
4.2 Keep compatibility projection wiring behind the inbox-export boundaries.
4.3 Keep daemon watcher/reconcile behavior aligned with the no-churn rule.

5. Track issue #219: tilde expansion in post-send hook paths.
5.1 During ATM config normalization, expand leading `~`, `~/`, and `~\` in
    `[[atm.post_send_hooks]].command[0]` to the current user home directory.
5.2 Relative paths remain relative to the declaring `.atm.toml`.
5.3 Bare executables without a path separator continue to resolve via `PATH`.
5.4 Add the normalization work alongside the existing S.8 config changes in:
   - `crates/atm-core/src/config/mod.rs`
   - `crates/atm-core/src/config/types.rs`
5.5 Record issue reference:
   - `#219`

## Required Code Targets

- `crates/atm-core/src/config/mod.rs`
- `crates/atm-core/src/config/types.rs`
- `crates/atm-core/src/config/discovery.rs`
- `crates/atm-core/src/home.rs`
- `crates/atm-core/src/schema/inbox_message.rs`
- `crates/atm-core/src/mailbox/atomic.rs`
- `crates/atm-core/src/mailbox/store.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/reconcile_runtime.rs`

## Required Document Updates

- `docs/claude-code-message-schema.md`
- `docs/atm-core/modules/config.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`

## Implementation Notes

- The compatibility-envelope rule is enforced at the `atm-core` config and
  mailbox-export seam, not by rewriting watcher or reconcile runtime logic.
- The daemon-side no-churn proof is covered by boundary-adapter tests that
  re-export and re-import the same ATM-authored logical message and verify
  identity stability across retrieval-stub projection.

## Acceptance Criteria

- ATM-authored JSONL export defaults to a `128 KiB` body cap
- the export cap is configurable downward to `0`
- oversized ATM-authored messages export the exact retrieval stub
  `atm read --message-id <id>`
- summary text remains present when the retrieval stub is exported
- full ATM-authored bodies remain durable in SQLite
- watcher/reconcile logic treats ATM-authored compatibility projection updates
  as idempotent and does not generate self-induced churn loops
- post-send hook command normalization expands leading home-directory tildes
  per issue `#219` while preserving relative-path and bare-executable rules

## Required Validation

- `just lint`
- compatibility-export tests in `atm-core`
- watcher/reconcile no-churn tests in `atm-daemon`
