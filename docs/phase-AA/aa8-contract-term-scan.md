# AA.8 Contract-Term Scan

Branch:
- `feature/pAA-s8-claude-schema-contract`

Purpose:
- preserve the QA artifact proving AA.8 removed stale wording that treated the
  current Claude inbox JSON-array shape as legacy

Recorded scan intent:
- banned stale wording set:
  - the old phrase that called the current Claude inbox JSON-array shape
    "legacy"
  - the old phrase that called the current Claude inbox JSON-array shape
    "array-backed"
- scan roots:
  - `docs/`
  - `tools/`
  - `crates/`
  - `scripts/`

Result:
- no matches are permitted in the AA.8 validated branch state
- `metadata.atm` may still appear in historical/read-compat documentation, but
  it must not be described as the current forward-write contract
