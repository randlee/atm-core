---
title: AI.13 reusable peer-pair smoke contract
status: complete
branch: feature/pAI-s13-peer-smoke-contract
worktree: ../atm-core-worktrees/feature/pAI-s13-peer-smoke-contract
target: integrate/phase-AI
depends_on: AI.11, AI.12
---

# AI.13 — reusable peer-pair smoke contract

## Closure

The repository contains one documented, repeatable peer-pair smoke procedure
and runner that future releases execute unchanged on any two hosts. It proves
the daemon protocol, not raw TCP reachability.

## Deliverables

1. Add one repository-owned peer-smoke runner and companion document. It takes
   host-role inputs, daemon endpoint, certificate/trust setup, identities, and
   an evidence directory; it never embeds a developer machine address or secret.
2. Define the required ordered cases: preflight; A→B send/read/nudge; B→A
   send/read/nudge; A→B requires-ack then B→A ack; exact-ULID duplicate;
   unavailable peer; untrusted/allowlist rejection; and failed remote ack with
   no acknowledgement-state mutation.
3. Define machine-readable evidence records containing commit, daemon/client
   version, sender/recipient, transport, message ULID, command, result, and
   sanitized log window. Raw socket success is never a passing case.
4. Add release documentation requiring this runner for every release that
   changes daemon, HTTP, TLS, storage write, acknowledgement, or transport code.
5. The runner owns deterministic teardown. On success, failure, interruption,
   or timeout it stops only daemons it launched, waits for their recorded PIDs,
   verifies no listener remains, and removes only runtime metadata owned by
   those stopped PIDs. It records teardown success/failure in evidence and
   never deletes an owner lock, socket, or endpoint record belonging to an
   unrelated live singleton.

## Shared smoke rules

- The sender creates one ULID. Every retransmission and receiver persistence
  uses that exact ID and immutable payload.
- A duplicate is pass only when the receiver retains one record and emits no
  second nudge, acknowledgement mutation, or peer send.
- An unavailable peer or failed ack is pass only when the operation returns a
  typed error and no prohibited delivery/ack state is created.
- Each host must run a persistent daemon and pass its local send/read/ack smoke
  before peer-pair tests begin.
- Test failure is not complete until runner-owned daemons/listeners are gone or
  a typed cleanup failure with the retained PID/listener evidence is reported.

## Acceptance criteria

- The runner is parameterized for Mac↔Mac and Mac↔Windows without code changes.
- It exercises HTTP resource endpoints through CLI/graft-facing daemon clients,
  not private storage helpers.
- Its negative cases prove mTLS and allowlist rejection occur before routing.
- A release operator can execute the document from a clean checkout without
  inferring addresses, roles, or expected results.
- Injected runner failure leaves no runner-owned daemon, listener, socket, or
  endpoint record after deterministic cleanup.

## Required validation

Run the runner against the repository's local two-daemon fixture; validate
argument/error handling; run `just lint` and `just test`; review the generated
evidence schema with a fixture artifact.

## Non-closure

AI.13 creates and locally validates the reusable procedure. It does not claim
physical Mac↔Mac or Mac↔Windows success; AI.14 and AI.15 do that.
