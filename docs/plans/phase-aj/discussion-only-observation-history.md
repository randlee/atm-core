# Discussion Only: Observation History And Resume

> Not a Phase AJ deliverable. Not an approved requirement, sprint, dependency,
> or implementation authorization. Discard freely.

## Question

AJ retains the current trusted session/pid in memory and emits structured
transition events. That makes the last session visible while the daemon lives,
but does not promise a restart-surviving `/resume` or `/continue` lookup.

## Candidate Follow-Up

If restart-surviving session history becomes required, add one daemon-owned
history model rather than storing pid/session on mail:

- immutable observation-transition records keyed by team, member, observed-at;
- raw session/pid and prior/new values for diagnostic history;
- no read/write/ack mail-row changes and no message-payload fields;
- explicit retention, privacy, doctor query, and roster projection contracts.

This needs a separate requirement and ADR. It must not be slipped into AJ's
hot dispatch path or used as lifecycle/routing/nudge policy input.
