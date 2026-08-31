---
phase: AV
sprint: AV.1b
status: ready_for_operator
---

# AV.1b live proof: read remains live under writer-lane stall

This is the operator-run acceptance proof for the AV.1b cutover. It takes
about two minutes after the matched daemon is already running. The runner does
not start, stop, switch, configure, or otherwise manage a daemon: that is an
operator action under ADR-053.

## Preconditions

1. Use **only** an isolated approved target: `m5-atmbench`, or a local
   disposable daemon profile. Do not use a personal/shared ATM home.
2. Build and install the matched `atm` and `atm-daemon` pair from
   `feature/av1b-read-handler-cutover`, using the operator's normal
   `/daemon-switch` workflow. The maintained Tokio+Axum daemon
   (`atm-http-runtime`) is the sole target; do not use or alter the frozen
   legacy synchronous daemon.
3. Seed one unread message for `proof-agent@proof-team` in the isolated
   account, and record its ID.
4. Confirm the public paired-release readiness contract:

   ```sh
   ATM_IDENTITY=proof-agent ATM_TEAM=proof-team atm doctor --team proof-team --json
   ```

   The response must be `summary.status=healthy`,
   `runtime_status.readiness=ready`, and show the same `1.4.6` version in
   `client_context.version` and `daemon_context.version`. The proof runner
   repeats this check and refuses to invoke `atm read` if it does not match.

## Deterministically hold the writer lane

The precise post-read writer operation is
`WriteOp::ApplyReadDisplayState`: it marks the selected message read and
updates the seen watermark after the response has become eligible. Hold the
**isolated target's** SQLite writer with an exclusive transaction before
running the proof. This blocks that operation while leaving the AV.1b
read-only mailbox reader connections eligible to serve the read.

In a separate terminal on the isolated target, open the daemon's isolated
`mail.db` and keep this transaction open until the proof finishes:

```sql
BEGIN EXCLUSIVE;
```

For example, if the disposable account's database is
`$ATM_HOME/.atm/db/mail.db`:

```sh
sqlite3 "$ATM_HOME/.atm/db/mail.db"
sqlite> BEGIN EXCLUSIVE;
```

Do not run this against `randlee` or any shared account. When the proof is
complete, issue `ROLLBACK;` and exit the `sqlite3` shell. This is deliberate
test pressure only; it is not a daemon control path and the runner never
touches the database itself.

## Run the proof

From this checkout, while the lock is held:

```sh
python3 scripts/smoke/av1b_read_under_stall.py \
  --team proof-team \
  --actor proof-agent \
  --message-id <seeded-unread-message-id> \
  --evidence-out docs/plans/phase-av/evidence/av1b-read-under-stall.json
```

The live external budget is **3000 ms**. `SERVER_REQUEST_BUDGET` is the
maintained daemon's request budget; although `[reader_lanes]` has a 10-second
storage wait cap, it cannot extend the caller's 3-second request deadline.
`PASS` requires `atm read` to exit successfully within 3000 ms while the
writer lock remains held.

The script writes one JSON record for both PASS and FAIL, containing UTC
timestamps, the expected release, pre/post doctor snapshots, command outcome
and measured latency, and a retained `atm log snapshot` excerpt starting at
the proof timestamp. A non-matching or non-ready daemon fails before any
`atm read` invocation. The output JSON is the evidence to retain with the
AV.1b review; do not replace a failed result with a prior result.
