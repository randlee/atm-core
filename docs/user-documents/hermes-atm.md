---
title: Hermes Gateway Integration
audience: end-user
reviewed_for_release: 1.4.4
---

# Hermes Gateway Integration

`hermes-atm` connects one Hermes gateway profile to ATM's graft receiver. It
delivers ATM nudges into Hermes through the public gateway API and registers
native mailbox tools for the profile. It does not require direct database
access, source edits, or a hand-written hook.

## Release Status

Version `1.4.4` is prepared but is not yet published to PyPI or TestPyPI.
Production publication remains pending this sprint's authorization. Do not
install an earlier TestPyPI candidate as a substitute; wait for the published
`1.4.4` release announcement.

`hermes-atm` supports CPython 3.11 through 3.14. It installs the matching
typed `atm-graft` client as a dependency.

## Before You Install

Install into the same Python environment that starts the target Hermes
gateway. The installed Hermes Agent must expose its public gateway-injection,
plugin-registration, and configuration APIs; the installer verifies this before
it changes the profile.

Collect these local profile values first. Keep the chat identifier in local
profile or launch-agent configuration; never put it in source, commits, or
shared troubleshooting notes.

| Setting | What it identifies |
| --- | --- |
| `profile` | Hermes profile name |
| `profile_home` | Hermes profile directory |
| `identity` | ATM member identity for the profile |
| `team` | ATM team containing that member |
| `ATM_HOME` | ATM durable home for the profile |
| `ATM_CHAT_ID` | Hermes session chat binding |
| `ATM_WORKSPACE_ROOT` | Canonical graft endpoint root |
| `launch_agent_plist` | LaunchAgent that starts this gateway |

The workspace root must exactly match the recipient's ATM roster
`workspace_root`. A mismatch can allow `atm send` to persist a message while
the receiver correctly fails closed instead of injecting the nudge.

## Install

After production PyPI publication, install the announced release with:

```bash
python -m pip install --upgrade "hermes-atm==1.4.4"
```

Register the profile with ATM before installing the receiver. Use the profile's
own identity and team:

```bash
atm teams update-member "$ATM_TEAM" "$ATM_IDENTITY" \
  --home-dir "$HERMES_PROFILE_HOME" \
  --workspace-root "$ATM_WORKSPACE_ROOT" \
  --harness hermes
```

Then run the package installer with that same gateway Python interpreter:

```bash
python -m hermes_atm install \
  --profile "$HERMES_PROFILE" \
  --profile-home "$HERMES_PROFILE_HOME" \
  --identity "$ATM_IDENTITY" \
  --team "$ATM_TEAM" \
  --chat-id "$ATM_CHAT_ID" \
  --atm-home "$ATM_HOME" \
  --workspace-root "$ATM_WORKSPACE_ROOT" \
  --launch-agent-plist "$HERMES_GATEWAY_PLIST"
```

The installer validates that the launch agent and installer use the same
Python interpreter. It then writes the standard receiver hook, installs the
package-owned native-tools plugin, and enables the `atm` toolset for the
profile's configured Hermes platforms. Do not make equivalent hook, plugin, or
`config.yaml` edits by hand.

## Reset And Verify

Reset the managed gateway once after installation. This loads the generated
receiver and native-tools plugin and publishes the graft receiver record for
the profile.

The reset should expose exactly these native Hermes tools:

- `atm_send` for ordinary mailbox delivery, with optional acknowledgement
- `atm_read` for bounded, read-only message inspection
- `atm_list` for bounded mailbox metadata

The tools use the installed profile's identity, team, and workspace root; tool
arguments cannot override them. Their results are structured success or error
envelopes, so an agent can act on a failure without parsing command output.

For the delivery proof, send one distinct localhost message to the installed
profile and wait for the agent's autonomous reply. That reply demonstrates the
whole supported path: ATM send, graft receiver, Hermes queue injection, and
agent context processing. Asking the agent to manually inspect its mailbox is
not an equivalent nudge proof.

## Updates And Recovery

After a package update, repeat this sequence:

1. Install the new `hermes-atm` wheel with the gateway Python.
2. Rerun `python -m hermes_atm install` for the profile.
3. Reset the managed gateway.
4. Repeat the autonomous localhost delivery proof.

Do not patch an installed wheel or generated files such as
`hooks/hermes-atm/handler.py`, `HOOK.yaml`, or the generated plugin
configuration. Reinstall and rerun the package installer instead.

If installation fails before writing files, confirm that the gateway uses the
same interpreter, that the required ATM environment values are present in its
launch configuration, and that the installed Hermes version exposes the public
capabilities named above. For ATM daemon and delivery diagnostics, see
[Troubleshooting](./troubleshooting.md).

Return to the [ATM User Guide](./README.md).
