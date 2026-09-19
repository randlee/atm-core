---
title: Herdr Integration
audience: end-user
reviewed_for_release: 1.6.0
---

# Herdr Integration

ATM can register a roster member with the `herdr` local-receiver backend. A
Herdr-backed member retains the ordinary ATM mailbox: durable delivery, audit,
and message identity remain in ATM. When delivery needs attention, ATM reaches
the live Herdr agent through Herdr's native IPC socket and uses the configured
built-in nudge/template path; the Herdr queue wake path also observes eligible
members for queued work and task reminders.

## Endpoint And Version Support

The default Herdr endpoint is `$XDG_CONFIG_HOME/herdr/herdr.sock` when
`XDG_CONFIG_HOME` is set; otherwise it is `$HOME/.config/herdr/herdr.sock`.
A roster member may instead name a Herdr session; the default session uses the
configuration root and a named session selects that named Herdr server.
Configure the member with `--backend herdr` and, when needed, `--session
<name>`; ATM does not start or rename a Herdr server for you.

ATM supports released Herdr versions 0.8.0 and later. Version 0.8.2 is the
current all-platform recording target; 0.8.1 was never released. See `atm
doctor --json` rather than assuming a socket, session, or client capability is
available.

For a configured Herdr backend, `atm doctor --json` reports `herdr.configured`,
the breaker, and `herdr.endpoints[]`. Each endpoint records its `session`,
`provenance`, `transport`, `endpoint`, `state`, `remedy`,
`capabilities.live_handoff`, and configured `members`. Use these endpoint
records to distinguish an unavailable server, an absent agent, and a target
configuration error.

## Durable Aliases And Unique-Names

An ATM member's canonical name remains its routing, audit, and message
identity. A roster alias is a separate live-agent target for Herdr. Configure
or remove it with the supported roster commands:

```bash
atm teams add-member atm-dev team-lead \
  --agent-type lead --backend herdr --alias team-lead_atm-dev
atm teams update-member --alias team-lead_atm-dev atm-dev team-lead
atm teams update-member --clear-alias atm-dev team-lead
```

`unique-name = alias ?? name MUST be unique across database`

The member's unique-name is its alias when set, otherwise its canonical name.
Every roster write that would create a new unique-name collision is rejected in
any team. Older collisions remain readable and `atm doctor` reports them for
the affected team as a `WarningRosterDrift` warning. Its finding begins
`effective roster name '<n>' for member '<m>' conflicts with member '<x>@<team>'; assign a unique --alias before the next roster write`.
The roster-write error names the conflicting owners and says: roster
unique-name collision(s): `<name>`: (team, member), ...; choose a distinct
`--alias` for one conflicting member.

For a Herdr member, the alias must match `[a-z][a-z0-9_-]{0,31}`. Prefer
`<identity>_<team>`, such as `team-lead_atm-dev`, on a shared Herdr server. The
actual Herdr pane or agent must use that name too:

```bash
herdr agent rename <pane_id> team-lead_atm-dev
```

Herdr agent names are unique per server. A second Herdr agent cannot start
with a colliding unique-name, so repair the doctor warning before starting the
member: set a distinct roster alias, then rename the Herdr agent to exactly
that alias.

`atm send <alias>`, `ATM_IDENTITY=<alias>`, and `--as <alias>` resolve to the
canonical roster member before routing, audit, self-send validation, and
mailbox access. The alias is not a second ATM identity.

## Diagnose And Repair

For `ATM_HERDR_AGENT_NOT_VISIBLE`, inspect `atm doctor --json` and correct the
named session, then rename the live Herdr agent or set a matching alias. For a
unique-name collision, run `atm teams update-member --alias
<unique-herdr-name> <team> <member>`, then rename that live Herdr agent to the
same value. Do not edit the database.

For daemon, log, and peer diagnostics, see [Doctor And Log](./doctor-and-log.md)
and [Troubleshooting](./troubleshooting.md).

Return to the [ATM User Guide](./README.md).
