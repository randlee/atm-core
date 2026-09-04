---
status: complete
---

# AL.13 — M5↔M4 End-to-End Cross-Host Proof

**branch:** `feature/al-13-smoke`
**worktree:** `../atm-core-worktrees/feature/al-13-smoke`
**base:** a single SHA frozen from `origin/integrate/phase-al` at run start
**owners:** M5 operator (execution and evidence), M4 operator (matched pair and reciprocal proof)
**peer:** M4 operator (`rand-m4.local`, supplied through the durable peer store)
**unblocks:** AL.15 only after every acceptance row passes

## Completion record (2026-08-10)

AL.13 is complete. The retained M5↔M4 candidate evidence covers the
authenticated preflight, ordinary delivery, acknowledgement, and the M5
benchmark gate. The master report index links the cross-host artifacts:

- G4 preflight: `site/reports/smoke/macos/rand-m5.local/20260808T191457421442Z-pid23503-peer-preflight/`;
- G5 delivery: `site/reports/smoke/macos/rand-m5.local/20260808T191605199584Z-pid24045-crosshost-send/`;
- G6 acknowledgement: `site/reports/smoke/macos/rand-m5.local/20260808T203610076893Z-pid39023-crosshost-ack/`.

Earlier Aug. 1 benchmark panels remain historical baseline data, not an open
AL.13 failure. Later M5 benchmark execution completed the performance gate;
the remaining Phase AL cross-host gap is Windows↔M4, not this M5↔M4 lane.

## Outcome

This sprint is complete only when it has retained evidence that the current
Tokio/Axum `atm-http-runtime` actually transports messages between M5 and M4
in both directions. A green local test, a successful one-way nudge, an SSH
connection, or a status report that says “blocked” is not completion.

The existing `/smoke-test` skill and `just smoke` entry point remain the sole
smoke runner. SSH is permitted only for the runner to inspect/operate the
already-managed M4 test peer; it is never accepted as proof of ATM transport.
The direct `atm send`, `atm read`, and `atm ack` calls made by the cross-host
rows are the proof.

Do not commit an IP address, start a second daemon, manually unlink an ATM
socket, or patch the frozen legacy `atm-daemon` crate. Use `/daemon-switch` to
operate the one managed replacement runtime.

## Closure gates and dependency order

| Gate | Owner | Required evidence | Unblocks |
|---|---|---|---|
| G0 — candidate freeze | M4 + M5 | One source SHA, one Cargo version, and matching release CLI/daemon pairs selected on both hosts. | G1 |
| G1 — managed runtime readiness | M4 + M5 | Both doctors report client and daemon context at that exact version, with `runtime_status.liveness=running` and `readiness=ready`. | G2 |
| G2 — reciprocal peer reachability | M4 + M5 | Each host reaches the other's configured peer listener; M5's 43101 listener is live, not merely configured in SQLite. | G3 |
| G3 — local regression guard | M5 | `localhost` and `local-ip` each pass all retained repetitions. | G4 |
| G4 — authenticated preflight | M5 | `peer-preflight rand-m4.local` passes all repetitions with matching M4 version. | G5 |
| G5 — ordinary delivery | M5 | `crosshost-send rand-m4.local` proves exact-body readback M5→M4 and M4→M5. | G6 |
| G6 — acknowledgement | M5 | `crosshost-ack rand-m4.local` proves both requires-ack/reply directions and acknowledgement IDs. | G7 |
| G7 — performance and publication | M5 | Benchmark/report pass; all smoke and benchmark artifacts are indexed; PR/CI/QA are merge-ready. | AL.15 |

No gate may be skipped. A failed gate is an active defect or environmental
blocker, not a passing result; fix it in the responsible home branch when safe,
then rerun from that gate. An unapproved UI-blocked attempt is excluded from
the candidate evidence rather than counted as a runtime failure or a pass. The
final retained run must be noninteractive and free of that interference.

## G0 — Freeze one executable candidate

1. M4 records `CANDIDATE_SHA=$(git rev-parse origin/integrate/phase-al)` and
   the Cargo version at that SHA in the run report.
2. M5 fetches that SHA and merges it into `feature/al-13-smoke`. Its evidence
   commits may add reports only; `git diff --exit-code "$CANDIDATE_SHA" --
   Cargo.toml Cargo.lock crates/` must show no product-code divergence.
3. Each host builds and signs the same release pair from its candidate source:

   ```sh
   cargo build --release -p agent-team-mail -p atm-daemon
   python3 .just/sign_daemon_dev.py  # Apple Development signing hook
   ```

4. Each host records the selected CLI and daemon versions. They must be equal
   to each other and to the candidate Cargo version. A version-only branch
   suffix difference is still a mismatch and blocks G1.

Why: the previous M5 evidence used `1.4.1-beta-ai-1` while M4 used
`1.4.1-beta-ai-15`. The peer preflight correctly rejected that pairing before
it could prove a transport transaction.

## G1 — Select and prove the one managed runtime

On each macOS host, discover the real LaunchAgent label and plist rather than
inventing one:

```sh
launchctl list | rg 'com\.atm\.daemon'
find ~/Library/LaunchAgents -maxdepth 1 -name 'com.atm.daemon*.plist' -print
```

Then use the existing selector pair explicitly (this also works when
`/opt/homebrew/bin` is not on the LaunchAgent's PATH):

```sh
python3 .claude/skills/daemon-switch/scripts/daemon-switch.py switch \
  --cli-link /opt/homebrew/bin/atm \
  --daemon-link /opt/homebrew/bin/atm-daemon \
  --cli target/release/atm --daemon target/release/atm-daemon --yes \
  --service <actual-label> --launch-agent-plist <actual-plist>
```

If the pair is already selected but unhealthy, use the skill's explicit
`restart --yes` path. If the managed service has been stopped and an unowned
stale UDS remains, use `--repair-orphan`; never delete the socket by hand.

`atm doctor --json` is accepted only if all of these are true:

- `client_context.version` and `daemon_context.version` are present and equal
  to the frozen candidate version;
- `runtime_status.liveness` is `running`;
- `runtime_status.readiness` is `ready`.

The prior M5 state—an overall “healthy” result with no `daemon_context`, an
exit-64 LaunchAgent, and no process bound to 43101—fails G1.

## G2 — Prove that both peer listeners are reachable

Each host retains the configured peer-interface result:

```sh
atm peer interface list --json
```

M5 must show its selected replacement daemon listening on its enabled
interface and port, not just a durable configuration row:

```sh
lsof -nP -iTCP:43101 -sTCP:LISTEN
```

M4 and M5 then perform a bounded TCP connection to the other's configured
durable hostname and port. The peer hostname/IP remains operator data in the
peer store and report; it is never committed. A refused connection is a G2
failure. This gate exists because M5→M4 delivery alone proves only M4's
listener; it says nothing about M4→M5 delivery.

Once both directions are reachable, M4 sends the M5 operator the run-ready
confirmation through ATM. The M5 operator owns all subsequent smoke commands
and report publication; Rand does not relay individual commands.

## G3–G6 — Run the complete message proof

Set the already-created M5 smoke mailbox and M4 CLI path only as runtime
environment variables; do not write either into repository configuration:

```sh
export ATM_SMOKE_REMOTE_IDENTITY=m5-test
export ATM_SMOKE_REMOTE_TEAM=atm-dev
export ATM_SMOKE_REMOTE_ATM=/opt/homebrew/bin/atm
export ATM_SMOKE_REPETITIONS=10
# Optional for an account-local loopback fixture; defaults to 43101.
# export ATM_SMOKE_DIRECT_PEER_PORT=43111
```

Run, in this exact order, from the M5 AL.13 home worktree:

```sh
just smoke localhost
just smoke local-ip
just smoke peer-preflight rand-m4.local
just smoke crosshost-send rand-m4.local
just smoke crosshost-ack rand-m4.local
```

For every one of the ten attempts, the retained report must show:

- selected pair doctor/version readiness;
- M5 local-IP and loopback send/read/ack guard rows;
- peer preflight's authenticated M4 doctor at the same version;
- exact message-ID and body proof for ordinary delivery in each direction;
- exact message-ID and `acknowledgesMessageId` proof for requires-ack/reply in
  each direction.

The M4 report/log view must independently contain the counterpart message IDs
for G5 and G6. This is a second source of evidence, not an SSH-success proxy.

## G7 — Benchmark, artifacts, and merge gate

After G6 passes, run:

```sh
just benchmark
just benchmark-report
```

The benchmark runs its isolated pair/database and must not borrow the live
smoke daemon. Retain its output through the standard reports navigation.

The AL.13 PR may be marked ready only when all of the following are true:

1. Every G3–G6 smoke artifact has `status: PASS`; none is a one-attempt or
   partial substitute.
2. `site/reports/index.html` links every M5 smoke directory and the benchmark
   report.
3. The status report lists the frozen SHA/version, both host platform labels,
   all report links, and the correlated message IDs for both directions.
4. The branch is merged forward from current `origin/integrate/phase-al`, all
   CI checks are green, and quality review passes.
5. No report describes a blocked, skipped, or mismatched cross-host row as
   successful.

Until those conditions are met, the PR remains evidence of an active failure,
not merge-ready AL.13 completion.
