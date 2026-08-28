---
name: publishing
description: Coordinate a manifest-driven software release through a named ATM publisher teammate. Use when preparing release preflight, publishing a release, retrying a failed publish channel, or diagnosing release workflow readiness in the current repository.
---

# Publishing

**Goal: one skill invocation, one human approval.** A release assignment runs
end-to-end with exactly one human gate — approval of the `release/vX.Y.Z` →
`main` PR. Everything before that PR (candidate cut, preflight, full-pipeline
rehearsal of every manifest-declared channel) happens first so the approval is
informed; everything after it (tag, GitHub Release, crates.io, TestPyPI →
production PyPI, Homebrew, winget, Scoop) proceeds autonomously and in
parallel wherever channel dependencies allow. Do not return to the user for
per-channel go-aheads, and do not leave manifest-declared channels
undispatched because the assignment text did not name them: the manifest and
dispatch plan define release completion, not the assignment prose.

Use the named `publisher` ATM teammate for production release work. Do not use
an unnamed background agent and do not create version-specific production
publisher identities. The shared release-state policy is
[`ref/release-state-strategy.md`](ref/release-state-strategy.md); read it
before selecting a branch, preflight location, or publish action.

## Start the publisher

1. Verify the required tools before delegation:

   ```bash
   command -v atm && atm --help
   command -v rmux && rmux --help
   ```

2. Confirm the roster has a named `publisher` teammate. Start one when needed;
   its production identity is exactly `publisher` for either runtime:

   ```bash
   rmux claude publisher --team <team-name> --model <claude-model>
   rmux codex publisher --team <team-name> --model <codex-model>
   ```

   The launch must establish `ATM_TEAM=<team-name>` and
   `ATM_IDENTITY=publisher`. Evaluation runs may use a distinct, clearly
   non-production identity.

3. Send a rendered [`preflight.xml.j2`](preflight.xml.j2) or
   [`publish.xml.j2`](publish.xml.j2) assignment through ATM. Require the
   immediate ACK, milestone status, and fenced JSON completion report from
   `publisher`.

## Channel publishers

The named `publisher` teammate coordinates role-specific background channel
workers inside its own session. `release/publish-channel-contracts.toml`
defines their standard role and contract; [`ref/channel-contracts.md`](ref/channel-contracts.md)
defines its operating procedure. Do not launch them as ATM teammates or tmux
panes, and do not duplicate secret names, registry APIs, or account conventions
in a repository manifest.

Before delegation, `publisher` and each channel worker must read the concise
credential facts in [`ref/channel-contracts.md`](ref/channel-contracts.md).
Credentials are already configured; do not ask for them. `Release Preflight`
is authoritative.

- `crates-io-publisher` — crate name/version inquiry and partial crate retry
- `pypi-publisher` — normalized PyPI/TestPyPI inquiry and rehearsal
- `github-release-publisher` — immutable GitHub Release channel
- `homebrew-publisher`, `winget-publisher`, `scoop-publisher` — their matching
  manifest-declared destination only

Ask `publisher` whether `<name>` is available on a registry; it delegates a
role-specific background worker for the read-only inquiry. The response must distinguish
`apparently_available`, `taken`, and `indeterminate`; a lookup never reserves a
name. Publishing remains gated by `publisher` and Release Preflight.

## Durable evaluations

Run the applicable fresh-context evaluation after changing this skill,
`publisher.md`, the manifest helper, or release workflows. The durable cases
are [`evals/publisher-preflight.md`](evals/publisher-preflight.md) and
[`evals/publisher-recovery.md`](evals/publisher-recovery.md). They use
evaluation-only identities and must never create a production tag or publish.
Also run [`evals/channel-name-inquiry.md`](evals/channel-name-inquiry.md) after
changing a background channel-worker contract or registry inquiry helper.

## Operating rules

- Use the assignment's publishing manifest (normally
  `release/publish-artifacts.toml`) and `.github/scripts/release_artifacts.py` as the
  only repository-specific publish surface.
- Use the vendorable `release/publish-channel-contracts.toml` as the single
  shared channel contract and [`ref/channel-contracts.md`](ref/channel-contracts.md)
  for its operating procedure. Preflight obtains public version/name evidence
  for every declared crate and Python distribution before it authorizes
  publication.
- Complete readiness preflight before a `main` merge and final preflight on
  the exact `main` commit before publishing, as the shared policy requires.
- Under explicit publisher assignment, dispatch `release-candidate.yml` to
  establish `release-candidate-vX.Y.Z` from `develop` before creating the
  release branch. Do not create that tag locally. The final gate requires the
  candidate tag to be an ancestor of `main`, not that `main` and `develop`
  still have identical tips.
- Before each preflight, record the candidate-to-release diff and escalate
  non-trivial implementation or dependency changes to the named coordinator.
  Commits added to `develop` after the candidate cut do not delay the release.
- Treat all publish tokens as already-provisioned GitHub Actions secrets. Do
  not ask whether they exist, request them, inspect them, or substitute local
  credentials.
- Permit retry only for failed structured results. For a partial crates.io
  run, preserve the same tag and release ref; the idempotent manifest job skips
  live crates and retries only the missing crate set.
- Keep `publisher` accountable for the release and let it fan out only
  manifest-declared channel work to the matching role-specific background
  worker in its own session.
- **Full-pipeline rehearsal before the single approval**: before the
  `release → main` PR is put up for approval, rehearse every remaining
  pipeline step of every manifest-declared channel locally against real
  artifacts — verify/select scripts, channel-config and dispatch-plan
  output, template renders with real URLs and checksums plus their syntax
  validators, metadata checks (`twine check`), and uploader semantics from
  pinned sources. Rehearse with the tooling revision each workflow will
  actually check out. **Syntax validation is not runtime validation**: where
  a rendered artifact executes at install time (a Homebrew formula's
  install/test blocks), rehearse the runtime path itself — e.g. a real local
  `brew install` from the rendered formula in a throwaway local tap against
  the published release assets, plus the test-block assertion against the
  real binary (`ruby -c` passed a formula that failed every user's
  `brew install`, v1.4.4 defect 14). Only credentialed operations (uploads,
  tap/bucket pushes, registry PRs) are exempt — never run those locally.
- **On any pipeline failure, batch — never trickle**: do not fix the first
  defect and re-dispatch. Rehearse the entire remaining pipeline, collect
  every defect into one fix round and one `main` merge, and verify the
  re-dispatch will actually execute the fixed tooling (a workflow whose
  checkout is pinned to the immutable tag never picks up fixes).
- **Parallel channel dispatch**: once the immutable GitHub Release is
  verified live, fan out every remaining manifest-declared channel worker
  concurrently. Only real dependencies serialize (TestPyPI rehearsal before
  production PyPI inside the pypi channel). One channel's failure holds that
  channel only; the others proceed.
