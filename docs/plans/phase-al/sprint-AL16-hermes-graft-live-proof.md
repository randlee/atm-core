# AL.16 — Installable `hermes-atm` Package

**branch:** `plan/al16-hermes-graft-live-proof` (this planning branch only)
**implementation base:** a fresh `/sc-git-worktree` from the accepted
`origin/integrate/phase-al` SHA
**owners:** ATM integration owner (architecture/review), Cipher-311d (Python
binding and Hermes coordination)
**goal:** deliver the smallest installable `hermes-atm` integration that turns
an incoming typed ATM nudge into one visible notice and one normal turn in the
configured agent's **existing Telegram session**.

**required operator handoff:** [Hermes ATM Live-Proof Handoff](hermes-atm-live-proof-handoff.md)
is the self-contained coordination contract for Cipher-311d and
`skillrx@hermes`. It resolves the precise meaning of an inbound ATM nudge and
the live-profile publication prerequisite.

## First gate: prove the real nudge path, not a look-alike

Before broad package extraction or compatibility work, AL.16 must install the
smallest `hermes-atm` candidate into the actual M4 Hermes profile and prove one
end-to-end nudge without restarting the gateway. The exact required flow is:

```text
separate ATM sender durable write
  -> recipient graft receiver
  -> installed hermes-atm runtime in the active Hermes gateway profile
  -> typed PyNudge callback for that profile
  -> configured existing Telegram adapter and ATM_CHAT_ID
  -> visible host-originated Telegram notice
  -> internal MessageEvent on that Telegram adapter
  -> normal GatewayRunner message pipeline and agent turn
  -> ordinary Telegram response in that same session
```

The old standalone prototype, a checkout import, a print callback, or a
fixture-only callback is not evidence. If this first gate fails, record the
smallest reproducible defect and stop; do not continue to the multi-profile,
recovery, CPython-3.14, M5, or PyPI work merely because unit tests pass.

### Actual Hermes semantics and the user-visible requirement

An ATM nudge is an **inbound host event** for the configured Hermes profile.
For the MVP it deliberately uses the profile's real Telegram adapter and
existing Telegram session, not a separate `Platform.ATM` session and not a
fake Telegram network update. The receiver callback constructs an internal
`MessageEvent` with `SessionSource(platform=TELEGRAM, chat_id=ATM_CHAT_ID,
user_id=ATM_CHAT_ID, profile=<profile>)`, then calls the documented
`GatewayRunner.inject_internal_message(...)` host API. The package supplies
the configured profile explicitly; it must not hard-code `skillrx`, construct
session keys itself, or call an adapter's `handle_message` method directly.
The gateway owns adapter lookup and the session key, so the event follows the
same normal agent-loop path as the configured Telegram conversation:

```text
PyNudge.body
  -> visible "ATM nudge received" notice
  -> internal MessageEvent on the real Telegram adapter
  -> agent:main:telegram:dm:<ATM_CHAT_ID>
  -> normal Hermes turn and normal Telegram response
```

The event is internal so it does not impersonate a remote Telegram user or
repeat user authentication. It is nevertheless a real turn in the existing
Telegram session, including when that agent is idle. When that session is
busy, Hermes's internal-event path queues the nudge silently for the next turn;
it does **not** inherit the normal Telegram input default of `interrupt` and
does not call `steer`. Human Telegram input remains free to use the existing
`/queue` and `/steer` controls. The notice is visible in
the user-facing chat; its default text identifies the nudge without exposing
the full private ATM message. The default nudge body is `read atm`, so the
agent retrieves the durable mail through the normal ATM client rather than the
nudge carrying or acknowledging mail itself.

The retained request-gated local proof hook is evidence of this exact path. It
is inert until explicitly requested and is never the production receiver: the
production callback runs for each typed `PyNudge`, without a gateway restart.

## The MVP model

AL.16 has exactly two installable Python distributions:

1. `atm-graft` is the generic Python adapter around the Rust/PyO3
   `atm-graft` client and receiver capability. It has no Hermes, Telegram,
   gateway, or host-session policy.
2. `hermes-atm` is the selected Hermes-facing Python package. It depends on a
   compatible `atm-graft` wheel and contains only Hermes lifecycle, session
   binding and Telegram-session injection composition. It can be updated when Hermes Agent changes
   without changing the generic adapter.

A PyPI JSON lookup on 2026-08-09 returned `404` for both `hermes-atm` and the
fallback `atm-hermes`; claim `hermes-atm` at the first authorized package
publication. If that name is claimed before publication, use the fallback
`atm-hermes` and change the package/import documentation in the same PR. This
keeps a Hermes installation explicit:

```text
pip install hermes-atm
```

The current checked-in Maturin project is named `atm-graft` but also ships
Hermes reference modules. AL.16 moves that Hermes-only code into
`hermes_atm`; `atm-graft` retains only generic typed bindings. This fixes the
packaging and ownership boundary without changing ATM transport, storage, the
legacy daemon, or the Telegram gateway protocol. Production gateway wiring is
AL.17; live proof is AL.18.

## Non-negotiable package boundary

The distributions are independently versioned and deliberately have a one-way
dependency. Their names, imports, ownership, and permissible changes are:

| Concern | `atm-graft` | `hermes-atm` / `hermes_atm` |
| --- | --- | --- |
| Purpose | Generic Python access to the Rust graft client and receiver | Hermes gateway lifecycle and Telegram safe-boundary composition |
| Dependency direction | Must not depend on Hermes | Depends on a compatible published/tested `atm-graft` wheel |
| May contain | PyO3 bindings, typed request/result values, endpoint ownership, session activation, and generic typed nudge callback | `HermesGraftRuntime`, environment/profile configuration, gateway event-loop scheduling, Telegram session binding, visible notice delivery, and internal-event injection |
| Must not contain | Hermes/Telegram imports, chat IDs, host session policy, gateway lifecycle, or steer code | Rust source copies, a second ATM client/receiver, direct storage/socket access, durable queue/replay, or daemon supervision |
| Change owner | ATM graft maintainers; changes require a generic-adapter contract review | Cipher and SkillRX may iterate in the Hermes harness until the live gateway behavior is correct, subject to the public adapter contract and Hermes review |

`hermes-atm` is the only package permitted to know that the host is Hermes.
An `atm-graft` release must remain usable by another Python host without
bringing in Hermes. Conversely, `hermes-atm` consumes the public `atm-graft`
API; it must not reach into private extension objects or import a source
worktree to bypass that API. Any capability needed by Hermes but absent from
the public adapter is first added as a generic, documented `atm-graft` API in
its own reviewed change; it is never reimplemented in `hermes-atm`.

The separate Hermes host contract is equally explicit: `hermes-atm` may call
only a **released, versioned** public Hermes lifecycle/injection capability
(`GatewayRunner.inject_internal_message(...)` for this MVP). A method that
exists only in a dirty local Hermes checkout, an untracked test, or an
undocumented startup-hook object is not a supported dependency. The package
must fail closed when the active gateway does not expose that capability; it
must not fall back to a private runner import or direct adapter
`handle_message` call. Hermes-side work that publishes this contract is a
separate reviewed/deployed dependency, and every supported Hermes environment
must rerun its affected live proof after deployment.

The iterative development loop is explicit: Cipher and SkillRX may repeatedly
install a candidate `hermes-atm` wheel into the live Hermes harness, exercise
the real profile lifecycle and Telegram-session injection fixtures, fix `hermes-atm` or Hermes
gateway code, and retest. A green reference fixture alone is not a release
claim. Each candidate must retain its `atm-graft` version, `hermes-atm`
version/wheel tag, Hermes revision, and interpreter lane in its result. The
generic adapter is changed only when the package-boundary table makes that
necessary.

The source of record for the distributable `hermes-atm` package is this ATM
repository. Once a candidate works in the Hermes harness, Cipher carries the
package source, package metadata, tests, and compatibility evidence back into
an `atm-core` PR. That reviewed ATM commit is the sole release source used to
build and publish the PyPI wheel. Hermes Agent changes remain in its own
repository and are linked by revision; neither a live harness edit nor a
Hermes Agent commit is a substitute for the reviewed ATM package commit.

## Python release-version boundary

The replacement daemon is intentionally identified by a Rust/Cargo build tag
such as `-beta-ai-N`. That tag is runtime-candidate identity only. It must not
be copied into Python package metadata, wheel filenames, lock constraints, or
the `hermes-atm` dependency declaration: Python packaging uses PEP 440, not
Cargo prerelease syntax.

For this MVP the published generic adapter is a final `atm-graft` **1.4.x**
release (for example, `1.4.2`), with no alpha, beta, release-candidate, dev,
local, or daemon-build suffix. `hermes-atm` must declare a normal PEP 440
constraint on the final generic adapter line, initially `atm-graft >=1.4,<1.5`.
Its own published version must also be a PEP 440 final release. It may advance
independently of the daemon candidate tag and does not imply a daemon release.

Before publication the release test must inspect the built wheel metadata and
installed environment, asserting all of the following:

1. `atm-graft` resolves to a final `1.4.x` PEP 440 version.
2. `hermes-atm` resolves to a final PEP 440 version and selects the declared
   compatible `atm-graft` interval.
3. No Python distribution version or dependency text contains `beta-ai`, the
   daemon version, a Cargo prerelease delimiter, or a source-worktree path.

The exact daemon SHA/tag is retained as redacted **test evidence** alongside
the wheel versions; it is not package identity. A candidate failing this
separation is a packaging defect, even if its wheel installs locally.

One Hermes agent profile owns one tuple:

```text
(ATM_HOME, ATM_TEAM, ATM_IDENTITY, ATM_CHAT_ID)
```

For example, `skillrx@hermes` and `hendrix@hermes` use different
`ATM_CHAT_ID` values and therefore different configured host sessions. Each
profile starts one `HermesGraftRuntime`, which owns exactly one graft receiver
for `(canonical graft root, team, agent)`. The receiver's endpoint is selected
by recipient agent/team; the configured `ATM_CHAT_ID` selects that receiving
profile's Telegram session. The sender's source/chat identity is attribution
and reply metadata only. It must never select a Hermes session.

The only host injection is:

```text
durable ATM write
  -> recipient graft receiver callback
  -> AtmGraftAdapter on the gateway event loop
  -> typed PyNudge callback for this profile
  -> select the configured profile and its existing Telegram adapter/session
  -> emit one visible host-originated notice
  -> GatewayRunner.inject_internal_message(...) creates the internal
     Telegram-source event with that profile identity
  -> normal message pipeline in that exact Telegram session
```

No separate ATM session, synthetic Telegram network update, second mailbox,
replay queue, poll loop, second receiver, or daemon-owned Hermes session is
permitted.

## Binding requirements

| Requirement / decision | AL.16–AL.18 implementation and proof |
| --- | --- |
| `REQ-GRAFT-PYTHON-001`, ADR-039 | `hermes-atm` uses only the existing PyO3 `atm-graft` API. It does not open a socket, access storage, or add a send/read/ack path. |
| `REQ-GRAFT-RUNTIME-002`, ADR-043.1 | One profile starts one generation-owned receiver. The endpoint record belongs to the receiver, never the Telegram gateway port, and restart reclaims only a stale/dead owner. |
| `REQ-GRAFT-NOTIFY-002`, ADR-043.2/6 | Nudge is a bounded, host-originated inbound wake signal. Failed callback delivery is observable and fails closed; there is no retry, durable graft state, or message replay. |
| `REQ-GRAFT-HERMES-002`, ADR-039, ADR-043.3 | `ATM_CHAT_ID` and an explicit Hermes profile binding are required at startup. A typed callback calls the public `GatewayRunner.inject_internal_message(...)` API, which sends the visible notice and injects an internal Telegram-source event into that profile's existing session. Tests prove one profile cannot target another profile's chat. |
| `REQ-GRAFT-HERMES-003`, ADR-043.4 | After listening, exactly one ten-second count-only recovery summary may use the same configured Telegram-session injection path. It must not read, acknowledge, mutate, or replay mail. |

## Delivery order

## Scope

1. Keep `atm-graft` as the one generic Maturin/PyO3 adapter: typed client,
   session activation, endpoint ownership, and typed nudge callback only. It
   must contain no Hermes/Telegram imports or host-session policy.
2. Create a dedicated `hermes-atm` Python distribution which depends on a
   compatible `atm-graft` wheel. Move the existing loader, bridge, and adapter
   into that package, with one documented public runtime entry point:
   `HermesGraftRuntime.from_environment(request=..., resolve_session_id=...)`.
   Do not copy the Rust source or create a second ATM transport client. Claim
   the PyPI project name only through the authorized release workflow; a
   successful HTTP `404` lookup is availability evidence, not ownership.
3. Add an isolated-venv wheel-install test: install `hermes-atm`, import the
   runtime, and show missing `ATM_HOME`, `ATM_IDENTITY`, `ATM_TEAM`, or
   `ATM_CHAT_ID` fails before receiver activation. The test must not need a
   Hermes checkout.
4. Publish the actual wheel name/version/install command in its package
   metadata and operator documentation. Do not claim that the current
   `atm-graft` wheel is already named `hermes-atm`.
5. In the actual M4 CPython 3.13 profile, prove one unique durable ATM marker
   invokes the typed receiver callback, delivers one visible host-originated
   Telegram nudge notice, and starts a normal response in the existing
   Telegram session. Retain redacted evidence for the durable write, typed
   callback, selected Telegram session key, outbound notice, and ensuing agent
   output. The proof uses no separate ATM session, implicit read/ack,
   retry/replay, restart, or second receiver.
6. While an ordinary turn in that **same Telegram session** is active, send a
   second unique marker. The result must remain queued until the first turn
   ends, then drain exactly once. A simultaneous CLI, cron, or different-chat
   turn is not a busy-queue proof because it has a different session key.

### Required Python compatibility evidence

The native PyO3 extension is interpreter-specific unless its build metadata
explicitly proves an `abi3` wheel. AL.16 must therefore build and install—not
merely import from a source checkout—the matching wheel in both supported
Hermes environments:

| Lane | Interpreter | Purpose |
| --- | --- | --- |
| Hermes/M4 live gateway | CPython 3.13 | The currently running Hermes/SkillRX gateway interpreter; live Telegram proof must use this lane. |
| Hermes/M4 compatibility | CPython 3.14 | Explicit upgraded-interpreter wheel compatibility evidence. It becomes a live lane only after Hermes is actually switched to it. |
| Hermes/M5 | CPython 3.11 | Default Hermes-agent compatibility target. |

For each lane, retain the interpreter path/version, wheel filename/tag,
`pip install hermes-atm` result, `just test-hermes-graft-bridge` result, and
the isolated-import/startup-validation result. A wheel built for one CPython
minor version is never accepted as evidence for another. If Maturin/PyO3
cannot build a required lane, treat it as a packaging defect and fix the
supported build configuration; do not silently change the running Hermes
interpreter or bypass pip with `PYTHONPATH`.

## Acceptance

AL.16 is ready to merge only when:

1. `pip install hermes-atm` installs a tested Python host integration that
   depends on the one generic `atm-graft` implementation.
2. The native `atm-graft` wheel and the pure-Python `hermes-atm` wheel are
   separately built, versioned, and have no source-worktree import path.
3. All three interpreter lanes pass the package/import/bridge gates.
4. The generic `atm-graft` wheel contains no Hermes gateway lifecycle,
   Telegram routing, or session-injection implementation.
5. CI and quality review pass. AL.17 cannot begin until its package artifact
   and exact version contract are available.
6. A boundary test proves `atm-graft` has no Hermes/Telegram dependency and
   `hermes-atm` uses only documented public `atm-graft` APIs. A harness
   candidate may be iterated freely, but cannot merge by importing a checkout
   or changing the generic adapter through private coupling.
7. The active Hermes gateway exposes the versioned public lifecycle/injection
   capability required by `hermes-atm`. A local-only Hermes source modification
   is insufficient; each supported Hermes version/environment must prove the
   installed package can obtain the capability before live delivery is claimed.
7. The working candidate is committed in `atm-core` with package metadata,
   tests, and interpreter evidence before publication. PyPI publication is
   performed only from that accepted commit through the authorized release
   workflow.
8. Built-wheel and installed-metadata tests prove the PEP 440 final-version
   contract: `atm-graft` is 1.4.x without a beta suffix and neither Python
   distribution leaks the daemon's `-beta-ai-N` build tag.
9. The live proof demonstrates both distinct outcomes: the inbound ATM host
   event enters `agent:main:telegram:dm:<ATM_CHAT_ID>` through the configured
   Telegram adapter and produces one visible host-originated notice plus the
   ensuing normal agent response. It works for an idle agent and never creates
   a separate ATM session.

## Follow-on sprints

- [AL.17 — Hermes Gateway Lifecycle Binding](sprint-AL17-hermes-gateway-lifecycle.md)
  consumes the released/tested package in the actual gateway process.
- [AL.18 — Hermes Telegram Live Proof](sprint-AL18-hermes-telegram-live-proof.md)
  proves durable-write-to-safe-boundary delivery and recovery behavior.
