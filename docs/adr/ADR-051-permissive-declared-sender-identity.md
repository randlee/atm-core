# ADR-051 — Permissive Declared Sender Identity and Roster Advisory

| Field | Value |
| --- | --- |
| Status | Accepted |
| Scope | Local CLI sender declaration, roster metadata, message envelopes |
| Relates to | ADR-001, ADR-026, ADR-035; GitHub issue #881 |

## Context

ATM runs in a trusted-local environment. The sender recorded in a message is
the identity asserted by the invoking CLI or integration; it is not an
authenticated principal. This is intentional: constrained harnesses,
temporary or isolated agents, bootstrap workflows, and automation tools can
send useful, self-describing messages before they have a roster entry. For
example, a future tool may identify itself as `gh`, `git`, or `github` in a
team without ATM reserving any of those names.

Treating roster membership as authorization would not provide meaningful local
security. A process that can alter its CLI environment can also claim a
rostered identity, while rejecting unknown identities would force legitimate
temporary tooling to impersonate a known agent. The roster instead describes
where a declared identity has an inbox and can receive follow-up work.

## Decision

ATM continues to accept any syntactically valid declared sender identity in
the trusted-local CLI path. It does not reject, authenticate, normalize to a
fixed vocabulary, or otherwise alter delivery for an identity absent from the
team roster. The canonical HTTP request and daemon write path remain exactly
the same whether the sender is rostered or unrostered.

After a successful CLI write (`atm send` or `atm ack`), the CLI performs a
best-effort local roster lookup for the claimed sender and caller team. If no
roster entry exists, it adds a non-blocking outcome warning stating that the
identity has no inbox and cannot receive replies or assignments. The warning
includes the recovery command:

```text
atm teams add-member <team> <identity>
```

Human CLI output renders this advisory on stderr. JSON output retains it in
the outcome's structured `warnings` array, leaving stdout valid JSON. A roster
lookup failure emits no advisory and never changes the write result: local
metadata inspection cannot turn a completed delivery into a CLI failure.

## Consequences

- A useful unrostered identity remains visible in the message envelope rather
  than being hidden behind a misleading impersonated roster identity.
- Senders receive an immediate reminder on every successful send or ack that
  an unrostered identity cannot receive a mailbox reply or assignment.
- Recipients and team leads may inspect roster membership when an unexpected
  sender appears; this is organizational diagnosis, not an authorization
  decision.
- A future authenticated-principal model, if needed, must be introduced as a
  separate capability and transport decision. It must not reinterpret today's
  roster metadata as retroactive authentication.

## Rejected alternatives

1. **Reject sends whose declared sender is absent from the roster.** This
   blocks legitimate bootstrap and automation workflows while a local process
   can still trivially claim a rostered identity. It creates friction without
   delivering a real security boundary.
2. **Silently accept unrostered identities.** This preserves flexibility but
   leaves a sender expecting replies with no indication that it has no inbox.
3. **Reserve a fixed list of tool identities.** ATM must remain workflow and
   tool agnostic. Sender names are descriptive caller data, not ATM business
   vocabulary.

## Required evidence

- A rostered sender produces no advisory.
- An unrostered sender produces the inbox/recovery advisory in the human CLI
  path and serializes it through the relevant outcome's `warnings` array for
  JSON callers.
- Both CLI write commands use their existing successful canonical daemon
  request and delivery path; the advisory is appended only after a successful
  CLI outcome.
