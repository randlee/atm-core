# atm-graft

Python bindings for the ATM ([atm-core](https://github.com/randlee/atm-core/blob/develop/README.md)) graft client, used to
embed an agent host in an ATM team.

`atm_graft` is a translation layer over the Rust `atm-graft` client. A host
process uses it to send, read, list and acknowledge ATM messages through the
local ATM daemon, and to receive the between-tool-call nudges that tell an idle
agent new work has arrived. It does not open a database or a daemon socket
itself; every operation goes through the typed graft client.

The public surface includes:

- request models `AtmSendRequest`, `AtmReadRequest`, `AtmListRequest` and
  `AtmAckRequest` (pydantic), with matching result types;
- `PyGraftSession`, `PyGraftSessionOptions`, `PyGraftSessionSnapshot` and
  `PyMailboxWorkCounts`;
- `PyNudge`, `PyCronNudge` and `PyObservability`;
- `AtmGraftError` for failures.

[hermes-atm](https://pypi.org/project/hermes-atm/) is the reference consumer:
the Hermes gateway integration built on this package.

Requirements and design:
[docs/atm-graft/requirements.md](https://github.com/randlee/atm-core/blob/develop/docs/atm-graft/requirements.md) and
[docs/atm-graft/hermes-agent-use-case.md](https://github.com/randlee/atm-core/blob/develop/docs/atm-graft/hermes-agent-use-case.md).

License: MIT OR Apache-2.0.
