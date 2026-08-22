# ADR-053 — Typed Temporary Daemon-Service Launch Overlay

| Field | Value |
| --- | --- |
| ID | ADR-053 |
| Status | Accepted |
| Scope | `daemon-switch` managed service control plane |
| Relates to | ADR-026, ADR-047, ADR-052, `REQ-P-DAEMON-SWITCH-001`, `REQ-P-BENCHMARK-001`, `REQ-CORE-TRANSPORT-002B1` |

## Context

ADR-047 makes peer-wire security a typed, process-local daemon launch policy.
The normal daemon process is mutual TLS; only the explicit
`--peer-wire-security plaintext-test` argument may launch a bounded diagnostic
or benchmark process in plaintext.  The setting is intentionally not durable,
not environment-driven, and never a fallback from mTLS.

Physical benchmarks also require the selected matched CLI/daemon release pair,
the ordinary account-scoped `HostRuntimeScope`, and one managed Tokio/Axum
`atm-http-runtime` daemon.  They must not use a child daemon or a second
endpoint.  The existing `daemon-switch` owns paired release switching and
ordinary service lifecycle but did not own a cross-platform temporary launch
argument transaction.  A harness-local plist edit/direct child workaround
would split lifecycle ownership and cannot safely cover macOS LaunchAgents,
Windows SCM services, and systemd user units.

The temporary service configuration must survive an interrupted operation
without turning a normal process restart into durable plaintext.  It must also
not repeat the primary-state incident pattern: recovery material is not
permission to guess, overwrite an operator change, or mutate an interactive
account's database.

## Decision

`daemon-switch` exclusively owns a narrow, typed temporary-launch session for
the already selected ATM CLI/daemon pair.  A session may select only
`mutual-tls` or `plaintext-test`; it never accepts a raw daemon argument,
alternate endpoint/root, environment selector, service wrapper, or arbitrary
configuration edit.

Before service mutation, the session validates the selected matched pair and
the explicitly named managed service, captures the accepted original launch
specification, and atomically persists an owner-only recovery journal.  The
journal records the session ID, platform/service identity, selected pair
identity, requested mode, original and overlay configuration digests, and
transaction phase.  Private keys, certificate contents, raw trust records, and
other secrets are never emitted in evidence; evidence records redacted metadata
and digests only.

The lifecycle transaction is:

```text
capture -> durable journal -> verified stop -> apply typed overlay
       -> paired start + doctor proof -> optional quiesce/restart
       -> verified stop -> exact original restoration
       -> paired normal start + doctor proof -> complete
```

An active/incomplete journal blocks normal `daemon-switch switch`, ordinary
restart, and a second temporary session.  Recovery is an explicit command,
not an automatic guess: it validates the retained session and expected
configuration before restoring the exact captured original launch
specification.  Mismatch, ambiguity, missing recovery material, a different
selected pair, or an unexpected daemon causes a typed fail-closed result.  The
tool preserves diagnostic/recovery material and leaves the daemon stopped when
it cannot prove a safe restoration.

Platform adapters provide the same contract while preserving native ownership:

- **macOS:** read/hash the controlled source LaunchAgent plist, write an owned
  overlay copy with one typed `ProgramArguments` addition, bootstrap that
  overlay, and bootstrap the untouched original plist on restore.
- **Windows:** capture the named SCM service's exact `BINARY_PATH_NAME`, use a
  round-trip-tested Windows argv codec to form an owned typed overlay, and
  restore the exact captured scalar through SCM.
- **Linux:** accept only an unambiguous systemd `--user` unit command shape,
  install an owned drop-in that resets/replaces `ExecStart` with one typed
  addition, and remove only that owned drop-in on restoration.

Unsupported service shapes fail before stop/mutation.  There is no direct
process fallback and no durable plaintext configuration.  After a successful
restore, the normal managed service starts without the temporary argument and
doctor proves the normal mTLS/default mode.

The feature is control-plane-only.  It does not modify `atm-http-runtime`, the
HTTP resource/router, `WriteRequest`, persistence, TLS stream adapter,
post-write scheduling, message protocol, or timed admission profile.  It
launches only the Tokio/Axum daemon target; the frozen synchronous daemon is
not a candidate implementation surface.  Existing paired-selector and macOS
Apple Development signing checks remain mandatory before lifecycle mutation.

## Consequences

- AO2.5.4 can request one reviewed service-mode session rather than owning
  platform files or child processes.
- Plaintext benchmark setup remains visibly temporary and recoverable, while
  normal daemon launch remains mTLS.
- A crash is diagnosable and safe to recover; it cannot silently replace an
  operator's current service configuration with a synthesized default.
- macOS, Windows, and Linux share product semantics but may reject a service
  configuration that their narrow adapter cannot preserve losslessly.
- Lifecycle/setup work stays outside benchmark samples, preserving the
  plaintext admission pipeline and its performance evidence.

## Rejected alternatives

1. **Let the benchmark harness start an `atm-daemon` child.** Rejected: it
   creates a second lifecycle owner/daemon path and evades the paired release
   proof.
2. **Use an environment variable for wire mode.** Rejected by ADR-047:
   environment selection is non-auditable and can make plaintext durable by
   accident.
3. **Expose generic `--daemon-arg` or arbitrary service edits.** Rejected:
   callers could change unrelated security or runtime semantics, and exact
   recovery would no longer be tractable.
4. **Rewrite the original plist/unit in place.** Rejected: interruption or an
   operator edit can destroy the only known-good service specification.
5. **Automatically repair an incomplete session.** Rejected: a journal proves
   what was captured, not that a later state is safe to overwrite.  Explicit
   recovery makes the mutation visible and fail-closed.
6. **Put the mode branch in the daemon admission path.** Rejected: ADR-047
   requires stream-boundary selection so plaintext retains the original hot
   path when TLS is compiled in.

## Required evidence

- Unit/failure-injection tests prove journal-before-mutation, exact restoration,
  and refusal of a second session/normal mutation while recovery is pending.
- macOS plist, Windows argv/SCM, and Linux unit/drop-in contract tests reject
  malformed, ambiguous, duplicate-mode, and changed-source configurations
  before service mutation.
- Each supported operating system has a real managed-service proof using the
  selected signed release pair: mTLS begin/restore and plaintext-test
  begin/restore both pass doctor, curl receiver, CLI loopback, and same-host
  send/read checks.  Windows and Linux are not considered complete on fake
  boundaries alone.
- Architecture evidence proves this feature is absent from `atm-http-runtime`,
  router, persistence, direct-peer connector, and benchmark `run_profile`.
- Session evidence records selected pair identity, adapter, mode, phase timing,
  redacted configuration digests, and recovery disposition.  It records no
  primary-database mutation.
