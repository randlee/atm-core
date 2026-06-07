# Phase Yb Testing And Validation

## Purpose

Define the shared validation matrix for Yb so every sprint uses the same
baseline commands and the same degraded-delivery proof requirements.

## Common Validation

Every Yb implementation sprint must pass:

```bash
cargo fmt --all --check
python3 .just/run_lint.py all
cargo build --workspace
cargo test --workspace
git diff --check
```

## Sprint-Specific Validation

### Y.7

Required proof:

- named tests for each degraded-delivery branch
- assertions over logical payload content and ordering
- no reliance on hook invocation count as delivery proof
- required named tests on the `Y.7` branch:
  - `sqlite_failure_for_claude_preserves_original_and_companion_error_payloads`
  - `sqlite_failure_for_non_claude_preserves_original_and_companion_payloads`
  - `append_failure_after_sqlite_commit_is_execution_only`
  - `named_plan_builder_proves_payload_equality_across_harnesses`
  - `named_companion_error_failure_handling_adds_explicit_warning`
  - `send_non_claude_sqlite_failure_delivers_original_and_error_via_outbound_boundary`
  - `send_append_failure_routes_to_post_send_hook_fallback`
- the `Y.7` closeout branch must also prove:
  - `crates/atm-core/src/ack/mod.rs::AckReplyStateMachine` constructs
    `ReplyDeliveryPlan` from the same typed persistence result surface as send
  - `ReplyDeliveryPlan` executes through
    `execute_reply_delivery_plan(...)`, not an ack-only notification shim

### Y.8

Required proof:

- grep/lint evidence that forbidden outer callers no longer branch on harness
- typed fail-closed tests for unsupported route requests

### Y.9

Required proof:

- non-Claude payload delivery uses `NonClaudeOutbound`
- notification-only hooks remain separate
- shared executor call graph is the same around both harness families

### Y.10

Required proof:

- boundary TOML files and docs match the final caller allowlists
- removal-ledger closure is complete or tracked as an explicit blocker set
- smoke-handoff docs are updated only after Yb enforcement passes
- normal Claude append delivery rejects JSON-array inbox files and points callers
  at the explicit repair/rebuild seam
- full mailbox rewrite remains reachable only through the repair/rebuild path,
  not as a silent runtime append fallback

### Y.11

Required proof:

- the low-level Claude append seam is never selected for
  `DeliveryHarnessPath::NonClaude`
- the repair/rebuild refresh seam is explicit and no longer encoded as a
  generic recipient-routed helper with a non-Claude no-op branch
- shared validation docs name the final outbound-boundary proof model

## Cross-Platform Note

Yb does not reopen host-specific transport design, but it does affect daemon
delivery boundaries that are used on every supported operating system.

Validation therefore must keep the supported-platform parity rule intact:

- Windows remains a supported same-host ATM target
- macOS/Linux must not be treated as the only runtime truth for new delivery
  boundaries
- no Yb acceptance criterion may rely on Unix-only behavior for correctness
