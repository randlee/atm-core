# AG delete-list enforcement

This directory contains the hard fail-closed sprint gates for Phase AG cleanup
work.

Each `agNN.toml` file defines one sprint contract:

- `scan_files`
  - exact files scanned for forbidden symbols and workaround paths
- `forbidden_literals`
  - exact strings that must not remain in code
- `forbidden_regexes`
  - branch/path-shape patterns that must not remain in code
- `allowed_changed_files`
  - only files that may change while executing that sprint
- `min_net_crates_loc`
  - required maximum net `crates/` LOC delta for that sprint
  - negative values mean the sprint must delete more lines than it adds

The enforcement lives in:

- `crates/atm-architecture/tests/boundary_enforcement.rs`

There are two gate modes.

## 1. Delete-list gate

This scans the named files and fails if forbidden symbols or workaround-path
patterns are still present.

Run:

```bash
cargo test -p atm-architecture --test boundary_enforcement \
  ag_delete_lists_must_have_no_forbidden_symbols_or_workaround_paths
```

This is intentionally failing today until the AG cleanup deletions are actually
done.

## 2. Active sprint diff gate

This checks a concrete sprint execution diff:

- only allowlisted files changed
- no unauthorized edits under `crates/atm-architecture/**`
- `crates/` net LOC meets the sprint threshold

Required environment:

- `ATM_ARCH_ACTIVE_SPRINT`
  - sprint id, for example `AG.18`
- `ATM_ARCH_DIFF_BASE`
  - base commit/branch for the sprint diff
- `ATM_ARCH_DIFF_HEAD`
  - head commit/branch for the sprint diff

Optional environment:

- `ATM_ARCH_ALLOW_GATE_CHANGES=1`
  - only for dedicated gate-maintenance work
  - do not use during normal sprint execution

Run directly:

```bash
ATM_ARCH_ACTIVE_SPRINT=AG.18 \
ATM_ARCH_DIFF_BASE=origin/develop \
ATM_ARCH_DIFF_HEAD=HEAD \
cargo test -p atm-architecture --test boundary_enforcement \
  active_sprint_diff_gate_must_hold_when_configured
```

Or use the wrapper:

```bash
scripts/check-ag-sprint-gates.sh AG.18 origin/develop HEAD
```

## Design intent

These files are meant to make the cleanup line mechanically enforceable:

- if a forbidden split path still exists, the build fails
- if a workaround path is reintroduced, the build fails
- if a sprint edits files outside scope, the build fails
- if a delete sprint grows code instead of shrinking it, the build fails

This is not advisory review logic. It is a merge gate.
