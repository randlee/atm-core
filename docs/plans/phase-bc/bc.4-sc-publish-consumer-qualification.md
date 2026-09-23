---
status: historical/superseded; u4 OPEN; QA-1 remediation active
---

# bc.4 sc-publish consumer qualification

The prior receipt below is retained as historical evidence only. It does not
qualify the current consumer layer or close u4.

## provenance

| item | result |
| --- | --- |
| upstream repository | `https://github.com/randlee/sc-publish` |
| historical source SHA (superseded) | `22137c2da13bf4638b4267b69c6c2f021617da73` |
| consumer branch | `feature/bc4-sc-publish-immutable-consumer` |
| consumer base | `5b30535acb8714eea7563b3789ae46b9981c21e7` |
| historical atm-core consumer head | `e6da63ad5c4e936de26fbfcfe3709ec1c9d37949` |
| pin | `release/sc-publish-pin.toml` records the full source SHA |

The old source revision was an accepted open PR head, not a merged revision.
The active merged sc-publish PR99/100/101/108 stack has accepted develop head
`98a75ba3e`; a clean consumer layer must repin and reinstall that result before
u4 closes.

## superseded installer evidence

The kit was exported at the pinned SHA into the isolated checkout
`/tmp/sc-publish-bc4-kit.ZzlAKY`. The canonical bootstrap and installer were
run from that checkout against the exact ATM consumer head; no generated
shared file was hand-edited.

```text
KIT_ROOT=/tmp/sc-publish-bc4-kit.ZzlAKY
CONSUMER=/Users/randlee/github/atm-core-worktrees/fix/bc4-review-1
"$KIT_ROOT/.venv-sc-publish/bin/python" \
  "$KIT_ROOT/plugins/sc-publish/.github/scripts/bootstrap_sc_compose.py" \
  --venv "$KIT_ROOT/.venv-sc-publish"
"$KIT_ROOT/.venv-sc-publish/bin/python" \
  "$KIT_ROOT/plugins/sc-publish/install.py" \
  --input "$CONSUMER/release/sc-publish-consumer-input.json" "$CONSUMER"
"$KIT_ROOT/.venv-sc-publish/bin/python" \
  "$KIT_ROOT/plugins/sc-publish/install.py" --dry-run \
  --input "$CONSUMER/release/sc-publish-consumer-input.json" "$CONSUMER"
```

The prior receipt recorded bootstrap exit 0, installer exit 0, and a repeat
dry-run exit 0 with `Publish-kit assets are in sync.` Those results are
superseded. QA-1 reran the canonical installer against the restored consumer
layer and the dry-run exited 1 because source `22137c2` would overwrite the
restored prerelease and dynamic-version hunks. The drift is the expected
frozen-layer behavior independently resolved as ATM-QA-008/QA-004; it is not
current qualification evidence.

## historical local consumer evidence

The following results belong to the historical ATM head above:

```text
pytest -q .github/scripts/tests
283 passed, 13 skipped, 41 subtests passed; exit 0

pytest -q .github/scripts/tests/test_fail_closed_probes.py \
  .github/scripts/tests/test_preflight_regressions.py \
  .github/scripts/tests/test_release_immutability.py
54 passed; exit 0
```

The focused suite was the immutable/preflight negative suite: mocked disabled,
forbidden, timeout, malformed, indeterminate, stale-state, and policy failure
paths fail closed. It performed no release, registry, channel, tag, or setting
mutation.

The upstream source suite at the historical open head independently reported
`pytest -q plugins/sc-publish/.github/scripts/tests`: `270 passed, 10 skipped;
exit 0`. That is upstream source evidence, not the ATM consumer result above.

## package and release contract inventory

The input declares 16 crate entries in publish order. Orders 0–15 are:

```text
0 atm-graft-python (not published), atm-query-python (not published)
1 atm-error
2 atm-storage
3 peer-tls
4 agent-team-mail-core
5 atm-storage-rusqlite
6 atm-http-runtime
7 atm-daemon-client
8 atm-herdr
9 atm-observability
10 atm-runtime
11 atm-template-sc-compose
12 atm-daemon-bootstrap
13 atm-daemon
14 atm-graft
15 agent-team-mail
```

The two order-0 Python crates are required build inputs; the remaining 14
entries are published, with `atm-graft` optional and the other published
entries required. The release binary inventory is `atm` (bundles
`docs/user-documents` at `share/doc/atm`) and `atm-daemon` (no bundled paths).

Python distributions and wheel runner targets are:

| distribution | source/module | build | targets |
| --- | --- | --- | --- |
| `atm-graft` | `crates/atm-graft-python` / `crates/atm-graft-python/python/atm_graft` | maturin | `ubuntu-latest`, `macos-latest`, `windows-latest` |
| `atm-query` | `crates/atm-query-python` / `crates/atm-query-python` | maturin | `ubuntu-latest`, `macos-latest`, `windows-latest` |
| `hermes-atm` | `crates/hermes-atm` / `crates/hermes-atm/src/hermes_atm` | setuptools | `ubuntu-latest`, `macos-latest`, `windows-latest` |

The expected consumer channel inventory is `pypi` →
`pypi-publish.yml` with production target, `homebrew` →
`homebrew-publish.yml`, `winget` → `winget-publish.yml`, and `scoop` →
`scoop-publish.yml`. The nine kit workflow files are
`crates-publish.yml`, `homebrew-publish.yml`, `npm-publish.yml`,
`pypi-publish.yml`, `release-candidate.yml`, `release-preflight.yml`,
`release.yml`, `scoop-publish.yml`, and `winget-publish.yml`. Existing
consumer-owned `hermes-atm-pypi-publish.yml` and `prerelease-archive.yml` are
not part of the kit inventory.

## inherited and deferred lifecycle boundary

BC4 does not claim FIX03–FIX10. Their exact disposition at the accepted source
head is:

| item | disposition |
| --- | --- |
| FIX03 | deferred; the default renderer remains sc-compose 1.5.0 |
| FIX04 | deferred; no generic preflight preparation/validation hook or opt-out |
| FIX05 | deferred; existing-tag source/build policy not redesigned |
| FIX06 | deferred; no root release-concurrency mechanism |
| FIX07 | inherited PR101 behavior; no new proof of admission before every registry write |
| FIX08 | inherited PR101 behavior; not changed or claimed |
| FIX09 | deferred; no cross-channel draft/attestation lifecycle implemented or live-qualified |
| FIX10 | deferred; no trusted workflow-ref policy implemented |

These are upstream/deferred lifecycle records, not bc.4 completion assertions.
The PR1566 Just lint failure is likewise unresolved compatibility evidence:
its pytests report that `release/publish-artifacts.toml` lacks the consumer
`[prerelease]` table, `sync_python_version` cannot find
`crates/atm-graft-python [project].version`, release/prerelease packaging
parity diverges, and daemon-switch prerelease resolution fails. This correction
does not edit generated/shared assets or claim that failure fixed.

## consumer checks and open disposition

- `release/sc-publish-pin.toml` contains the exact 40-character source SHA.
- The historical receipt recorded byte-for-byte kit output; the QA-1 dry-run
  now reports drift against the restored consumer layer.
- PR #101, PR #106, and the old `22137c2` receipt are retained only as
  historical evidence; the active merged PR99/100/101/108 result is the
  pending source for the next clean consumer layer.
- No repository setting, credential, release, tag, registry, or channel was
  mutated by the historical consumer qualification; the separately reported
  2026-09-22 setting enablement is not bc.5 completion evidence.
- The evidence above is historical local evidence plus recorded upstream source
  evidence. It is not current merged-source, hosted-QA, credentialed, or live
  publication qualification.
- QA-005 branch coverage remains pending artifact-layer validation; no branch
  coverage percentage is claimed.

## disposition

`u4` remains **OPEN**. A clean consumer layer must pin merged develop
`98a75ba3e`, reinstall without drift, and record the resulting local evidence.
Merge status, hosted-QA completion, credential activation, and live
release/channel qualification remain outside this historical receipt; setting
and credential activation belong to separately authorized bc.5 evidence.
