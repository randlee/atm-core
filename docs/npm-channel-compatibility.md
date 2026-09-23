# npm and package-only release compatibility evidence

PR: https://github.com/randlee/sc-publish/pull/95 (draft, base `develop`).
Baseline: `a4206dcf75edd047f57abebdf6e4ed4d096f2543`.
The exact tested candidate commit and GitHub CI run are recorded in the PR body;
this file is part of that candidate and does not claim a merge or qualification.

## Existing working consumers

The comparison uses read-only inputs and installs each kit into independent
scratch directories. Both initial installs and repeat `--dry-run` checks pass.
Parsed rendered artifact manifests and every pre-existing channel contract are
identical. Outputs are identical for `python-wheel-matrix`,
`python-sdist-matrix`, `list-publish-plan`, `preflight-secret-plan`, and
`release-asset-patterns`. The existing installed manifests' wheel matrices are
also compared, including sc-compose's ARM runner addition.

| Consumer | Inspected checkout HEAD | Input SHA256 |
| --- | --- | --- |
| sc-compose | `b763d2ffdaf6941bd8b375dba4e77676e357169f` | `97ec11ab3c8a9cc5ccff4ed69a97727969e80f4a3febc85eb36d53dc53f8ba2f` |
| atm-core | `849405ef76cf44937153215ea6bfe8a0ddc1f739` | `26ca2237a0e46ed14cc6dcd01d60ca1dbc45aad09be01141cfc53726735ac51b` |

Reproduce using the pinned renderer Python and isolated checkouts or archives:

```bash
python .integration/verify_consumer_compatibility.py \
  --baseline-kit /path/to/a4206dc/plugins/sc-publish \
  --candidate-kit /path/to/candidate/plugins/sc-publish \
  --consumer-input /path/to/sc-compose/release/sc-publish-install.json \
  --consumer-input /path/to/atm-core/release/sc-publish-consumer-input.json
```

The inputs' hashes are checked by the review evidence rather than assumed from
checkout HEAD (consumer files may have local edits). The script prints both
identity and comparison receipts. It performs no network or registry operations.

Existing wheels already worked: sc-compose and atm-core use runner strings,
native maturin builds, and the existing Python upload channel. sc-compose's
installed workflow additionally uses `maturin[zig]==1.9.4` and
`--compatibility manylinux2014 --zig` for `ubuntu-24.04-arm`. This change reuses
that mechanism. Explicit target objects add the missing per-architecture Rust
target, manylinux level, macOS deployment target, unique artifact ID, and wheel
platform assertions. Legacy runner-string matrix JSON remains identical.

## Validation

- Source suite: `python -m pytest plugins/sc-publish/.github/scripts/tests -q`:
  166 passed, 10 skipped on Python 3.12 with sc-compose 1.5.0 and PyYAML 6.0.3.
- Installed suite: 162 passed, 14 skipped after a clean installer overlay
  and repeat drift check. Both source and installed suites ran with ambient
  `RELEASE_TAG` removed; mocked publication fixtures set their own tag. The
  installed suite is now also a GitHub CI gate. Source tests cover
  all new npm/wheel modules instead of only historical unittest modules.
- Tests execute actual crate publish shell orchestration with a mocked Cargo
  executable and registry lookup. Both root and retry legs pass the standalone
  manifest path; dependency-aware preflight plans include standalone crates.
- npm tests cover nonpublishing preflight, matching-version skip, conflicting
  integrity rejection, accepted-upload recovery, failed-upload retry, registry
  errors, archive identities/checksums, all-artifact validation before writing,
  credential scoping, and immutable-release download gating.
- Wheel tests cover all five declared platforms, duplicate/missing/wrong
  platforms, legacy runner compatibility, and retained build failure gates.
- A temporary copy of the real TypeScript client built and packed version 1.4.0
  successfully, with `dist/index.js` and `dist/index.d.ts`; tarball SHA256 was
  `1b0c764507b7da0d8ad9dc3a750a2ac1c028c310d0b5270b202b1af7199b3c62`.
  The package was never published.

The baseline has a reproducible macOS Bash 3 failure for empty
`already_published_channels`: the preservation helper expands an unset array.
Run the baseline pytest selector
`-k test_release_preflight_registry_checks_execute_preserved_channel_exception`
to reproduce (1 failed, 2 passed). An empty-input guard now returns false before
array expansion, matching the existing supported Linux/Bash behavior. The full
candidate suite passes on macOS without excluding or waiving that test.

## Action runtime evidence and limits

Official action metadata/release notes identify the Node24 versions used:
[checkout v5](https://github.com/actions/checkout/blob/v5/action.yml),
[setup-python v6](https://github.com/actions/setup-python/blob/v6/action.yml),
[setup-node v6](https://github.com/actions/setup-node/blob/v6/action.yml),
[cache v5](https://github.com/actions/cache),
[upload-artifact v6](https://github.com/actions/upload-artifact/releases/tag/v6.0.0),
[download-artifact v7](https://github.com/actions/download-artifact/blob/v7/action.yml),
and [action-gh-release v3](https://github.com/softprops/action-gh-release/blob/master/CHANGELOG.md).
Existing composite action pins are retained.

No release tag, registry write, real publish dispatch, credential modification,
or native five-platform build was performed. The tests establish configuration,
planning, build command, archive verification, and retry behavior; they do not
replace live release qualification or the consumer's 25-cell wheel import gate.
The npm workflow requires GitHub immutable releases to be enabled and a valid
`NPM_TOKEN` in the `npm` environment; this work does not provision either.

## Consumer test portability correction

An independent sc-observability install exposed two test assumptions: scanning
caller-owned workflows applied the kit runtime floor to unrelated actions, and
Python README checks assumed `bindings/python/pyproject.toml`. The runtime test
now selects only shared workflows/composites; README checks follow declared
Python and Cargo manifests and validate optional file references where present.
A nested-package regression covers absent README metadata, a declared local
README, and a missing declared file.

An isolated copy of sc-observability at
`d75b54e10b7af8e7899aa3891c9c901ac6949140`, overlaid through the installer, passed
its full installed suite: **157 passed, 11 skipped**, with a clean repeat dry-run.
Source suite: **158 passed, 10 skipped**. Both retained peer compatibility
comparisons pass unchanged. This correction changes shared tests/documentation,
not publication behavior or consumer-owned workflows.

## Pre-tag npm source gate

Review identified that npm source versions were checked only during the later
build job. `verify-version-lockstep` now validates every declared npm source
name/version, rejects private packages, and enforces the same public registry
and publishConfig policy used for archived packages. The build reuses this
validator. Before the tag step, a separate failing gate reads the declared npm
manifest and each package.json from the exact resolved release commit, so a
valid dispatch checkout cannot conceal stale metadata in the tree being tagged.

Regression tests reject stale versions, mismatched names, private packages,
restricted access, and registry overrides. They assert exact-commit reads and
workflow ordering before tag creation and downstream build/publication jobs.
Full source: **166 passed, 10 skipped**; generic installed: **162 passed,
14 skipped**; isolated actual consumer: **165 passed, 11 skipped**. The existing
sc-compose and atm-core compatibility comparison remains unchanged. No
credential convention, registry write, tag, or live dispatch is part of this fix.

## PHC-QA-013: bounded subprocess tests

An AST audit found all 26 `subprocess.run` calls in
`test_release_artifacts.py` and all six in `test_publish_kit_scripts.py` lacked
a timeout. Each now passes an explicit 30-second timeout. These commands run
local fixture Git operations, CLI helpers, shell checks, and mocked registry
flows; they do not perform real builds or publication. Python's native
`TimeoutExpired` identifies the command and deadline and kills/reaps the direct
child. This is not a portable descendant-process-tree termination guarantee.

The upstream CI job also has an explicit 15-minute aggregate deadline. Previously
it inherited GitHub's [360-minute default job limit](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idtimeout-minutes);
local runs had no command deadline. Consumer-owned CI limits remain caller-owned.

Three regressions substitute a real sleeping child into the artifact CLI,
artifact Git, and kit Git helpers, reduce the deadline to 0.2 seconds, and verify
the reported timeout/command and that the child has been reaped. They preserve
the helper's options and fail immediately if its timeout is missing.

Validation (ambient `RELEASE_TAG` removed, Bash 5.3): full source **169 passed,
10 skipped**; generic installed **165 passed, 14 skipped**; isolated
sc-observability installed **168 passed, 11 skipped**. The two changed test files
pass **85 passed, 10 skipped**. Peer compatibility checks against the pinned
develop baseline retain identical sc-compose and atm-core manifests, contracts,
matrices, plans, and assets. No release behavior or authentication was changed.

Reproduce the focused and full runs with the renderer environment provisioned:

```sh
python -m pytest plugins/sc-publish/.github/scripts/tests/test_release_artifacts.py plugins/sc-publish/.github/scripts/tests/test_publish_kit_scripts.py -q
python -m pytest plugins/sc-publish/.github/scripts/tests -q
```

## PHC-QA-017/018: actual nonpublishing preflight corrections

Consumer run [35415396742](https://github.com/randlee/sc-observability/actions/runs/35415396742)
exposed a missing crates.io credential probe and Cargo's registry resolution
while generating an archive lockfile. Both were reproduced independently.

The liveness step now wires the contract-declared `CARGO_REGISTRY_TOKEN` and
uses `release_credentials.py` for manifest-authorized read-only probes. Every
failure, including unsupported checks, records its channel outcome. The helper
bounds requests to 20 seconds, refuses redirects/unexpected endpoints, and
never prints credentials or service response bodies.

The existing crates.io contract names `/api/v1/me`, which is now cookie-only.
Official crates.io [AuthCheck implementation](https://github.com/rust-lang/crates.io/blob/6b53ae9cebcfcdc19c81ff9a87664959a2285914/src/auth.rs)
authenticates first and only then returns its specific cookie-only diagnostic;
the [endpoint implementation](https://github.com/rust-lang/crates.io/blob/6b53ae9cebcfcdc19c81ff9a87664959a2285914/src/controllers/user/me.rs)
uses that check. We accept a valid user identity or that exact HTTP 403 JSON
response as evidence of authentication only. Other 403 responses, authentication
failures, malformed responses, redirects, and transport errors fail closed.
The output explicitly does **not** establish crate ownership or publish scope.
A future change to the service diagnostic will fail closed and require review.
No live token probe was performed: all HTTP responses were synthetic.

Cargo's [`--no-verify`](https://doc.rust-lang.org/cargo/commands/cargo-package.html)
skips the archive build, not registry resolution for its lockfile. For dependent
release crates only, preflight now runs `cargo check --locked` against the real
manifest and packages a normalized source archive with `--no-verify
--exclude-lockfile`. That archive is never published. Independent crates retain
full archive verification, and publication retains normal lockfile generation
and registry verification. Plan-generation errors now stop the step instead of
being hidden by process substitution.

A real fixture with two workspace crates and a standalone crate reproduces the
old failure with an isolated Cargo home and offline mode. The corrected workflow
creates both dependent archives with normalized versioned dependencies; the
independent crate still undergoes archive build verification. Compiler errors,
missing source files, and invalid release plans fail. Tested with the default
Cargo **1.94.1**; the packaging flag also exists in Cargo **1.93.0**.

Validation: new regressions **25 passed**; full source **194 passed, 10 skipped**;
generic installed **190 passed, 14 skipped**; isolated actual sc-observability
installed **193 passed, 11 skipped**. Existing sc-compose and atm-core
baseline/candidate comparisons remain identical for manifests, existing channel
contracts, matrices, publish plans, credential plans, and release assets.
No credential was read, publication attempted, or release workflow dispatched.
