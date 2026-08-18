# Hermes Graft Bridge Runbook

## Inputs and boundary

One launchd job runs one Hermes profile. The operator owns the profile
registry; each row supplies `profile`, `ATM_TEAM`, `ATM_IDENTITY`, optional
`ATM_CHAT_ID`, bridge configuration path, log path, rendered plist path, and
receiver socket path. Do not put personal profiles or paths in this repository.

Start with
[`templates/hermes-bridge-profiles.example.tsv`](templates/hermes-bridge-profiles.example.tsv)
and replace every example value in an operator-owned registry. The checked-in
template is
[`templates/ai.hermes.atm-graft-PROFILE.plist`](templates/ai.hermes.atm-graft-PROFILE.plist).
Replace each `@...@` token from one registry row. The three
`ProgramArguments` tokens are absolute executable paths with this exact
contract:

```text
@BRIDGE_GATE_COMMAND@ @HERMES_GATEWAY_READINESS_COMMAND@ @BRIDGE_COMMAND@
```

`@BRIDGE_GATE_COMMAND@` is a profile-owned wrapper. It must run
`@HERMES_GATEWAY_READINESS_COMMAND@`; only after that command exits zero may it
`exec` `@BRIDGE_COMMAND@`. A failed readiness command must leave the bridge
unstarted and return its nonzero status to launchd, which may retry the same
gate through `KeepAlive`. This is the required pre-activation readiness gate;
the active probe below verifies an already-started job and does not replace it.
`@BRIDGE_COMMAND@` is the profile-owned Hermes runner. It imports
`atm_graft_hermes_loader.HermesGraftRuntime` and is not a daemon command. The
runner passes its authenticated Hermes `session.steer` request callable and
registration/rebind-backed async `resolve_session_id(chat_id)` callable to
`HermesGraftRuntime.from_environment(...)`, then awaits `runtime.start()` and
calls `runtime.close()` during shutdown. That resolver returns the opaque live
runtime session ID for the configured platform chat. The adapter fails closed
with a typed error when the resolver is missing, fails, returns no session, or
returns the raw chat ID; only the resolved runtime ID is sent as `session_id`.
The runner binds the bridge to that non-interrupting steer hook, not ordinary
inbound-user-message ingress. A rejected/error response is logged as a visible
delivery failure; it must not fall back to a normal message or create a retry
queue.

The runner supplies `adapter.live_nudge_callback` as the bridge's live callback
and `adapter.recovery_summary_callback` as its AI.37 recovery hook. Both only
schedule the adapter's awaited steer delivery on the already-connected host
loop; neither callback selects a source-derived host session.

The bridge process is restartable by launchd. It does not start, stop, restart,
or own `atm-daemon`.

The bridge activates from its configured ATM team and identity; its workspace
does not need a `.atm.toml`. A present `.atm.toml` supplies optional ATM
defaults only and must not suppress the receiver. Any malformed configuration,
invalid workspace root, endpoint ownership conflict, or listener bind failure
is an explicit startup error rather than an inert successful session.

## Reconcile the graft workspace root

The bridge publishes its receiver record under the workspace root passed to
`PyGraftSessionOptions`, while the daemon resolves that location from the
recipient's durable roster metadata. After a profile is first registered, or
whenever its checkout/worktree moves, update the roster before restarting the
bridge:

```sh
ATM_TEAM=hermes ATM_IDENTITY=hendrix \
  atm teams update-member hermes skillrx \
  --workspace-root /path/to/skillrx/workspace
```

Run this repair for every profile whose live graft session uses a different
workspace root than its stored `home_dir`; repeat it after a worktree move,
profile migration, or launch-registry change. Verify the durable value with
`atm members --json` and confirm the member's `extra.workspace_root` matches the
path supplied to `PyGraftSessionOptions`. The daemon logs the selected root
source (`workspace_root` or the compatibility `home_dir` fallback) at debug
level, making stale metadata visible without guessing which branch resolved.

Automatic roster mutation during graft activation is intentionally not used:
the graft library does not own team-admin storage or operator authorization.
The explicit update keeps that boundary auditable and prevents a bridge from
silently changing durable team configuration.

## Install and status

For a rendered profile `example`:

```sh
plutil -lint ~/Library/LaunchAgents/ai.hermes.atm-graft-example.plist
launchctl bootstrap "gui/$UID" ~/Library/LaunchAgents/ai.hermes.atm-graft-example.plist
launchctl print "gui/$UID/ai.hermes.atm-graft-example"
```

Validate registry shape and its computed qualified ATM chat keys without
touching launchd:

```sh
just verify-hermes-bridge-deployment /operator/hermes-bridge-profiles.tsv
```

After the registry has real paths and AI.19 is `PASS`, run active probes:

```sh
just verify-hermes-bridge-deployment /operator/hermes-bridge-profiles.tsv --active
```

The active probe verifies every job is independently registered, every graft
receiver path is available, then performs a controlled `SIGTERM` and
`launchctl kickstart -k` restart for each profile. It must not run against the
example registry. Before bootstrap, verify the rendered plist has the ordered
gate/readiness/bridge `ProgramArguments` contract above; no operator may bypass
the gate by putting the bridge runner directly in the first argument slot.

## Restart, logs, and diagnosis

```sh
launchctl kickstart -k "gui/$UID/ai.hermes.atm-graft-example"
tail -n 200 /operator/logs/hermes-atm-example.log
launchctl print "gui/$UID/ai.hermes.atm-graft-example"
```

If registration fails, validate the rendered plist with `plutil -lint` and
verify its owner-readable bridge configuration and log directory. If the
receiver probe fails, inspect the profile runner and its graft configuration;
do not add a polling loop or start another daemon. A callback failure is
reported through the existing graft callback path, not retried by launchd. Run
the checked-in reference proof before diagnosing a downstream gateway:

```sh
python3 scripts/phase-ai/run-hermes-steer-smoke.py --fixture
```

It proves live and recovery wakes reach a configured session only after a safe
tool boundary without invoking a normal-message handler or mutating ATM mail.

## Add a profile

Add one registry row with a new safe profile name. Render one plist, validate
it, bootstrap it, then run the active probe for the complete registry. Confirm
the printed key is `atm:agent:chat-id@team` (or `atm:agent@team` without a
chat ID) and that it does not share Telegram or Discord state.

## Current validation limit

The operator profile registry has not been supplied and AI.19 is `FROZEN`, not
`PASS`. Therefore this sprint provides the template, runbook, and safe
registry/probe tooling only. It does not claim profile deployment evidence or
AI.20 `PASS`.
