# AL.19 — Hermes ATM M5 Multi-Python Verification

**execution host:** M5 (`rand-m5`)
**execution worktree:** the M5 owner's named AL.19 evidence worktree, checked out from the exact candidate under test
**Hermes target:** M5's actual installed Hermes gateway service, plus an isolated CPython 3.11 wheel-compatibility lane
**owners:** M5 `arch-ctm` (execution and evidence), Cipher-311d (ATM package coordination), `skillrx@hermes` (Hermes-side advice/review), ATM integration owner (architecture review)
**starts when:** buildable `atm-graft` and `hermes-atm` candidate wheels exist; PyPI publication is not a prerequisite

## Goal

Verify `hermes-atm` as a deliberately multi-Python-version integration package.
AL.19 has two distinct evidence lanes:

1. an isolated CPython 3.11 compatibility lane that builds, installs, imports,
   and runs the managed package tests with the matching native `atm-graft`
   wheel; and
2. a live lane using the interpreter and Hermes Agent version that actually
   serve the M5 profile.

These lanes must never be conflated. The initial inventory found that M5's
active gateway was CPython 3.14.6 with Hermes Agent 0.19.0, not CPython 3.11.
Thus a successful 3.11 wheel is useful early portability evidence, but cannot
prove live delivery. Conversely, a live 3.14 service result does not prove the
CPython 3.11 extension is compatible.

## Design contract under verification

| Distribution | Responsibility | Must not own |
| --- | --- | --- |
| `atm-graft` | Generic PyO3 receiver/client bindings and typed `PyNudge` delivery | Hermes imports, Telegram session policy, chat IDs, gateway lifecycle, or source-checkout coupling |
| `hermes-atm` | Installed pure-Python composition: explicit profile configuration, receiver activation, event-loop handoff, visible notice, and selected-session delivery through the deployed `GatewayRunner.inject_internal_message(...)` contract | Direct storage/socket access, ATM-owned session, second receiver, retry/replay state, private PyO3 import, hard-coded profile, session-key construction, or direct adapter `handle_message` calls |

(Hermes `mode="queue"|"steer"` is Hermes's session-dispatch mode; ATM's Phase-AQ queue/steer *nudge kinds* align with but are distinct from it.)

The current MVP mode is Hermes's internal-event **queue** seam. It is the
first supported delivery mode, not a permanent product preference. An active
matching Telegram session queues an event; it does not interrupt or invoke
steer. A future explicit ATM steer mode is outside this sprint.

## Preconditions and frozen inputs

Before each evidence row, record:

1. candidate source SHA and selected Tokio/Axum daemon candidate tag;
2. final PEP 440 versions and wheel filenames for `atm-graft` and
   `hermes-atm`;
3. the isolated CPython 3.11 executable/version **and** active service
   executable/version as distinct values;
4. active Hermes Agent version/import root and machine/OS identity; and
5. M5 worktree/branch and indexed report destination.

The daemon tag is evidence only: no Python package version, wheel metadata, or
dependency requirement may contain `-beta-ai-N`. Worktrees and virtual
environments must be clean. Do not use `PYTHONPATH`, `sys.path`, patched
wheels, hand-edited endpoint records, or a standalone bridge/prototype.

## Execution checklist

### A. Align the ATM runtime pair

1. Fetch the recorded candidate in the M5 worktree.
2. Build/install a matching `atm` CLI and Tokio/Axum `atm-http-runtime`
   daemon pair.
3. Use supported `daemon-switch` selection and verify `atm doctor --json`.
4. Record selected executable paths and redacted doctor output.

Frozen legacy `crates/atm-daemon` is never started, benchmarked, or repaired.

### B. Inventory the actual M5 Hermes service first

Before package installation, inspect the process that serves the selected
profile—not a convenient shell Python—and record its executable, interpreter,
`sys.path`, Hermes Agent version, module root, documented plugin/startup
context, and public capability keys.

Probe the deployed `GatewayRunner.inject_internal_message(...)` contract. If
the active service does not provide it, record a versioned Hermes compatibility
blocker and stop the live lane. Do not substitute private imports, direct
adapter calls, or a source-tree patch.

### C. Isolated CPython 3.11 compatibility lane

1. Create a fresh isolated CPython 3.11 venv. It is a compatibility lane, not
   assumed to match the active service interpreter.
2. Build the exact `atm-graft` and `hermes-atm` wheels from the recorded
   candidate; install both normally with `pip`.
3. Record interpreter, wheel tags, `pip show`, and
   `import atm_graft, hermes_atm` evidence.
4. Run `just test-hermes-graft-bridge` against CPython 3.11, focused runtime
   tests, and repository lint. Extend the managed runner only if interpreter
   selection is unavailable; do not create another smoke script.

This is early compatibility evidence until the relevant candidate is reviewed;
it is never presented as a live service result.

### D. Actual M5 service lane

1. Install matching candidate wheels into the active service environment using
   Hermes's supported package workflow. Do not replace the service interpreter
   merely to make it resemble the CPython 3.11 compatibility lane.
2. Configure one profile declaratively with `ATM_HOME`, `ATM_IDENTITY`,
   `ATM_TEAM`, and its profile-specific `ATM_CHAT_ID`.
3. Start/restart through the supported Hermes lifecycle. Verify that the
   process imports installed wheels—not an ATM source checkout—and publishes
   one schema-v2, generation-owned receiver.
4. Confirm `hermes-atm` supplies explicit profile/chat values only, while the
   deployed runner resolves adapter and session identity. Fail closed for
   missing configuration, missing adapter, or unavailable contract.

### E. Queue proof after the public contract exists

Use a separate registered ATM sender and unique markers; retain only redacted
identifiers in repository evidence.

1. **Idle:** send one marker while the configured Telegram session is idle;
   prove durable acceptance, callback, one visible notice, one selected
   existing session, and one normal response.
2. **Busy:** start a real ordinary Telegram turn in that exact profile and
   `ATM_CHAT_ID` session. Send a second marker while it is active; prove one
   queued event, no interrupt/steer, and one drain after completion. A CLI,
   cron, or another chat is not valid busy evidence.
3. **Isolation:** run an existing two-profile fixture, or live proof where two
   profiles exist, proving profile A cannot target profile B.
4. **Negative checks:** no implicit `atm read`/`atm ack`, second session,
   second listener, synthetic external Telegram update, retry/replay, or
   visible private message body.

### F. Defect handling and reporting

ATM package defects are isolated in fresh `origin/integrate/phase-al`
worktrees, tested, and quality-reviewed. Hermes capability defects are fixed
and reviewed in Hermes Agent, deployed, then the blocked row is rerun. Commit
only redacted reports through the existing `site/reports` navigation; keep raw
diagnostics locally. An evidence branch may retain reports but must not claim
a pass from unreviewed or local-only code.

## Required report matrix

| Row | Required evidence | Pass condition |
| --- | --- | --- |
| Runtime pair | selected CLI + daemon and doctor JSON | matched Tokio/Axum pair healthy |
| Active Hermes capability | active-service interpreter/module root and API probe | deployed runner contract available in the actual service |
| CPython 3.11 package lane | wheel tags, isolated install/import, bridge/runtime tests | no checkout import; both packages usable |
| Active M5 service lane | service interpreter/matching wheels and receiver lifecycle | installed package starts one current receiver |
| Idle queue delivery | durable marker, notice, response | exactly one intended session processes it |
| Busy queue delivery | same-session ordinary turn plus second marker | no steer/interrupt; exactly one later drain |
| Isolation | two-profile live proof or documented fixture | no cross-profile delivery |

## Acceptance criteria

1. The isolated CPython 3.11 environment installs/imports exact matching wheels
   without a source-tree dependency; final closure reruns this row on the
   reviewed candidate.
2. The actual M5 Hermes service separately installs its matching wheels,
   exposes the deployed runner contract, and starts one valid generation-owned
   receiver for the configured profile.
3. Both idle and busy queue probes pass in only the intended Telegram session.
4. Busy proof is demonstrably queue-based: no steer, interrupt, duplicate, or
   lost queued nudge.
5. Existing bridge/runtime tests, lint, and doctor are green for the frozen
   candidate.
6. The indexed report records CPython 3.11 compatibility separately from the
   active M5 service lane and from M4 evidence.
7. Every defect is either fixed through its own reviewed PR and rerun, or
   remains an explicit blocker; no local workaround is called release-ready.

## Non-goals

- Replacing queue with steer or deciding final product policy between them.
- New transport, storage, replay, retry, fan-out, gateway, or TLS features.
- Modifying or using frozen legacy `atm-daemon`.
