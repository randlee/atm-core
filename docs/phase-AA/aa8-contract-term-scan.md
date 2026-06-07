# AA.8 Contract-Term Scan

Branch:
- `feature/pAA-s8-claude-schema-contract`

Purpose:
- preserve the QA artifact proving AA.8 removed stale wording that treated the
  current Claude inbox JSON-array shape as legacy

Executed scan:
- banned stale wording set:
  - `legacy array inbox`
  - `array-backed inbox`
- scan roots:
  - `docs/`
  - `tools/`
  - `crates/`
  - `scripts/`
- file count checked: `610`
- command:

  ```bash
  rg -n 'legacy array inbox|array-backed inbox' docs tools crates scripts \
    -g '!docs/phase-AA/aa8-contract-term-scan.md' -S
  ```
- exit code: `1`
- stdout:

  ```text
  <no matches>
  ```

Result:
- no matches are permitted in the AA.8 validated branch state
- `metadata.atm` may still appear in historical/read-compat documentation, but
  it must not be described as the current forward-write contract
