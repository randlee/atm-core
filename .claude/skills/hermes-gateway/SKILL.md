---
name: hermes-gateway
description: Check status of, or reset, a Hermes graft agent's gateway. Use when a Hermes-graft agent's published endpoint record is stale (wrong schema_version, wrong owner_generation, or workspace-rooted instead of roster-home-rooted) and sends are accepted but the receiver hook fails to decode or route them, or when simply checking whether a profile's gateway is up. Triggers: "restart hermes gateway", "hermes gateway", "hermes gateway status", "reset hermes gateway".
---

# Hermes Gateway

Check status of, or reset, exactly one Hermes-graft agent's gateway. This
skill never edits the published endpoint JSON by hand — a reset always drives
the agent to regenerate it from the installed `atm-graft` bridge.

This skill vendors `hermes_gateway`, a status/restart utility originally
authored by the `skillrx` Hermes-graft agent for this skill, at
`.claude/skills/hermes-gateway/scripts/hermes_gateway`. Invoke it via that
explicit relative path — do not assume a bare `hermes_gateway` command is on
`PATH`.

## Required Parameter

- `agent_name` (**mandatory**) — the Hermes-graft agent whose gateway is being
  checked or reset, e.g. `skillrx`. Every invocation must specify exactly one
  `agent_name`; this skill does not support a fleet-wide or unscoped
  operation.

## Invocation

```
/hermes-gateway <agent_name>              # status only (default)
/hermes-gateway <agent_name> --reset      # full reset procedure
```

Refuse to proceed without an explicit `agent_name`.

## Always Get Agent Info First

Regardless of whether the invocation requests status or `--reset`, always
start by running:

```
.claude/skills/hermes-gateway/scripts/hermes_gateway <agent_name>
```

This reports the LaunchAgent PID, status (running/dead/stopped), and Telegram
chat ID sourced from `~/.hermes/profiles/<agent_name>/channel_directory.json`.
Use it to confirm the profile exists and to see its current state before
deciding whether a reset is even warranted — a status-only invocation stops
here.

## Reset Procedure (`--reset` only)

### Preconditions

1. Confirm the target agent's current endpoint record location, typically
   `<workspace>/.atm/graft/hermes/<agent_name>.json`.
2. Read the record and note `schema_version`. The current contract requires
   `schema_version: 2` with `owner_generation` (ULID), `owner_chat_id`,
   `loopback`, and `capability` fields. Any older/absent schema version, or a
   record missing those fields, is stale and in scope for reset.
3. Identify whether the record's path is workspace-rooted (wrong) rather than
   roster-home-rooted (correct). Roster-home resolution is normally reached
   via a profile symlink; a broken or missing symlink is the most common root
   cause of a workspace-rooted publish.

### Rules

1. **Never hand-edit the stale JSON record.** Editing the file only masks the
   underlying linkage/version defect and produces an unverified proof.
2. **Never fabricate or synthesize proof events.** A synthetic Telegram
   `MessageEvent` or equivalent does not satisfy the live-proof requirement —
   only an authenticated steer/send from the agent's real, running gateway
   counts.
3. **Never restart a gateway you are currently speaking through** without
   explicit confirmation — `hermes_gateway --restart` kills the LaunchAgent
   process, which drops the session driving it.
4. Instruct `agent_name` to:
   a. Run the installed `atm-graft` bridge from its real profile (not an ad
      hoc workspace checkout).
   b. Repair the endpoint-root linkage (recreate/repoint the profile symlink
      so bridge output resolves to the roster-home path the daemon looks up).
   c. Restart its gateway so it republishes under `schema_version: 2` with a
      fresh `owner_generation` ULID:
      `.claude/skills/hermes-gateway/scripts/hermes_gateway --restart
      <agent_name>`. This uses `launchctl kickstart -k` on
      `ai.hermes.gateway-<agent_name>` if already running, or
      `launchctl bootstrap` if stopped.
   d. Prove the reset with one authenticated steer/send through the live
      gateway.
5. Poll for the endpoint record to update (short fixed interval, bounded
   attempts — e.g. six 5-second polls), cross-checking
   `.claude/skills/hermes-gateway/scripts/hermes_gateway <agent_name>` to
   confirm the LaunchAgent actually came back up with a new PID. If the
   record has not advanced past the stale `schema_version` after the bounded
   poll window, escalate with the concrete diagnosis (record path, observed
   vs required schema_version, PID/address last seen) rather than continuing
   to poll indefinitely.
6. Once the record shows `schema_version: 2` with all required fields, verify
   the authenticated steer/send proof before declaring the reset complete.

## Non-Goals

- Does not reset more than one agent's gateway per invocation (`hermes_gateway
  --restart all` is out of scope here — this skill is single-profile only).
- Does not modify daemon-side lookup logic — this is an endpoint-publication
  reset on the agent side only.
- Does not accept a synthetic/simulated event as reset proof.
