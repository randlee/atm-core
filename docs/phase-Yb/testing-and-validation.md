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

## Cross-Platform Note

Yb does not reopen host-specific transport design, but it does affect daemon
delivery boundaries that are used on every supported operating system.

Validation therefore must keep the supported-platform parity rule intact:

- Windows remains a supported same-host ATM target
- macOS/Linux must not be treated as the only runtime truth for new delivery
  boundaries
- no Yb acceptance criterion may rely on Unix-only behavior for correctness
