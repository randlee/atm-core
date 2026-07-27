---
title: AI.33 admission capacity and smoke evidence
status: proposed
branch: feature/pAI-s33-admission-capacity-smoke
target: integrate/phase-AI
depends_on: AI.31, AI.32
---

# AI.33 — admission capacity and smoke evidence

## Closure

Release-built ATM proves ten consecutive one-second intervals of at least
1,000 local host-qualified admissions per second on a disposable SQLite
database. The smoke runner also produces a compact, endpoint-explicit report
for ten repetitions of each local and available cross-host check.

## Deliverables

1. First commit sets every releasable assembly to `1.4.0-beta-ai.33` and
   records matching branch CLI/daemon values with `atm doctor --json`.
2. Add `scripts/smoke/run_admission_capacity.py` plus unit tests. It creates a
   temporary `ATM_HOME`, starts exactly one release-built branch daemon for
   that directory, and deletes the directory only after evidence collection.
   It must reject an unset/unsafe home path and never target `~/.atm` or a
   team/shared database.
3. Drive the public daemon client/API, not a direct dispatcher or mock. For ten
   consecutive one-second intervals, submit and receive 1,000 host-qualified
   `send` admissions. Run the same proof once with an accepting controlled peer
   and once with an unavailable configured peer. Persist JSON evidence with
   accepted count, response count, latency summary, failures, daemon PID,
   `ATM_HOME`, and release/doctor data; report PASS only when every interval
   meets the requirement.
4. Extend the keeper smoke runner from PR #675,
   `scripts/smoke/run_feature_smoke.py`, rather than creating another shell
   runner. Default each positive ladder row to ten consecutive attempts. Its
   generated combined HTML report has:
   - one card per participating computer;
   - a compact endpoint table for hostname, IP used, CLI version, daemon
     version, and doctor PASS/FAIL for each endpoint;
   - rows naming origin, destination, protocol/route, operation, and `n/10`
     result; and
   - a short failure summary plus bounded session logs.
5. The ladder is explicit:
   - one-computer: doctor, physical-interface self-IP send/read, `127.0.0.1`
     send/read, and their required-ack round trips;
   - two-computer: direct TLS curl doctor each direction, DNS TLS curl doctor
     each direction, ordinary ATM send/read each direction, then required-ack
     round trips each direction; and
   - recovery: only after both ordinary cross-host levels pass, make the peer
     unavailable, admit messages locally, restore it, and prove original ULID
     delivery without false remote-success claims.
6. Curl rows are transport diagnostics, not ATM delivery proof. ATM rows must
   verify the same ULID in the receiver inbox and, for ack-required, verify
   message delivery/read plus the reply acknowledgement's successful delivery.

## Required tests

- Unit tests reject production/shared `ATM_HOME`, retain all ten attempts in
  JSON, and render a FAIL row with the first failing attempt rather than hiding
  it behind a summary.
- A controlled local daemon integration test proves the capacity harness uses
  the public client and isolated SQLite database.
- Renderer tests verify one card per endpoint and show both endpoints' IP,
  version, and doctor state on a cross-host report.
- A test proves a received acknowledgement is correlated by ULID, not arrival
  order or a constant fixture value.

## Acceptance criteria

- Ten of ten one-second intervals each return at least 1,000 local admissions
  and responses using a disposable database.
- Every smoke row executes ten times by default; any failed attempt makes that
  row FAIL and preserves evidence.
- Cross-host report makes origin/destination and both endpoint identities
  obvious without reading source or raw logs.
- `just lint`, `just test`, `just smoke localhost`, and the isolated capacity
  command pass on the branch daemon. Cross-host rows are run only when peers
  are available; unavailable infrastructure is reported NOT-RUN, never PASS.

## Non-goals

This is not a production load benchmark, a multi-machine stress suite, or a
replacement for CI. It is a deterministic release gate for the exact local
admission and peer-delivery contracts introduced by AI.31–AI.32.
