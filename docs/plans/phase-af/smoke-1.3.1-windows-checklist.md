# Smoke 1.3.1 — Windows checklist

## Purpose

This is the Windows execution checklist for the 1.3.1 release candidate at
`develop` commit `98a4e66c`. It is designed for a separate Windows Codex agent
run coordinated directly by the user.

No macOS verdict should be inferred from this file. This is Windows-only.

## Candidate under test

- branch/worktree content should match `develop` at `98a4e66c`
- workspace version should resolve to `1.3.1`

## Durable Windows result publication

The Windows agent must publish its durable results back into this branch under
`reports/smoke/` using the same timestamped convention as the macOS lane.

Required result files:

- `reports/smoke/<timestamp>-smoke-1.3.1-windows.md`
- `reports/smoke/<timestamp>-smoke-1.3.1-windows.json`

The Windows agent should also keep the standard smoke artifacts emitted by:

- `python scripts/smoke/run.py fast --write-artifacts`
- `python scripts/smoke/run.py normal --write-artifacts`
- `python scripts/smoke/run.py thorough --write-artifacts`

## Required evidence to collect

For every row below, record:

- exact command
- stdout/stderr or transcript excerpt
- PASS / FAIL
- if failed, first failing command and concrete observed behavior

## Preconditions

1. Use a real Windows host.
2. Start from a clean OS-user session with no ambient `atm-daemon.exe`.
3. Use the candidate worktree, not an older installed binary from PATH.
4. Confirm `cargo --version` and `python --version` are available.
5. Confirm no legacy wrapper usage; use native `atm` CLI only.

## Matrix

| Row ID | Goal | Command(s) | Expected result |
| --- | --- | --- | --- |
| `AF31-WIN-001` | workspace version check | `rg -n '^version = \"1.3.1\"' Cargo.toml` | workspace root reports `1.3.1` |
| `AF31-WIN-002` | fast lane | `python scripts/smoke/run.py fast --write-artifacts` | exits `0`; `reports/smoke/*smoke-fast*` shows `status: passed` |
| `AF31-WIN-003` | normal lane | `python scripts/smoke/run.py normal --write-artifacts` | exits `0`; `reports/smoke/*smoke*` shows `status: passed` |
| `AF31-WIN-004` | thorough lane | `python scripts/smoke/run.py thorough --write-artifacts` | exits `0`; `reports/smoke/*smoke-thorough*` shows `status: passed` |
| `AF31-WIN-005` | direct shared-host release lane | `python scripts/smoke/run_thorough_shared_host.py` | exits `0`; proves shared-host singleton/send-input path on isolated Windows host |
| `AF31-WIN-006` | AF-1 singleton expectation | inspect `AF31-WIN-005` output plus doctor artifacts | exactly one daemon owner pid during the run; no leaked daemon afterward |
| `AF31-WIN-007` | AF-2 observability/release gates | inspect `AF31-WIN-003`/`004` artifacts | doctor path, retained logs, removed-flag preflight, and version/report rows stay green |
| `AF31-WIN-008` | AF-3 native send-input integrity | inspect `AF31-WIN-005` output | inline, stdin, and file send paths all preserve expected durable bodies; invalid stdin fails locally without changing daemon pid set |
| `AF31-WIN-009` | real same-host `atm-graft` host lane | `python scripts/smoke/run_graft_same_host.py` | exits `0`; the real graft host registers, consumes an advisory nudge, and completes unary read/ack/send through the shared daemon contract |

## Required command details

### 1. Version / branch context

Run:

```bash
git rev-parse --short HEAD
rg -n '^version = "1.3.1"' Cargo.toml
```

Pass if:

- HEAD is the expected candidate commit or the user-approved equivalent
- the workspace root Cargo.toml reports `1.3.1`

### 2. Fast smoke

Run:

```bash
python scripts/smoke/run.py fast --write-artifacts
```

Pass if:

- command exits `0`
- generated report shows `status: passed`

### 3. Normal smoke

Run:

```bash
python scripts/smoke/run.py normal --write-artifacts
```

Pass if:

- command exits `0`
- generated report shows `status: passed`

### 4. Thorough smoke

Run:

```bash
python scripts/smoke/run.py thorough --write-artifacts
```

Pass if:

- command exits `0`
- generated report shows `status: passed`

Fail if:

- the failing row is `AD18-RUNTIME-ROOT-001` due to an ambient daemon; that
  means the host was not isolated enough for a valid singleton proof

### 5. Direct shared-host lane

Run:

```bash
python scripts/smoke/run_thorough_shared_host.py
```

Pass if:

- command exits `0`
- output status is `passed`
- output confirms one shared daemon handled two workspaces with one shared
  `ATM_HOME`
- no leaked daemon remains after the run

Fail if:

- any ambient `atm-daemon.exe` is already live
- the singleton owner pid differs between the two workspaces
- invalid stdin succeeds or mutates the daemon pid set
- inline/stdin/file durable bodies do not match expected values exactly

### 6. Real same-host graft host lane

Run:

```bash
python scripts/smoke/run_graft_same_host.py
```

Pass if:

- command exits `0`
- output status is `passed`
- the real `atm-graft` host registers against the shared daemon contract
- the graft host receives an advisory nudge
- the graft host completes unary `read`, `ack`, and `send`
- the CLI operator can read the graft host follow-up message
- no leaked daemon remains after the run

Fail if:

- graft activation does not reach the listening state
- no advisory nudge is delivered to the graft host
- `read`, `ack`, or `send` fails inside the graft host lane
- the operator cannot read the graft host follow-up message
- the lane requires a second daemon or leaks one after completion

## AF evidence this checklist must reproduce

- AF-1:
  `docs/plans/phase-af/af-1-host-singleton.md`
- AF-2:
  `docs/plans/phase-af/af-2-observability-release-gates.md`
- AF-3:
  `docs/plans/phase-af/af-3-native-send-input-integrity.md`

Specifically, the Windows run must reproduce:

- one-daemon singleton behavior on an isolated host
- no leaked daemon artifacts after the shared-host smoke
- retained observability/release-gate rows staying green
- native inline/stdin/file send-input integrity
- the real same-host `atm-graft` advisory plus unary host lane

## Windows verdict rules

- Only mark Windows green if `AF31-WIN-002` through `AF31-WIN-009` all pass.
- If `AF31-WIN-005` cannot run because of an ambient daemon, mark the lane
  `FAIL`, not `SKIP`.
- Do not waive singleton, send-input, or graft-host failures.

## Windows result payload minimum

The Windows JSON result should include at minimum:

```json
{
  "level": "smoke-1.3.1-windows",
  "platform": "Windows",
  "candidate_version": "1.3.1",
  "binary_sha": "<git sha>",
  "status": "passed|failed",
  "rows": [
    {
      "id": "AF31-WIN-009",
      "verdict": "PASS",
      "notes": "real same-host atm-graft host lane succeeded"
    }
  ]
}
```
