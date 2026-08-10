# AL.19 — Hermes ATM M5 CPython 3.11 Verification

**execution host:** M5 (`rand-m5`)
**execution worktree:** the M5 owner’s named AL.19 worktree, checked out from
the exact versioned adapter candidate under test
**Hermes target:** M5’s newer installed Hermes harness running CPython 3.11
**owners:** M5 `arch-ctm` (execution and evidence), Cipher-311d (ATM package
coordination), `skillrx@hermes` (Hermes-side advice/review if needed), ATM
integration owner (architecture review)
**starts when:** the `atm-graft` public adapter shape and `hermes-atm` package
boundary are defined and buildable; public PyPI publication is not a
prerequisite

## Goal

Verify `hermes-atm` as a deliberately multi-Python-version integration package
by performing a fresh installation and live verification on M5’s newer Hermes
harness under CPython 3.11.

The M5 proof is materially different from the M4 package lanes. `hermes-atm`
is pure Python, but it consumes the interpreter-specific PyO3 `atm-graft`
wheel. Therefore a local CPython 3.13 or 3.14 result does not establish that
the matching CPython 3.11 native wheel installs, imports, starts inside the
newer M5 harness, and reaches the intended existing Telegram session.

AL.19 validates that the package boundary was designed for the supported
interpreter matrix rather than accidentally coupled to the machine where it
was first demonstrated. It is a fresh M5 install/upgrade and live-verification
pass, not a duplicate packaging sprint and not a source-checkout experiment.
It deliberately starts early: buildable candidate wheels are installed directly
into an isolated M5 CPython 3.11 environment as soon as the adapter shape is
defined. A later reviewed merge/release candidate reruns only the affected
evidence rows before any release claim.

## Design contract under verification

The two distributions remain separate and one-way:

| Distribution | Responsibility | Must not own |
| --- | --- | --- |
| `atm-graft` | Generic PyO3 receiver/client bindings and typed `PyNudge` delivery | Hermes imports, Telegram session policy, chat IDs, gateway lifecycle, or source-checkout coupling |
| `hermes-atm` | Installed pure-Python Hermes composition: explicit profile configuration, receiver activation, event-loop handoff, visible notice, and selected session delivery through `GatewayRunner.inject_internal_message(...)` | Direct storage/socket access, an ATM-owned session, a second receiver, retry/replay state, a private PyO3 import, a hard-coded agent/profile, or direct adapter `handle_message` calls |

The current MVP delivery mode is Hermes’s existing internal-event **queue**
seam. It is the first proven seam, not a permanent product preference. ATM
steer is a planned future delivery mode and is not implemented or invoked in
this sprint. The queue path must never interrupt an active Telegram run.

## Preconditions and frozen inputs

AL.19 may begin as soon as the adapter shape is defined and both candidate
wheels build. Before beginning each M5 run, record the following in the report
header:

1. the exact candidate source SHA (or uncommitted working-tree diff identifier)
   and the selected daemon candidate tag;
2. exact final PEP 440 versions and wheel filenames for `atm-graft` and
   `hermes-atm`;
3. M5’s Python executable and full version (`3.11.x`), machine/OS identity,
   and newer Hermes harness revision; and
4. the existing M5 worktree/branch and report destination.

Python versions are evidence dimensions, not interchangeable labels. The
daemon’s `-beta-ai-N` version is recorded separately and must not leak into
either Python distribution version or dependency specification.

The M5 owner must start from a clean named worktree and a clean CPython 3.11
virtual environment. Do not import from an ATM checkout using `PYTHONPATH` or
`sys.path`, patch an installed wheel, hand-edit a graft endpoint record, or
reuse an old standalone bridge/prototype process. Direct installation of wheels
built from the recorded candidate worktree is explicitly allowed and is the
intended early-testing path.

## Execution checklist

### A. Align the executable runtime pair

1. Fetch the recorded candidate in the M5 worktree.
2. Build/install the matching `atm` CLI and Tokio/Axum `atm-http-runtime`
   daemon pair.
3. Use the supported `daemon-switch` workflow to select the CLI and daemon as
   one pair, restart the one managed daemon, and verify `atm doctor --json`.
4. Capture the selected executable paths and doctor result in the redacted
   report.

No legacy synchronous daemon may be started, benchmarked, or repaired for this
sprint. Failure to switch the matched replacement runtime is an environment
blocker, not a reason to change the frozen daemon.

### B. Verify the active Hermes public capability before installation

Before installing a candidate into a live profile, inspect the actual Hermes
gateway service environment—not merely whichever `python` is convenient in a
shell—and record:

1. its executable, interpreter version, `sys.path`, installed Hermes version,
   and imported Hermes module root;
2. the documented plugin/startup hook context and its public capability keys;
3. a minimal import/capability probe for the public
   `GatewayRunner.inject_internal_message(...)` contract; and
4. the exact error if that contract is unavailable.

`hermes-atm` supports a harness only when that public contract is available in
the environment that actually runs the selected profile. A wheel-only CPython
3.11 import pass does not establish this. Do not substitute a private import,
direct adapter call, source-tree patch, or altered `sys.path` if the contract
is absent; record a versioned Hermes compatibility blocker and stop the live
lane. A later Hermes-side public API/lifecycle change must be reviewed and the
affected preflight plus live rows rerun.

### C. Install candidate distributions into the actual CPython 3.11 lane

1. Create a fresh CPython 3.11 venv used by the active M5 Hermes profile (or
   its documented replacement venv).
2. Build the exact `atm-graft` and separately built `hermes-atm` candidate
   wheels from the recorded source SHA, then install those wheel files using
   normal `pip` installation. Do not wait for a PyPI upload.
3. Record `python --version`, `pip show` output, wheel tags, and a plain
   `import atm_graft, hermes_atm` result.
4. Run the established `just test-hermes-graft-bridge` entry point against
   CPython 3.11. Extend this managed runner only if it cannot select the M5
   interpreter; do not create a second smoke script.
5. Run the focused `hermes-atm` runtime tests and the repository lint gate.

This step verifies the matrix design in a real additional interpreter lane;
it does not claim that one interpreter’s compiled extension works in another.
Until the candidate’s relevant package changes are reviewed, label the result
as **early compatibility evidence**, not a release pass.

### D. Bind the installed package to the newer M5 Hermes harness

1. Configure one M5 Hermes profile declaratively with `ATM_HOME`,
   `ATM_IDENTITY`, `ATM_TEAM`, and its profile-specific `ATM_CHAT_ID`.
2. Start/restart the profile through Hermes’s supported gateway lifecycle.
   The running gateway must import the installed wheel, not an ATM source
   checkout and not a copied bridge script.
3. Verify that one generation-owned, schema-v2 graft receiver is listening.
   Repair publication only by lifecycle reactivation; never by editing receiver
   JSON or accepting a schema-v1 fallback.
4. Confirm the package obtains only documented host capabilities: the public
   `GatewayRunner.inject_internal_message(...)` API, gateway event loop, and
   explicitly configured profile/session identity. It must not infer a profile
   from an ATM identity, hard-code `skillrx`, construct a session key, or call
   an adapter's `handle_message` method directly.
5. Fail closed if required environment/configuration is missing, if the
   selected profile has no live Telegram adapter, or if profile/session identity
   cannot be resolved safely.

### E. Prove idle and busy queue behavior

Use a separate registered ATM sender and unique durable markers. Retain only
redacted identifiers in repository evidence.

1. **Idle proof:** while the target Telegram session is idle, send one marker.
   Confirm durable ATM acceptance, typed `PyNudge` receipt, one concise visible
   host-originated Telegram notice, internal-event enqueue, normal existing
   session processing, and one normal agent response.
2. **Busy proof:** start an ordinary Telegram turn in the exact configured
   `ATM_CHAT_ID` session, then send a second unique marker. Confirm it remains
   in Hermes’s per-session queue, does not call steer, does not interrupt the
   active turn, and drains exactly once after the active turn completes. A
   concurrent CLI, cron, or different-chat turn has a different session key
   and is not busy-queue evidence.
3. **Isolation proof:** where a second configured M5 profile is available,
   confirm a marker for profile A cannot appear in profile B. If no second
   live profile exists, run the existing two-profile fixture and record that
   the live test was single-profile only.
4. **Negative checks:** neither proof may perform implicit `atm read`/`atm ack`,
   create a second Hermes/ATM session, create a new listener per nudge, send a
   synthetic external Telegram update, or issue an interrupt.

An accepted queue entry that does not drain, a duplicate response, a missing
visible notice, wrong-profile delivery, or an interrupt is a failure. Do not
hide it with retries, state cleanup, or a hand-written success report.

### F. Defect handling and evidence

- A generic binding/package defect is fixed on the candidate package branch or
  isolated onto a fresh `origin/integrate/phase-al` worktree, then tested and
  reviewed by quality-mgr before a release claim. Early M5 testing may proceed
  immediately after changed candidate wheels build; rerun the affected row
  after review/merge and attach both result sets to the report.
- A Hermes harness defect is fixed and reviewed in Hermes Agent; do not copy
  that fix into `atm-graft` or make the live profile import a private checkout.
- Preserve raw diagnostic material locally. Commit only a redacted report
  under `site/reports/` following the existing report index/navigation scheme.
- The evidence branch may retain reports and M5-only notes. It must not claim
  a passing product result from uncommitted, locally modified, or unreviewed
  package code.

## Required report matrix

| Row | Required evidence | Pass condition |
| --- | --- | --- |
| Runtime pair | selected CLI + daemon and doctor JSON | matched Tokio/Axum pair healthy |
| Active Hermes capability | active-service interpreter/module root and public API probe | documented public runner injection contract available |
| CPython 3.11 package lane | wheel tags, isolated install/import, bridge/runtime tests | no checkout import; both packages usable |
| Harness lifecycle | package import, receiver publication, restart ownership | one live schema-v2 receiver, no stale owner |
| Idle queue delivery | durable marker through Telegram notice and response | exactly one intended session processes it |
| Busy queue delivery | active ordinary turn plus second marker | no steer/interrupt; exactly one later drain |
| Isolation | two-profile live proof or documented fixture | no cross-profile delivery |

## Acceptance criteria

AL.19 begins as soon as its adapter-shape precondition is met. It is complete
only when all of the following are true:

1. M5’s actual CPython 3.11 environment installs and imports the exact
   candidate `atm-graft` and `hermes-atm` wheels without a source-tree
   dependency. Final closure reruns this row on the reviewed candidate.
2. The newer M5 Hermes harness starts the installed runtime and publishes one
   valid, generation-owned receiver for the configured profile.
3. The active M5 Hermes service exposes the documented public runner injection
   contract; otherwise AL.19 remains blocked rather than using a private
   compatibility workaround.
4. Both idle and busy queue probes pass with visible notice and normal response
   in only the intended Telegram session.
5. Busy delivery is demonstrably queue-based: no steer call, no interrupt, no
   duplicate, and no lost queued nudge.
6. Existing bridge/runtime tests, lint, and doctor are green for the frozen
   candidate.
7. The indexed, redacted M5 report includes the complete matrix and separately
   records M5 CPython 3.11 evidence from M4 CPython 3.13/3.14 evidence.
8. Every code defect found during the run is either fixed through its own
   reviewed PR and rerun, or remains an explicit blocker; no local workaround
   is represented as a release-ready pass.

## Non-goals

- Replacing queue with steer, or deciding final product policy between queue
  and steer.
- New transport, storage, replay, retry, fan-out, additional gateway, or TLS
  features.
- Modifying the frozen legacy daemon or using it to make M5 tests pass.
- Treating a CPython 3.13/3.14 result as evidence for CPython 3.11.
