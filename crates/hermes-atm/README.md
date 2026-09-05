# hermes-atm installation

`hermes-atm` is the package-owned Hermes gateway hook for ATM graft nudges.
It installs one receiver for one Hermes profile and injects delivered nudges
through the public `GatewayRunner.inject_internal_message(...,
mode="queue"|"steer")` API according to the additive ATM nudge kind. It does
not open an ATM database, select a Hermes adapter, or require any
post-install source edits.

## Required settings

Collect these values before installation. Keep the chat identifier in local
profile configuration or the launch agent; do not put it in source, commits,
or shared install notes.

| Setting | Purpose | Must match |
| --- | --- | --- |
| `profile` | Hermes profile to receive nudges | Gateway launch profile |
| `profile_home` | Directory containing that profile's `hooks/` directory | ATM roster `home_dir` |
| `identity` | ATM agent/member name | ATM roster member name |
| `team` | ATM team containing the member | ATM roster team |
| `chat_id` | Hermes session chat binding | Gateway's configured session |
| `atm_home` | ATM durable home | `ATM_HOME` for the profile |
| `workspace_root` | Canonical graft endpoint root | ATM roster `workspace_root` exactly |
| `launch_agent_plist` | Gateway LaunchAgent plist | Its first `ProgramArguments` entry must be the Python running this installer |

The `workspace_root` equality is mandatory. ATM post-send delivery resolves a
recipient endpoint from roster metadata. If the receiver is installed beneath
one root while the roster names another, `atm send` can persist a message but
the nudge will fail closed because it cannot find or contact the receiver.

## Clean installation sequence

1. Install a Hermes Agent version that exposes the public
   `GatewayRunner.inject_internal_message` and `gateway.config.Platform.TELEGRAM`
   host seams.
2. Configure the profile's launch agent with `ATM_HOME`, `ATM_IDENTITY`,
   `ATM_TEAM`, `ATM_CHAT_ID`, and `ATM_WORKSPACE_ROOT`.
3. Install the immutable wheels into that launch agent's Python environment:

   ```sh
   python -m pip install --upgrade atm-graft hermes-atm
   ```

4. Publish the same profile and graft root to ATM's roster. Run this using the
   profile's ATM identity and team:

   ```sh
   atm teams update-member <team> <identity> \
     --home-dir <profile-home> \
     --workspace-root <workspace-root> \
     --harness hermes
   ```

5. Run the package installer with the launch agent's Python. The command
   validates the public Hermes capability and interpreter match before it writes
   the standard generated hook and the package-owned native-tools plugin. It
   also declaratively enables `hermes-atm-native-tools` and its `atm` toolset
   for every configured Hermes platform in `config.yaml`; do not make those
   configuration edits by hand:

   ```sh
   python -m hermes_atm install \
     --profile <profile> \
     --profile-home <profile-home> \
     --identity <identity> \
     --team <team> \
     --chat-id "$ATM_CHAT_ID" \
     --atm-home "$ATM_HOME" \
     --workspace-root "$ATM_WORKSPACE_ROOT" \
     --launch-agent-plist <gateway-launch-agent.plist>
   ```

6. Reset the profile gateway. The restart loads the generated hook and publishes
   the receiver record under `<workspace-root>/.atm/graft/<team>/<identity>.json`.
7. From the same profile identity, send a localhost message to the profile and
   require an autonomous reply. Only that reply proves the full path: ATM send,
   receiver, Hermes queue injection, and agent context processing.

## Native ATM tools

The same installer registers exactly four native Hermes tools through the
public plugin API: `atm_send`, `atm_read`, `atm_list`, and `atm_ack`. They use
the typed `atm-graft` client and the installed profile's identity, team, and
workspace; tool arguments cannot override those profile settings. `atm_read`
is read-only, `atm_list` returns bounded metadata, and `atm_ack` acknowledges
one pending message through the canonical send path. Administrative and
advanced CLI operations remain `atm` CLI operations.

For the reproducible native-tool proof, see [NATIVE_TOOLS_PROOF.md](NATIVE_TOOLS_PROOF.md).
The implementation checklist and validation record are in [TASKLIST.md](TASKLIST.md).

Do not edit `hooks/hermes-atm/handler.py`, `HOOK.yaml`, or `config.json` after
installation. To change package behavior, publish a new wheel, reinstall it,
rerun `python -m hermes_atm install`, reset the gateway, and repeat the proof.
