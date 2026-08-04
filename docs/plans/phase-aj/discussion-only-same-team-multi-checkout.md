# Discussion Only: Same Team In Multiple Local Checkouts

> Not a Phase AJ deliverable. Not an approved requirement, sprint, dependency,
> or implementation authorization. Discard freely.

## Scenario

One computer runs two ATM checkouts — for example `atm-dev` from the internal
disk and `atm-dev` from an external SSD — while both use the same daemon.

## AJ Position

AJ keeps one current observation keyed by `(team, member)`. Each trusted
heartbeat or environment-attested CLI/graft event replaces that entry's current
pid/session when supplied. A change is retained as diagnostic evidence only.
No production decision distinguishes the two checkouts.

This deliberately supports today's diagnostic simplicity, not concurrent
instance identity. The roster may show the most recently observed session/pid;
it must not claim that this proves one checkout is inactive or rogue.

## Future Options To Evaluate

1. **Keep single current observation.** Lowest complexity; retained events and
   doctor diagnose frequent toggling. Appropriate until concurrent same-member
   work needs independent delivery or notification.
2. **Add an explicit instance key.** Define a stable, opt-in checkout/daemon
   instance identity and key observations by `(team, member, instance)`.
   Requires an ADR, wire contract, roster projection, migration strategy, and
   explicit routing/nudge semantics before implementation.
3. **Separate daemon namespaces.** Each checkout owns a distinct daemon and
   database/socket namespace. This avoids identity collision but changes the
   operational model and cross-daemon delivery story.

## Non-Negotiable Boundary

Neither pid, session id, checkout path, nor inferred agent state may decide
routing, nudge, retry, admission, delivery, or access. Any future multi-instance
design must preserve that boundary and prove it with tests.
