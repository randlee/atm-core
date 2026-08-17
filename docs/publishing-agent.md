# Manifest-Driven Publishing Guide

This is the vendorable operator guide for the checked-out repository. Its
repository-specific release surface is declared only in
`release/publish-artifacts.toml`.

## Vendor Contract

Copy the release workflows, composite actions, helper scripts/templates,
publisher and role-specific background channel-worker prompts, this guide,
`release/publish-channel-contracts.toml`, and
`release/publish-artifacts.toml`. A consuming repository changes only its
artifact manifest: crates, binaries, targets, Python distributions, destination
repositories, artifact paths, and channel dispatch inputs must not require
workflow or prompt edits.

Validate the result with:

```bash
python3 scripts/release_artifacts.py validate-manifest \
  --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml
```

## Credential Preflight

All publish credentials are already provisioned as standardized GitHub Actions
repository or environment secrets. Agents never ask whether they exist,
request/re-enter them, inspect them, or log them.

`Release Preflight` is the mandatory authority that permits the root release
workflow to start. It reads the manifest-derived credential plan and:

- checks required repository-secret presence without printing values;
- authenticates required crates.io and GitHub-destination credentials without
  printing values; and
- checks PyPI/TestPyPI protected-environment secret metadata without binding
  preflight to an approval-gated environment.

It is fail-closed, not fail-fast: it completes every independent check and
returns a sanitized result for every root and post-release channel. Checks that
depend on an earlier failed prerequisite are marked `blocked` with that
dependency. A denied overall verdict never authorizes publication.
The workflow publishes those results through its
`channel_preflight_results` job output; `publisher` passes each matching entry
to the role-specific background channel worker that owns that channel. Each
result is bound to the normalized release tag and includes global release
authorization alongside its channel-specific credential checks, so a worker
can reject stale or mismatched evidence.

If preflight reports a missing or rejected credential, report its channel and
sanitized diagnostic to `team-lead`; do not attempt a local workaround. The
environment metadata check confirms name/existence only. A channel whose
credential cannot be proven live by metadata declares
`credential_rehearsal_inputs` in the manifest; its worker completes that safe
channel rehearsal before production closeout without exposing the credential.

## Parallel Channel Dispatch

The root workflow keeps crates.io and GitHub Release work in separate jobs, so
their results are independently monitored and recorded. After it has produced
the immutable GitHub Release, obtain the external post-release work set from
the manifest:

```bash
python3 scripts/release_artifacts.py channel-dispatch-plan \
  --manifest release/publish-artifacts.toml --tag v<VERSION>
```

Publisher dispatches the role-specific background channel worker specified by
each returned channel in parallel inside its own session. Each worker receives its manifest-derived
preflight contract and completed sanitized preflight result before it owns only
its channel's workflow, inputs, verification, optional credential rehearsal,
and structured result. A completed channel is not rerun: retry only the channel
results that failed, using the same release tag. The root crates.io and GitHub
Release jobs follow the same per-channel gate before they start.

Every channel worker returns:

```json
{
  "channel": "<manifest channel>",
  "workflow": "<manifest workflow>",
  "inputs": {"tag": "v<VERSION>"},
  "dispatch_run_id": "<GitHub run id>",
  "status": "passed|failed",
  "verification": ["<channel-specific fact>"],
  "sanitized_diagnostic": "<empty or non-secret failure text>"
}
```

Do not rerun the root release workflow, recreate a tag, rebuild artifacts, or
replay a passed channel to recover a different channel.

For PyPI, metadata presence is not enough to prove publication readiness. The
production worker requires the manifest-declared TestPyPI rehearsal result for
the same candidate artifacts; a matching download/install and artifact digest
is the required server-side evidence. An existing TestPyPI version whose
artifact identity cannot be confirmed is a blocked channel, not a reason to
skip the rehearsal.
