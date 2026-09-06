---
phase: AY
sprint: AY.6
title: Coordinated Herdr endpoint restart
branch: feature/ay6-herdr-restart-coordination
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay6-herdr-restart-coordination
integration_branch: integrate/phase-ay
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: core
parallel_with: [AY.8]
stack_parent: feature/ay5-herdr-entry-control-plane
pr_target: feature/ay5-herdr-entry-control-plane
dependency_relations:
  - prerequisite: AY.5
    dependent: AY.6
    relation: must_follow
    rationale: restart coordination consumes AY.5's native-doctor ingestion, deterministic identifiers, owned entry status, JSON envelope, and fail-closed transaction state; the AY.6 PR is stacked directly on AY.5.
  - prerequisite: AY.6
    dependent: AY.7
    relation: must_follow
    rationale: AY.7 implements and CI-verifies the Windows CLI process and per-user entry/restart branches against the complete coordinated-restart contract delivered here.
  - prerequisite: AY.6
    dependent: AY.8
    relation: parallel_safe
    rationale: AY.6 edits daemon-switch restart logic/tests and operator governance/docs; AY.8 edits atm-herdr socket transport, protocol fixtures, boundary revision, and architecture exemption after shared AY.3 contracts merge.
---

# AY.6 — Coordinated Herdr endpoint restart

Add one explicit operator command that restarts one configured Herdr endpoint,
preserving panes through live handoff when the running server advertises that
capability and otherwise requiring informed acknowledgement before a stop. The
command never updates Herdr, never restarts ATM, and verifies success with a
fresh native doctor read. Ordinary ATM restart fails closed while any endpoint
is still protocol-mismatched.

## Delivery topology and `/gh-stack`

AY.6 is stacked directly above AY.5 and below Windows completion AY.7:

```text
integrate/phase-ay <- AY.2 <- AY.3 <- AY.4 <- AY.5 <- AY.6 <- AY.7
```

Use the `/gh-stack` skill noninteractively:

```bash
git config rerere.enabled true
git config remote.pushDefault origin
gh stack link --base integrate/phase-ay \
  feature/ay2-herdr-transport-seam \
  feature/ay3-herdr-endpoint-doctor-config \
  feature/ay4-herdr-breaker-lifecycle \
  feature/ay5-herdr-entry-control-plane \
  feature/ay6-herdr-restart-coordination
gh pr view feature/ay6-herdr-restart-coordination \
  --json headRefName,baseRefName,state
```

Append AY.7 with `gh stack link <stack-number> <branch>`. Phase AY forbids
`gh stack rebase`, `gh stack sync`, and `gh stack merge`; `link` creates no
local tracking, so verify the base with `gh pr view --json`. Use merge
commits, no force-push, and parent-first PR completion. AY.5 development pushed
triggers merge-forward into AY.6 before every development/fix round.

AY.8 remains an independent branch from `integrate/phase-ay`, parallel-safe
with AY.6. Neither branch merges an unmerged sibling.

## Preconditions

- P-A and P-B from the Phase AY plan are satisfied.
- AY.5 development is pushed, and AY.6 is created from
  `feature/ay5-herdr-entry-control-plane`.
- AY.5's entry transaction, status projection, identifier helper, doctor
  ingestion, and JSON/exit contract are green on the parent branch.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or shape-only
completion fails the sprint.

- [ ] D1 — Extend REQ-P-DAEMON-SWITCH-002 and the ADR-053 addendum with the C1
  restart-only contract, one-endpoint selection, live-handoff eligibility,
  destructive-stop acknowledgement, entry-owned relaunch, verification,
  ordinary ATM restart preflight, JSON/exit semantics, and complete error
  inventory.
- [ ] D2 — Add operator-invoked `--restart-herdr [<endpoint>]` to
  `.claude/skills/daemon-switch/scripts/daemon-switch.py`. Resolve the selector
  only from AY.3 doctor endpoints and AY.5 entry status. An omitted endpoint is
  accepted only when exactly one is configured; `socket_path` provenance and
  unowned/incomplete entries fail closed.
- [ ] D3 — Implement C2: use scoped live handoff only when the installed client
  is newer than the selected running server and
  `capabilities.live_handoff == true`; otherwise require
  `--stop-herdr-panes`, stop the selected endpoint, and relaunch through AY.5's
  owned start-at-login entry. Never run `herdr update`.
- [ ] D4 — Re-read native `atm doctor --json` after the operation and exit zero
  only when the selected endpoint is `ok`. `--restart-herdr` never restarts ATM.
  Add a preflight to the existing ATM daemon restart path that refuses while
  any configured endpoint is `client_server_mismatch`.
- [ ] D5 — Add complete selector/handoff/stop/relaunch/verification/preflight
  fixtures, exact JSON snapshots, grep gates, and operator documentation in the
  daemon-switch skill. All platform argv and identifiers are asserted; AY.7
  owns Windows execution in the Windows CI lane.

### Paths to delete

None.

## Required work and exact targets

| Ownership | Exact targets |
| --- | --- |
| Governance | `docs/requirements.md`, `docs/adr/ADR-053-typed-temporary-daemon-service-launch-overlay.md` |
| Restart/preflight | `.claude/skills/daemon-switch/scripts/daemon-switch.py` using AY.5's platform helpers |
| Tests | `.claude/skills/daemon-switch/tests/test_daemon_switch.py` and its platform/doctor fixtures |
| Operator documentation | `.claude/skills/daemon-switch/SKILL.md` |

This is shell/Python-level sequencing of bounded existing Herdr commands. It
adds no daemon-side state, startup hook, supervisor, polling loop, or process
ownership. Do not modify `atm-http-runtime`, the frozen synchronous daemon, or
Herdr transport selection.

## Command and behavior contracts

### C1 — Explicit command

```text
daemon-switch ... --restart-herdr [<default-or-session>] [--stop-herdr-panes]
```

One invocation selects one endpoint. With no endpoint value, exactly one
configured endpoint must exist. The command is rejected when Herdr is not
configured. It never runs as part of ordinary switch/restore/restart and never
runs implicitly.

### C2 — Selection and restart algorithm

The operation reads doctor once for selection, resolves AY.5's single
`herdr_entry_identifier(platform, endpoint)`, and follows this exact branch:

```text
if selector is absent and endpoint count != 1:
    refuse HERDR_RESTART_ENDPOINT_REQUIRED
if selector does not match:
    refuse HERDR_RESTART_ENDPOINT_UNKNOWN
if endpoint.provenance == socket_path:
    refuse HERDR_RESTART_SOCKET_PATH
if entry is unowned, missing, or has an active journal:
    refuse with AY.5 entry status code

if installed_client > endpoint.running_server
   and endpoint.capabilities.live_handoff == true:
    run scoped `server live-handoff`
else:
    require --stop-herdr-panes
    run scoped `server stop`
    start the selected owned entry

reread doctor
require selected endpoint.state.kind == ok
```

Bounded completion policy (AYP-R13-008): the whole `--restart-herdr` run
has one overall deadline (default 120 s, `--restart-timeout-secs`); every
Herdr shell-out (`server live-handoff`, `server stop`, entry start) has a
per-command timeout (default 30 s) after which the child is killed and
reaped and the run fails with `HERDR_RESTART_TIMEOUT` naming the command;
the post-relaunch doctor verification retries with bounded backoff (at most
5 reads, 2 s, 4 s, 8 s, 8 s, 8 s, all within the overall deadline) and
fails with `HERDR_RESTART_VERIFY_TIMEOUT` listing the last observed
endpoint state. Both codes use the AY.5 exit class 4 and the C4 envelope.
Fixtures: a hung command fake (never exits) proves the per-command bound;
a slow-but-successful readiness fake (endpoint ok on the third read) proves
retry without a false failure; both use injected time, no real sleeps.

Every command is scoped: default uses `herdr server ...`; a session uses
`herdr --session <name> server ...`. `live_handoff: null` means unknown and
takes the stop branch. A handoff failure stops immediately because Herdr owns
its rollback. Stop/relaunch warns that panes exit and may only proceed with the
explicit acknowledgement.

### C3 — ATM restart preflight

Before the existing daemon-switch ATM restart changes process state, it reads
native doctor and checks every configured endpoint. Any
`client_server_mismatch` yields `HERDR_RESTART_ENDPOINTS_PENDING` and the list
of affected endpoints. The operator restarts those endpoints one at a time,
then reruns the ordinary ATM restart. Other endpoint failures remain legible in
doctor but do not silently trigger Herdr lifecycle actions.

### C4 — JSON and exit contract

Reuse AY.5's one-object stdout envelope and 0/3/4 exit classes:

```json
{
  "ok": false,
  "code": "HERDR_RESTART_ENDPOINT_REQUIRED",
  "message": "more than one Herdr endpoint is configured",
  "remedy": "rerun with --restart-herdr <default-or-session>",
  "entries": [
    { "endpoint": "default", "identifier": "com.randlee.atm.herdr-server" },
    { "endpoint": "work", "identifier": "com.randlee.atm.herdr-server.work" }
  ]
}
```

Herdr stderr is quoted in `message` for `HERDR_RESTART_HERDR_FAILED` without
changing its stable code or exit class. Human warnings go to stderr.

## Error inventory

| Code | Exit | Cause | Required recovery |
| --- | --- | --- | --- |
| `HERDR_NOT_CONFIGURED` | 3 | doctor reports configured false | Configure Herdr or omit restart request |
| `HERDR_DOCTOR_UNREADABLE` | 4 | selection/verification doctor data failed | Fix native doctor and rerun; never guess |
| `HERDR_RESTART_ENDPOINT_REQUIRED` | 3 | several endpoints and selector omitted | Select one returned endpoint |
| `HERDR_RESTART_ENDPOINT_UNKNOWN` | 3 | selector matches none | Use a returned default/session name |
| `HERDR_RESTART_SOCKET_PATH` | 3 | selected endpoint is externally owned | Restart through the external owner |
| `HERDR_RESTART_PANES_ACK_REQUIRED` | 3 | ordinary stop would terminate panes without acknowledgement | Accept impact and pass `--stop-herdr-panes` |
| `HERDR_RESTART_NO_LIVE_HANDOFF` | 3 | mismatched/newer-client endpoint lacks a true handoff capability and stop is unacknowledged | Pass `--stop-herdr-panes` or enable compatible handoff |
| `HERDR_RESTART_HERDR_FAILED` | 4 | scoped stop, handoff, relaunch, or verification failed | Follow quoted Herdr/platform detail and retry |
| `HERDR_RESTART_ENDPOINTS_PENDING` | 3 | ATM restart sees protocol-mismatched endpoints | Restart every listed endpoint first |

AY.5 entry codes remain valid when entry ownership/journal inspection fails and
are not remapped to generic restart codes.

## Acceptance criteria

- [ ] A1 — Requirements and ADR map every C1–C4 rule and error code to an exact
  fixture; req-qa finds no implicit update/start/restart path.
- [ ] A2 — Selector fixtures cover not-configured, one endpoint without value,
  several without value, explicit default/session, unknown, socket-path, mixed
  socket-path plus sessions, foreign/missing entry, and active journal.
- [ ] A3 — Handoff fixtures cover mismatch/newer-client with capability true,
  false, and null. Only true selects live handoff without acknowledgement;
  false/null require the documented stop authorization.
- [ ] A4 — Stop fixtures assert the warning and acknowledgement, scoped argv,
  AY.5 identifier result, owned-entry relaunch, and fresh doctor verification
  for default and named endpoints on macOS, Linux, and Windows fakes.
- [ ] A5 — `--restart-herdr` never restarts ATM; ordinary ATM restart refuses
  with all pending mismatched endpoint names and succeeds after they are `ok`.
- [ ] A6 — Every error row has exact stdout JSON and exit assertions; source
  grep finds no `herdr update` string in daemon-switch production sources.
- [ ] A7 — Architecture/source review finds no daemon-runtime Herdr lifecycle
  logic and no frozen synchronous-daemon change.
- [ ] A8 — Merge gate is 0 blocking, 0 important, and 0 minor in scope;
  quality-mgr posts PASS and CI is green at merge time.

## Required validation

This is the authoritative validation list.

- [ ] V1 — `python3 .claude/skills/daemon-switch/tests/test_daemon_switch.py`
  exits zero with every selector, handoff, stop, verification, preflight, JSON,
  argv, and platform fixture enabled.
- [ ] V2 — `rg -n "herdr update" .claude/skills/daemon-switch/scripts` returns
  no production invocation.
- [ ] V3 — `just lint spell` and `just lint adr-index` exit zero.
- [ ] V4 — `just validate` exits zero.
- [ ] V5 — `gh pr view feature/ay6-herdr-restart-coordination --json
  headRefName,baseRefName,state` reports base
  `feature/ay5-herdr-entry-control-plane`; AY.8 is absent from the linear stack.

## Non-closure and out of scope

- Herdr binary installation and upgrade remain operator-owned Herdr actions.
- Windows restart verification in the Windows CI lane is AY.7; socket
  transport/cutover are AY.8–AY.9.
- This sprint does not select, add, or remove a Herdr transport.
- No legacy synchronous-daemon runtime or dispatch work is permitted.
