# Herdr Compatibility Ledger

Status: AY.1 baseline, 2026-09-06

This ledger is the compatibility authority for the Herdr client contract. ATM
supports every Herdr release at or above `HERDR_MINIMUM_VERSION`; it does not
select the newest release and does not require a daemon restart when Herdr is
upgraded. The current minimum is **0.8.0**, set by Rand under ADR-061. The
design and recording target is v0.8.2. There is no v0.8.1 release.

`PROTOCOL_VERSION` is recorded as a secondary fact. It versions Herdr's
bincode client socket, not the NDJSON API used by the ATM CLI transport, and
must not be used as the compatibility floor. For the direct socket transport,
ATM keys compatibility on the Herdr release reported by `ping.version` and
feature/capability data.

## Supported release matrix

| Herdr reference | Release/provenance | `PROTOCOL_VERSION` | Windows artifact | ATM support status | Conformance recording |
| --- | --- | ---: | --- | --- | --- |
| v0.8.0 | tag commit `857196de`, 2026-08-03 | 19 | No official Windows artifact; Windows was beta packaging without a release job | Minimum supported release; Unix lanes | v0.8.2 design recording plus the v0.8.0 blocked-prompt delta mode |
| v0.8.1 | No tag/release | — | — | No release to support or record | None; do not invent a compatibility row |
| v0.8.2 | tag commit `34ba52cc`, 2026-08-19; design target | 20 | Official Windows x86_64 artifact | Supported on all released platforms | `docs/plans/phase-aq/fixtures/herdr-cli-contract-fixture.md` F1–F6 |
| `3a822e81` | master reference, 2026-09-05; untagged snapshot | 22 | Installer/release changes are recorded as drift; not a released floor | Reference-only drift target; additive changes must remain compatible | Compare against v0.8.2; add a recording only if an ATM operation changes |

The v0.8.0-to-v0.8.2 review found identical argv, flags, JSON shapes, and exit
codes for all six ATM operations. Its only ATM-path error-behaviour delta is
the v0.8.2 pre-write `agent_blocked` rejection. The v0.8.2-to-master review
found the same six-operation surface and error-code set, with one material
`agent prompt --wait` activity-gate outcome change: `timeout` may replace
`agent_prompt_stalled`. ATM maps both by stable code and does not inspect
message text.

## Six-operation contract manifest

The following manifest is repeated for each supported release. The v0.8.2
recording is the canonical shape; v0.8.0 uses it plus the documented
blocked-prompt delta; master uses it plus the documented wait-gate delta.

| Operation | Direct argv | Success JSON shape / consumed fields | Failure shape and exit | ATM use |
| --- | --- | --- | --- | --- |
| `agent prompt` | `herdr agent prompt <AgentName> <rendered nudge template>` | `{"id":"cli:agent:prompt","result":{"type":"agent_prompted","agent":{...}}}`; no agent fields are consumed by the immediate path | `{"id":"...","error":{"code":"...","message":"..."}}`, exit 1; exit 2 is an impossible argv-construction bug | Immediate steer; no `--wait`; rendered built-in template only |
| `agent wait` | `herdr agent wait <AgentName> --until idle --until done --until blocked --timeout <ms>` | `{"id":"cli:agent:wait","result":{"type":"agent_info","agent":{"agent_status":"...",...}}}`; `agent_status` is the contracted field | `agent_not_found`, `agent_not_running`, `timeout`, and transport codes as structured exit-1 errors; malformed status/timeout is exit 2 | Retained adapter contract; not invoked by Phase AQ |
| `agent get` | `herdr agent get <AgentName>` | `{"id":"cli:agent:get","result":{"type":"agent_info","agent":{"agent_status":"...",...}}}`; doctor reads `agent_status` only | `agent_not_found` or `agent_target_ambiguous`, plus transport codes, as structured exit-1 errors; malformed arity is exit 2 | Doctor presence probe; `BreakerPolicy::Bypass` |
| `agent list` | `herdr agent list` | `{"id":"cli:agent:list","result":{"type":"agent_list","agents":[...]}}`; ATM reads each entry's `name` and `agent_status` | A missing member is a normal exit-0 list absence; transport failures are structured exit-1 errors; extra argv is exit 2 | Queue-pump polling, once per distinct configured session |
| `notification show` | `herdr notification show <title> --body <body> --sound request` | Exit 0; title/body are separate argv values. No response field is part of ATM's contract | Non-zero is a typed adapter failure; error code is parsed when provided | Lead escalation notification; independent of mail delivery |
| `status server --json` | `herdr status server --json` | JSON includes `version` and `protocol`; the socket transport's equivalent is `ping` | Doctor-only transport failure; never used to gate daemon startup or messaging | Doctor server-status probe, not a nudge |

Source and fixture anchors for the manifest are ADR-058 D2–D10, the Phase AY
plan's request-set table, and
[`herdr-cli-contract-fixture.md`](../plans/phase-aq/fixtures/herdr-cli-contract-fixture.md)
F1–F6. The fixture is derived from source, not a live transcript; it uses
placeholder names and omits mutable message text from the compatibility rules.

## Version-specific deltas

### v0.8.0

- `PROTOCOL_VERSION` is 19.
- `agent prompt`, `agent wait`, `agent get`, `agent list`, `notification
  show`, and `status server --json` retain the manifest's argv and response
  shapes.
- `agent prompt` to an already blocked agent submitted and then waited. The
  fake-Herdr replay for v0.8.0 must model this delta; ATM's outcome remains
  compatible with the normal wait/error handling.
- Windows has no official release artifact at this version. This is a
  packaging fact, not a Windows feature exclusion or a raised minimum.

### v0.8.2

- `PROTOCOL_VERSION` is 20.
- This is the canonical design recording and the first official Windows
  release artifact.
- A blocked prompt is rejected before input with `agent_blocked`.
- The canonical recording is the existing F1–F6 fixture set.

### master `3a822e81`

- `PROTOCOL_VERSION` is 22; this is not a new minimum.
- `agent prompt --wait` has the activity-gate drift recorded in the Phase AY
  plan: `timeout` can replace `agent_prompt_stalled`, and the message text is
  not stable. Parsers must continue to key on codes.
- Endpoint resolution, NDJSON framing, agent JSON fields, notification argv,
  and exit-code surfaces remain compatible. Additive fields are ignored.
- Windows installer PATH handling prepends the versioned release directory;
  ATM resolves the configured/binary alias per spawn and never persists the
  resolved path.

## AY.2 recording-manifest path

AY.2 owns the portable fake-Herdr recordings and replay fixtures. This ledger
records their manifest path without copying fixture bodies:

```text
crates/atm-herdr/tests/fixtures/herdr-versions/manifest.json
```

The manifest must identify the release, source revision, `PROTOCOL_VERSION`,
and the six operation recording names. The v0.8.0 replay is a delta mode over
the v0.8.2 recording, not a second copied body. A future release adds a new
recording set only when the drift procedure below finds a change in one of the
six operations.

## Drift-check procedure

At every AY sprint start and at phase end:

1. Confirm the maintained reference checkout is
   `/Users/randlee/Documents/github/herdr`; record its branch and `HEAD`.
2. Compare the last recorded revision with the current reference:
   `git diff <last-recorded>..HEAD --` over the client-facing paths below.
3. Review changes for argv, flags, JSON fields, error codes, exit status,
   endpoint selection, framing, and platform process behaviour. Ignore
   message text and JSON key ordering.
4. If none of ATM's six operations changes, append a dated no-surface-drift
   entry. If one changes, add a recording manifest or a documented delta,
   update the affected parser/fixture contract, and obtain the required
   schema review before accepting the release.
5. If a change cannot be absorbed additively, escalate to Rand for either a
   recorded `HERDR_MINIMUM_VERSION` decision or an approved second transport
   implementation. Never silently raise the minimum or assume the newest
   Herdr.

Client-facing paths scanned by the drift check:

```text
src/api/
src/cli/agent.rs
src/cli/notification.rs
src/cli/spec.rs
src/cli.rs
src/cli/protocol_guard.rs
src/ipc.rs
src/session.rs
src/integration/env.rs
src/protocol/wire.rs
distribution/
```

## Ledger entries

| Date | Compared revisions | Result | Action |
| --- | --- | --- | --- |
| 2026-09-05 | v0.8.0 `857196de` → v0.8.2 `34ba52cc` | Six-operation argv/flags/JSON/exit surface unchanged; blocked-prompt rejection added | One v0.8.2 recording plus v0.8.0 fake delta |
| 2026-09-05 | v0.8.2 `34ba52cc` → master `3a822e81` | Six-operation surface and error-code set unchanged; wait-gate and protocol integer drifted | Keep one recording; parse stable codes and ignore additive fields |
