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
`@BRIDGE_COMMAND@` is the profile-owned Hermes runner that imports
`atm_graft_hermes_bridge`; it is not a daemon command. The runner uses its
ordinary inbound-user-message hook.

The bridge process is restartable by launchd. It does not start, stop, restart,
or own `atm-daemon`.

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
reported through the existing graft callback path, not retried by launchd.

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
