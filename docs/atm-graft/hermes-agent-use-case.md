# Hermes as an ATM client

Hermes receives ATM wake-up nudges through the independently installable
`hermes-atm` package. The generic `atm-graft` wheel supplies the typed PyO3
client and one receiver capability only; it contains no Hermes, Telegram, or
gateway-session policy.

`hermes-atm` also installs the initial native mailbox tools through Hermes'
public plugin seam: `atm_send`, `atm_read`, and `atm_list`. They use the same
typed public `atm_graft` API as the adapter and return the documented JSON
success/error union. The installed profile configuration, not tool arguments,
owns identity, team, home, and workspace root. Native `atm_read` is strictly
read-only; acknowledgements remain an optional field on native `atm_send`.

## Production composition seam

`hermes_atm.HermesAtmRuntime` is the sole production composition seam. At
gateway startup, the installed `hermes-atm` hook reads its declarative local
profile configuration, activates one `atm_graft.PyGraftSession`, and passes
each typed `PyNudge` to the reviewed public
`GatewayRunner.inject_internal_message(...)` API. That API emits one visible
host-originated Telegram notice and injects one internal event into the
configured profile's existing Telegram session.

```text
durable ATM write
  -> recipient atm-graft receiver
  -> HermesAtmRuntime typed PyNudge callback
  -> GatewayRunner.inject_internal_message(profile, configured ATM_CHAT_ID)
  -> visible notice and normal Hermes turn in that Telegram session
```

The configured profile and `ATM_CHAT_ID` select the receiving Hermes session.
The sender address is attribution only; it never selects a session. The runtime
does not poll a mailbox, create a second ATM conversation, construct a
synthetic Telegram update, call a private gateway API, or use `steer`.

For messages that require an explicit ATM acknowledgement, pass the optional
`requires_ack` argument. The Python graft client sends and reads in-process;
acknowledgement is deliberately the native `atm ack` CLI workflow, which emits
the canonical linked write for the selected message:

```python
sender_session.send(receiver, "please confirm", requires_ack=True)
message = next(
    message for message in receiver_session.read() if message.body == "please confirm"
)
# Then use: atm ack <message-id> "confirmed"
```

## Installation and configuration

Install `hermes-atm` into the Python environment that runs the Hermes gateway,
then materialize its standard hook with `python -m hermes_atm install`. The
local hook configuration contains exactly one profile tuple:

```text
(ATM_HOME, ATM_TEAM, ATM_IDENTITY, ATM_CHAT_ID, ATM_WORKSPACE_ROOT)
```

The profile's published ATM roster row must use the same `home_dir`,
`workspace_root`, and `harness = hermes`. After installation, reset the
gateway so it loads the hook. The package README contains the complete safe
installation and self-send proof sequence. Do not hand-edit generated hook
files or patch the installed `atm-graft` wheel; rerun the package installer
after a package update instead.

`hermes-atm` depends only on final PEP 440 `atm-graft` releases in the 1.4.x
line. Cargo daemon candidate tags are runtime identity and are never Python
wheel versions or Python dependency constraints.

## Package boundary

`atm-graft` remains reusable by non-Hermes Python hosts. It may expose typed
request/result values, session activation, endpoint ownership, and the typed
nudge callback. It must not import Hermes or Telegram code, contain a chat ID,
or choose a gateway session. `hermes-atm` owns Hermes lifecycle, event-loop
scheduling, profile/session binding, notice delivery, and internal-message
injection. It consumes only public `atm-graft` APIs and does not implement a
second ATM transport, mailbox/replay queue, or daemon lifecycle.

The pre-AL.16 `atm_graft_hermes_loader`, `atm_graft_hermes_adapter`, and
`atm_graft_hermes_bridge` source paths were retired. They are neither shipped
nor supported operator entry points.
