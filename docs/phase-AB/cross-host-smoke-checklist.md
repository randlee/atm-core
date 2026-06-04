# Phase AB Cross-Host Smoke Checklist

## Purpose

Frozen cross-host smoke matrix for Windows/macOS interoperability on disposable
state.

## Record Schema

Each row must record:

- `row_id`
- `lane`
- `sender_host`
- `receiver_host`
- `flow`
- `commands_or_entrypoints`
- `expected_result`
- `required_evidence`
- `status`
- `notes`

## Frozen Required Smoke Coverage

| Row ID | Lane | Sender Host | Receiver Host | Flow | Commands Or Entrypoints | Expected Result | Required Evidence | Status | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `AB-SMOKE-001` | `Lane A` | `Windows` | `Windows` | release-binary doctor on Windows clean-room state | `atm doctor --json` under disposable `ATM_HOME`/`ATM_CONFIG_HOME` | daemon auto-start succeeds and readiness is healthy or warning-only | command transcript; `doctor --json`; retained log snapshot when daemon-backed | `PENDING` | required by Required Smoke Coverage |
| `AB-SMOKE-002` | `Lane A` | `macOS` | `macOS` | release-binary doctor on macOS clean-room state | `atm doctor --json` under disposable `ATM_HOME`/`ATM_CONFIG_HOME` | daemon auto-start succeeds and readiness is healthy or warning-only | command transcript; `doctor --json`; retained log snapshot when daemon-backed | `PENDING` | required by Required Smoke Coverage |
| `AB-SMOKE-003` | `Lane A` | `Windows` | `macOS` | one-way durable cross-host send | Windows `atm send --json` to macOS recipient | durable send succeeds on disposable clean-room state | sender JSON result; receiver host transcript; retained logs from both hosts | `PENDING` | required by Required Smoke Coverage |
| `AB-SMOKE-004` | `Lane A` | `macOS` | `Windows` | one-way durable cross-host send | macOS `atm send --json` to Windows recipient | durable send succeeds on disposable clean-room state | sender JSON result; receiver host transcript; retained logs from both hosts | `PENDING` | required by Required Smoke Coverage |
| `AB-SMOKE-005` | `Lane A` | `Windows` | `macOS` | cross-host receiver read | receiver-side `atm read --all --json` after `AB-SMOKE-003` | receiver reads the just-delivered message successfully | receiver JSON result; command transcript; retained logs when daemon-backed | `PENDING` | required by Required Smoke Coverage |
| `AB-SMOKE-006` | `Lane A` | `macOS` | `Windows` | cross-host receiver read | receiver-side `atm read --all --json` after `AB-SMOKE-004` | receiver reads the just-delivered message successfully | receiver JSON result; command transcript; retained logs when daemon-backed | `PENDING` | required by Required Smoke Coverage |
| `AB-SMOKE-007` | `Lane A` | `macOS` | `Windows` | cross-host ack back to the original sender | receiver-side `atm ack ...` after a `--requires-ack` send | original sender sees the reply-state mutation across hosts | sender JSON result; receiver JSON result; retained logs from both hosts | `PENDING` | required by Required Smoke Coverage |
| `AB-SMOKE-008` | `Lane A` | `Windows` | `macOS` | degraded notification after durable cross-host send | successful cross-host send plus failing notification/hook path | durable send still succeeds and degradation is visible as evidence | sender JSON result; degraded warning/error evidence; retained logs from both hosts | `PENDING` | required by Required Smoke Coverage |
| `AB-SMOKE-009` | `Lane A` | `Windows/macOS` | `macOS/Windows` | retry-visible interruption and recovery | daemon restart / temporary peer unavailability during cross-host flow | retry/recovery remains observable without losing the smoke outcome classification | command transcript; retained logs from both hosts; recovery notes | `PENDING` | required by Required Smoke Coverage |
| `AB-SMOKE-010` | `Lane B` | `Windows/macOS` | `macOS/Windows` | copied-state revalidation after clean-room success | approved subset rerun on disposable copied ATM/Claude state | copied-state lane passes only after Lane A is already green | copied-state command transcript; sender/receiver JSON; retained logs from both hosts | `PENDING` | required by Required Smoke Coverage |

