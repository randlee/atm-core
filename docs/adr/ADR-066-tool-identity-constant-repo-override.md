# ADR-066 — Tool Identity: A Constant Sender Address, Overridden Only By Repository Configuration

| Field | Value |
| --- | --- |
| ID | ADR-066 |
| Status | Accepted |
| Date | 2026-10-18 |
| Scope | Every tool that sends ATM messages or assigns ATM tasks on a team's behalf: `atm-bd` (console and cron) today, cron oversight and gateways next |
| Relates to | ADR-065; atm-core issue #1870 (reserved `tool` team) |

## Context

`atm-bd` 0.16.1 runs every `atm send` and `atm task assign` as the phase
lead (`--as team-lead`). Problem batches addressed to the lead are refused as
self-sends, so the lead never receives them (bead atm-phase-bi.7). The fix
needs a sender identity of the tool's own, and a rule for where that
identity comes from.

Rand's rulings (2026-10-18):

- "we should send as atm-bd@tool (we can create a atm 'tool' team at install
  short term so this works)."
- "we should hard-code the tool identity in the future. accepting ATM_TEAM
  or ATM_IDENTITY will most likely cause more confusion that it is worth. if
  there is any over-ride of <identity>@<team>, it should happen as an
  optional config veriable at the repo level, not environment variable."
- "ATM_TEAM environment variable CAN be used to identify what team the cli
  is executing against, it should NOT be used as the atm address the tool
  uses for sending messages."

Verified on the live host the same day: `atm teams add-member tool atm-bd`
creates the `tool` team; a cross-team send from `atm-bd@tool` to
`team-lead@atm-dev` delivers; `atm task assign` across teams is refused
("task commands are local-team only"), which issue #1870 tracks.

## Decision

### D1. The sender address is a constant in the tool

Each tool carries its own sender address as a constant: `atm-bd@tool` for
`atm-bd`. The address is the member `atm-bd` of the reserved team `tool`.

### D2. The only override is repository configuration

A repository may override the address with one optional key in its
orchestration configuration file (`.claude/project/atm-bd-orchestration.yaml`
for `atm-bd`), holding a full `<identity>@<team>` address. No environment
variable, command-line flag or phase file sets the sender.

### D3. ATM_IDENTITY and ATM_TEAM are never the sender

The tool does not read `ATM_IDENTITY` to pick its identity. `ATM_TEAM`
may tell the tool which team it is executing against (the target team),
never which address it sends from. The environment the tool sets on its
own `atm` subprocesses is derived from D1 or D2 and is internal mechanics,
not an input.

### D4. The `tool` team exists before the tool runs

Until atm-core reserves the `tool` team (#1870), the tool's install step
creates it with the tool's member when it is missing, and the tool reports
a typed startup error naming that step if the address does not resolve.

### D5. Task assignment stays with the lead until atm-core allows cross-team assignment

`atm task assign` across teams is refused today, so a tool assigns tasks as
the phase lead until #1870 lands; sends go as the tool now. The two halves
are tracked separately so neither waits on the other.

## Consequences

- A lead can receive a tool's reports; a self-send can no longer occur.
- A reader of any tool message knows the sender from the address alone.
- Review check: a tool that reads `ATM_IDENTITY` for its own identity, or
  takes a sender from a flag or environment variable, is a finding.

## BK-9 amendment — reserved virtual namespace and authenticated principals

The historical temporary physical `tool` roster workaround is superseded.
`tool` is a reserved virtual namespace, not an ordinary `TeamName`; no normal
team creation, member administration, or agent launch may create `cron@tool`
or another `<agent>@tool`. `cron@tool` is a built-in daemon principal.

Plug-in principals are fixed authenticated registrations, not a tool-selected
address. The daemon admits a signed installer request using an administrator
protected trust root and admits each runtime message using a sender, host,
generation, message-id/digest, key, and nonce bound proof. It then mints a
non-serializable internal capability. Environment variables, CLI flags, hook
JSON, repository config, and an mTLS peer identity are insufficient. A peer
requires an origin-host attestation for the exact admitted message. These
rules preserve readable historical `@tool` mail while prohibiting new
impersonation and do not grant cross-team task ownership.

### BK-9 contract record — proof framing

All proof signing inputs are canonical UTF-8 field frames: every field is
preceded by its unsigned 32-bit big-endian byte length. Numeric registration
generation is one unsigned 64-bit big-endian field. The opaque `signature` is
always excluded. The field order is:

- installer (`ATM-TOOL-INSTALL-v1`): domain, schema version, operation,
  installer id, installer key id, sender, package digest, registration
  generation, host id, runtime key id, runtime public key, previous runtime
  key id (empty when absent), nonce, issued at, expires at;
- runtime message (`ATM-TOOL-MESSAGE-v1`): domain, runtime key id, sender,
  registration generation, host id, message id, message digest, nonce;
- peer attestation (`ATM-TOOL-PEER-v1`): domain, sender, registration
  generation, host id, message id, message digest, nonce.

BK-9 freezes these value contracts only. BK-10 owns SQLite persistence for
tool registrations; BK-11 owns the installer contract documentation and
deterministic fixtures; BK-15 owns installer validation, protected
trust-root loading, and production daemon composition of the authenticated
adapter.
