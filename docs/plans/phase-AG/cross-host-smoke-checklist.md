# Phase AG Cross-Host Validation Checklist

## Purpose

Frozen validation matrix for Windows/macOS cross-host interfaces on the `1.3.1`
release line.

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
- `linked_finding`
- `notes`

## Frozen Required Validation Coverage

| Row ID | Lane | Sender Host | Receiver Host | Flow | Commands Or Entrypoints | Expected Result | Required Evidence | Status | Linked Finding | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `AG-VAL-001` | `Lane A` | `Windows` | `Windows` | release-binary doctor on Windows clean-room state | `atm doctor --json` | daemon auto-start succeeds and readiness is healthy or warning-only | command transcript; `doctor --json`; retained log snapshot when daemon-backed | `PASS` | `AG-FIND-002` | Windows host-env baseline accepted for AG.1 on a computer with no installed/running ATM; strict no-live-OS-account-state clean-room remains excluded from this PASS. |
| `AG-VAL-002` | `Lane A` | `macOS` | `macOS` | release-binary doctor on macOS clean-room state | `atm doctor --json` | daemon auto-start succeeds and readiness is healthy or warning-only | command transcript; `doctor --json`; retained log snapshot when daemon-backed | `FAIL` | `AG-FIND-003` | Host-singleton doctor health is green on the active 1.3.1 daemon, but the documented clean-room env contract does not isolate the daemon/runtime surface on one host. |
| `AG-VAL-003A` | `Lane A` | `Windows` | `macOS` | unauthorized-host rejection before mailbox mutation | Windows `atm send --json` from a host not present in the enabled allowlist | remote daemon rejects the peer before mailbox mutation and emits the structured rejection record | sender transcript/JSON; receiver rejection log entry with host/socket/reason; proof no mailbox mutation occurred | `PENDING` | `AG-FIND-004` | required before normal authorized cross-host success rows |
| `AG-VAL-003` | `Lane A` | `Windows` | `macOS` | first live cross-host durable send | Windows `atm send --json` to macOS recipient | daemon-to-daemon channel is live and durable send succeeds | sender JSON result; receiver transcript; retained logs from both hosts | `BLOCKED` | `AG-FIND-004` | Blocked in released `1.3.1`: production code exposes outbound peer dialing only and no production inbound TCP peer-listener lane for the remote daemon to accept. |
| `AG-VAL-004` | `Lane A` | `macOS` | `Windows` | first reverse-direction durable send | macOS `atm send --json` to Windows recipient | reverse-direction durable send succeeds | sender JSON result; receiver transcript; retained logs from both hosts | `FAIL` | `AG-FIND-005` | Live attempt `01KXK8PV31QMXR3FPJRXRQNSJZ` returned success from the macOS CLI, but code and retained evidence show local-only sink delivery into `~/.atm/daemon/non_claude_outbound.jsonl` rather than a peer-transport handoff to the Windows daemon. |
| `AG-VAL-005` | `Lane A` | `Windows` | `macOS` | receiver-side read after `AG-VAL-003` | `atm read --all --json` on macOS | receiver reads the just-delivered message | receiver JSON result; transcript; retained logs when daemon-backed | `PENDING` | `—` | validate read path |
| `AG-VAL-006` | `Lane A` | `macOS` | `Windows` | receiver-side read after `AG-VAL-004` | `atm read --all --json` on Windows | receiver reads the just-delivered message | receiver JSON result; transcript; retained logs when daemon-backed | `PENDING` | `—` | validate reverse read path |
| `AG-VAL-007` | `Lane A` | `macOS` | `Windows` | cross-host ack round-trip | `atm ack ...` after a `--requires-ack` send | original sender sees the reply-state mutation | sender JSON result; receiver JSON result; retained logs from both hosts | `PENDING` | `—` | validate ack mutation |
| `AG-VAL-008` | `Lane A` | `Windows` | `macOS` | degraded notification after durable cross-host send | successful send plus failing notification/hook path | durable send succeeds and degradation is visible | sender JSON result; degraded warning/error evidence; retained logs from both hosts | `PENDING` | `—` | notification degradation must not be misclassified |
| `AG-VAL-009` | `Lane A` | `Windows` | `macOS` | retry-visible interruption and recovery | daemon restart or temporary peer unavailability during Windows -> macOS cross-host flow | retry/recovery remains observable without losing result classification | transcript; retained logs from both hosts; recovery notes | `PENDING` | `—` | bounded recovery proof for the Windows -> macOS direction only |
| `AG-VAL-010` | `Lane B` | `Windows/macOS` | `macOS/Windows` | copied-state revalidation | approved subset rerun on disposable copied state | copied-state lane passes only after Lane A is green | copied-state transcript; sender/receiver JSON; retained logs from both hosts | `PENDING` | `—` | not allowed before Lane A success |
| `AG-VAL-011` | `Lane A` | `Windows/macOS` | `Windows/macOS` | transport-security requirement disposition | inspect retained evidence for cross-host transport against documented TCP/TLS requirement and record whether TLS is actually present | AG either captures affirmative TLS evidence or links a named `PRODUCT-BUG` / requirement-drift finding; any release-usable verdict must then explicitly exclude transport-security coverage when TLS is absent | requirement/architecture citation; implementation citation; finding link if TLS is absent; final verdict linkage | `PASS` | `AG-FIND-001` | Disposition captured: current `1.3.1` cross-host path remains plain TCP, so transport-security coverage is explicitly excluded while `AG-FIND-001` stays open. |
| `AG-VAL-012` | `Lane A` | `Windows` | `Windows` | secure loopback handshake | secure loopback send/receive through the local daemon peer listener | secured transport handshake succeeds locally and ATM payload delivery still works | command transcript; sender/receiver JSON; secure-handshake log evidence | `PENDING` | `—` | secure local proof before second-host reruns |
| `AG-VAL-013` | `Lane A` | `Windows` | `macOS` | secure LAN host-pair rerun | authorized secure daemon-to-daemon send/read/ack on the simplest real network path | secure daemon-to-daemon transport succeeds across the LAN host pair and the AG.7 functional rows still pass | sender/receiver JSON; handshake evidence; retained logs from both hosts | `PENDING` | `AG-FIND-001` | secure rerun of the primary host-pair matrix |
| `AG-VAL-014` | `Lane A` | `Windows` | `macOS` | unauthorized or unauthenticated peer rejected under secured transport | secure daemon-to-daemon attempt with wrong/missing trust material | remote daemon rejects the peer before mailbox mutation under the secured transport path | sender transcript/JSON; receiver rejection log entry; proof no mailbox mutation occurred | `PENDING` | `AG-FIND-001` | secure-mode rejection proof |
| `AG-VAL-015` | `Lane A` | `Windows` | `macOS` | secure routed/VPN host-pair rerun | authorized secure daemon-to-daemon send/read/ack across the routed/VPN path | secure daemon-to-daemon transport succeeds across the routed/VPN host pair and preserves the AG.7 functional guarantees | sender/receiver JSON; handshake evidence; retained logs from both hosts | `PENDING` | `AG-FIND-001` | secure rerun of the secondary routed/VPN path |
