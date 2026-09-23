# sc-publish Publish Kit

sc-publish is a vendorable release/publish kit: standardized publisher agents
plus standardized per-channel GitHub workflows, all driven by one
repository-specific release manifest. Every kit file is installed
**byte-for-byte** into the consumer repository — copied files are never
hand-edited. If an installed file looks wrong for your repository, the fix is
either your consumer input JSON (which drives the two rendered manifests) or
an issue/PR against the upstream kit; local drift is a defect, not a
customization mechanism.

> In consumer repositories this document is installed as
> `README.sc-publish.md` so it never overwrites the repository's own README.

## The install contract

Installation is three commands, run from the consumer repository root:

```bash
# 1. Provision the exact pinned sc-compose 1.5.0 renderer wheel into a virtualenv.
python plugins/sc-publish/.github/scripts/bootstrap_sc_compose.py --venv <venv>

# 2. Install: copy every kit file byte-for-byte and render the two release
#    manifests from your complete, caller-owned consumer input JSON.
<venv>/bin/python plugins/sc-publish/install.py --input <consumer-input.json> <repo>

# 3. Verify: a repeat dry-run must report no drift (exit 0).
<venv>/bin/python plugins/sc-publish/install.py --dry-run --input <consumer-input.json> <repo>
```

The consumer input JSON is the single reviewable declaration of everything
repository-specific: project identity, release targets, crates, release
binaries, Python distributions, and the post-release channels the repository
actually uses. Only two files are rendered from it —
`release/publish-artifacts.toml` and `release/publish-channel-contracts.toml`;
everything else is a shared verbatim copy. Re-running the installer after a
kit upgrade re-synchronizes the copies; `--dry-run` exits 1 and prints a diff
whenever a consumer file differs from the kit.

## Pinning, bootstrap, and qualification

Consumers never track a moving sc-publish branch. The org model is one
blessed, qualified kit revision that every consumer repository adopts
together:

- **Pin manifest.** Each consumer records the qualified upstream revision in
  `release/sc-publish-pin.toml` (full 40-character SHA; see the installed
  `release/sc-publish-pin.toml.example`). The pin is the only statement of
  which kit a repository runs; a consumer tree that differs from its pin is
  drift.
- **Isolated bootstrap.** To install or re-sync, clone the kit **at the
  pinned revision** into a repository-local, gitignored cache (for example
  `.sc-publish-kit/`) and run the install contract from that clone. Never
  run the installer from a shared mutable checkout (such as a sibling
  `../sc-publish` working tree used by other repositories or agents) — a
  branch switch there silently changes what every consumer installs.
- **Consumer input path.** The `--input` JSON path is the consumer's choice —
  `release/sc-publish-consumer-input.json` and `release/install.json` are
  both in use across consumers. The filename is not part of the kit
  contract; only the schema is.
- **Qualification before a pin advance.** A new kit revision is blessed for
  the org only after, on at least one real consumer repository: a clean
  install from the candidate revision, a clean repeat `--dry-run` (exit 0,
  no drift), a passing kit test suite
  (`pytest .github/scripts/tests/` with the pinned renderer), a live
  release-candidate tag cut, and one post-release channel leg retry.
  Record the evidence in a receipt, then advance every consumer's pin to the
  blessed SHA together.

## Runtime profiles

The kit ships two publisher runtime profiles that share the same manifests,
scripts, and workflows:

- **Claude/Codex sessions** run the publisher as a named ATM teammate. The
  agent definition is `.claude/agents/publisher.md`, launched through the
  publishing skill (`.claude/skills/publishing/SKILL.md`). It is a full ATM
  team member (`ATM_TEAM`/`ATM_IDENTITY`) and reports channel blockers to its
  assignment's named recipient.
- **The Cursor IDE** runs the publisher inline via `.cursor/` (agent,
  command, and skill). It performs the channel steps in-session rather than
  through ATM teammates.

The Claude/Codex publisher spawns the role-specific background channel workers
(`crates-io-publisher`, `github-release-publisher`, `pypi-publisher`, `npm-publisher`,
`homebrew-publisher`, `scoop-publisher`, `winget-publisher`). Cursor executes
the same channel playbooks inline and sequentially. Both profiles consume the
same non-disclosing credential preflight before any publication.

## GitHub prereleases

Consumers may opt in by declaring `[prerelease]` in
`release/publish-artifacts.toml`. The installed `prerelease` skill is
registered under both `.claude/skills/` and `.codex/skills/`, with `--list`,
`--install`, and `--publish` modes (`--create` remains a compatibility alias).
Publishing creates only GitHub prerelease archive pairs: it tags an explicitly
authorized, clean non-protected branch,
waits for `prerelease-archive.yml`, verifies every Release asset and checksum,
and prints the Release URL. It never publishes to the normal package or
channel registries. Install resolves a selected or latest matching
prerelease, checksum-verifies the host archive, stages it under the manifest
install root, updates the platform selector, and invokes the declared
post-install and verification commands. Dry runs do not make network calls.

## The channel model

Each publish channel — `github_release`, `crates_io`, `pypi`, `npm`, `homebrew`,
`scoop`, `winget` — is a separate, idempotent leg:

- Root legs run inside `release.yml` (build, crates.io publication, GitHub
  Release creation). Post-release legs are standalone `workflow_dispatch`
  workflows (`crates-publish.yml`, `pypi-publish.yml`, `npm-publish.yml`, `homebrew-publish.yml`,
  `scoop-publish.yml`, `winget-publish.yml`) anchored on the already-published
  GitHub Release for a tag.
- Every leg detects already-published state and skips instead of
  republishing, so a failed leg is independently retryable **by tag** without
  touching the channels that already succeeded.
- Channel identity, standardized secret names, and public registry endpoints
  come from the vendored `release/publish-channel-contracts.toml`; the
  repository-specific destinations come from `release/publish-artifacts.toml`.
- The post-release workflows check out the release tag's tree for kit
  actions/scripts and release config, so the tag must have been created
  **after** the kit was installed in the consumer repository. Re-publishing a
  pre-kit tag is unsupported; cut a new release from a kit-installed tree
  instead.

## Where to look next

- `.claude/skills/publishing/ref/publish-kit-requirements.md` — normative
  requirements for the kit.
- `.claude/skills/publishing/ref/channel-contracts.md` — per-channel worker
  contracts and inquiry protocol.
- `.claude/skills/publishing/ref/release-state-strategy.md` — release state
  machine (develop → release candidate → release → main), provenance gate,
  and post-cut drift handling.

## npm packages

Declare `npm_packages` (optional, defaults to empty) together with `channels.npm`:

```json
{
  "npm_packages": [{"name": "@example/client", "source": "bindings/typescript"}],
  "channels": {
    "npm": {"workflow": "npm-publish.yml", "dispatch_inputs": {"dry_run": "false"}}
  }
}
```

Merge this fragment into the complete installer input. Do not declare npm as an
`enabled` flag. Each source must have a committed `package-lock.json` and public
`package.json` whose name and version match the release tag (without `v`). The
pre-tag lockstep gate validates every declared npm package name/version and
public publication settings; an additional gate reads the exact resolved release
commit before creating its tag. The release build runs `npm ci`, `npm run build --if-present`, and `npm pack
--ignore-scripts`; commit any generated inputs needed for the build. The release
attaches the resulting `.tgz` files and includes them in `checksums.txt`.

Enable GitHub immutable releases before publishing through npm; this channel
fails closed unless the GitHub Release API confirms `immutable=true`. It does
not enable repository settings itself. The publication workflow checks out the
tag, downloads archives, validates every package identity/version and SHA256,
and queries the public npm registry before writing. It never rebuilds archives.
`NPM_TOKEN` is scoped to the GitHub `npm` environment and supplied only to the
publication step. No OIDC permission or repository-scoped npm secret is needed.
Read-only preflight cannot establish token validity or package write permission.

Dispatch `npm-publish.yml` with `dry_run=true` (the default) for a nonpublishing
preflight. Authorized publication uses `dry_run=false`. Existing versions skip
only when registry SHA512 integrity matches the release bytes; mismatches fail.
A failed upload rechecks the registry for an accepted identical archive, and an
unresolved failure is retried with the same tag. Stable releases use the npm
`latest` tag and prereleases use `next`; retry does not move an already-published
version's dist-tag. Recovery of dist-tags is a separate authorized operation.

Shared JavaScript action runtime floors are checkout v5, setup-python v6,
setup-node v6, cache v5, upload-artifact v6, download-artifact v7, and
softprops/action-gh-release v3 (Node24; runner >=2.327.1). Composite Rust/tool
installation actions retain their existing pins. These action upgrades and npm
mocks do not constitute the live release qualification required above.

## Explicit wheel platforms and standalone crates

Existing `python_distributions[].wheels` runner strings retain their matrix and
native maturin/setuptools behavior. The proven sc-compose ARM runner path uses
`maturin[zig]==1.9.4` with `--compatibility manylinux2014 --zig`.
For explicit maturin platforms, each wheel may instead be an object:

```json
{"id":"linux-arm64","os":"ubuntu-24.04-arm","target":"aarch64-unknown-linux-gnu","platform":"manylinux_2_28_aarch64","manylinux":"2_28"}
```

`id` uniquely names the uploaded build artifact; `os` is the runner label,
`target` is the Rust target triple, and `platform` is the expected wheel tag.
Explicit Linux wheels reuse maturin's Zig support with the selected manylinux
compatibility. macOS objects require `deployment_target` (for example `10.13`
with `x86_64-apple-darwin` / `macosx_10_13_x86_64`, or `11.0` with
`aarch64-apple-darwin` / `macosx_11_0_arm64`). Windows uses
`x86_64-pc-windows-msvc` / `win_amd64`. Explicit platform objects require maturin;
setuptools keeps the existing runner strings. Build output and collected release
assets must match every declared platform exactly once.

Crates with their own `[workspace]` may appear in `crates` using an explicit
`cargo_toml`; their version must equal the release workspace version. Both
Cargo package checks and publish jobs use `--manifest-path`. The old plan CLI
output remains unchanged; shared workflows request the additional manifest
column with `--include-manifest`.

`release_binaries: []` is supported for package-only releases. The binary build
and binary archive expectations are skipped; declared Python and npm build
failures still prevent release creation. Rust crate and Python distribution
artifact IDs must remain distinct even when they share a source crate.

### Optional immutable sc-lint source installation

The shared `setup-sc-lint` action retains its released `0.4.0` default. Consumers
can opt into a reviewed source commit through `project.sc_lint_source_revision`
in `install.json` (rendered into `release/publish-artifacts.toml`), or the action's
`source-revision` input. `setup-lint-toolchain` forwards `sc-lint-source-revision`.
Use a full lowercase 40-character commit SHA; tags, branches, shortened hashes,
and conflicting action/manifest pins fail. The release `version` input applies
only when source mode is absent. Default release resolution adds no Python
requirement and does not invoke the source installer.

Source mode requires Git, Cargo/the checked source's Rust toolchain, and Python
3.11+ with venv/pip. It verifies the exact checkout, builds all four binaries
and a locked maturin wheel from that checkout, then installs the wheel into a
fresh consumer `.sc-lint/venv`. Python helpers use the current package layout;
source mode never copies historical `.just` helpers or obtains sc-lint from PyPI.
Build commands have a 30-minute limit, setup/fetch/pip commands a five-minute
limit, and version probes a 30-second limit; timeout errors identify the command
and never select a fallback. Maturin is pinned to 1.9.4; declared third-party Python dependencies may be fetched
from the configured package index. CLI/backend/Python crate versions must agree
with Cargo metadata; both installed CLI and Python versions are checked.

Existing consumer venvs are rejected instead of mixing revisions. The action
records commit, version and binary/wheel SHA256 values in
`.sc-lint/source-install.json`, sets `SC_LINT_BIN`, and adds the matching sibling
binaries to PATH. A source-installed consumer cannot silently switch back to
release mode. Use a fresh CI checkout/environment for each installation.

Both modes retain the existing root-discovery/backend-execution smoke contract.
Source mode prints its full JSON report and lint status: successful execution
can still report lint findings. This installer does not change caller lint
policy or claim that an `ok:true` report with `data.status=fail` is clean.

## Immutable release prerequisite and rollout

New publication requires the repository's immutable releases setting to be
verifiably enabled. Shared Release Preflight and Release fail closed when it
is disabled, inaccessible, or indeterminate. Release checks before tag or
registry publication, rechecks before asset upload, and requires the published
release's `immutable` field to be exactly `true` before its release job succeeds
and downstream publication may proceed. Existing published mutable releases
are unsupported by this pipeline; automatic conversion is not implemented.
Draft releases may resume asset upload. Complete immutable releases may be
reused; missing assets require a new version, and `replace_release_assets=true`
is rejected. No setting, release, asset, or tag is changed by the checker.

**Credential limitation: adoption is pending credential design and QA.** GitHub's
[immutable-releases endpoint](https://docs.github.com/en/rest/repos/repos#check-if-immutable-releases-are-enabled-for-a-repository)
requires repository **Administration (read)**. Stock Actions `GITHUB_TOKEN`
cannot request this permission in workflow YAML. The shared workflows currently
use that existing token and therefore block when the endpoint is inaccessible;
this PR does not provision a new secret or claim a stock-token rollout works.
An existing immutable release proves only its own state, not today's repository
setting. A local saved boolean or prior admin check cannot authorize a later
workflow run. HTTP 404 is reported as disabled-or-inaccessible/indeterminate,
because GitHub may mask permission failures. Explicit `enabled:false` is disabled.

Publisher/admin bootstrap is a separate, authorized setup operation:

1. An administrator enables **immutable releases** in repository Settings →
   General → Releases (or through GitHub's separately authorized administration
   API). Never disable it to retry a release. The installer and checker do not
   perform this operation.
2. With an existing suitably authorized GitHub CLI session, run this read-only
   check from the installed consumer: `python3 .github/scripts/release_immutability.py
   --repository OWNER/REPO --tag v1.2.3`. Use the actual candidate
   tag. It emits only sanitized state and exits nonzero on failure. No token
   value should be included in commands, logs, reports, or manifests.
3. Resolve the workflow credential design before adoption: the runtime must be
   able to perform the same fresh administration-read check. Any separately
   approved narrow GitHub App/credential integration belongs in a reviewed
   follow-up, not a stale attestation or an exemption from this prerequisite.
4. Publish future releases with all assets staged before finalization (the
   shared `softprops/action-gh-release@v3` path uploads before publishing).
   Run the checker with `--finalized` for recovery/downstream admission. Historical
   mutable releases remain historical; use a new version when immutability is
   required. Never delete/recreate a release or move its tag to convert it.

Read-only inventory supplied by the rollout lead on 2026-09-20 found
`enabled:false,enforced_by_owner:false` on all six repositories below. This is
an observation, not evidence of enablement or a settings change:

| Repository | Existing installation evidence | Adoption route |
| --- | --- | --- |
| atm-core | `release/sc-publish-pin.toml` at `25668ecc…`; mixed assets documented, update tracked in #1486 | Resolve tracked drift, regenerate with its actual input |
| wyvern | Flat `release/sc-publish-pin.toml` at `25668ecc…` | Preserve flat pin layout; regenerate actual input |
| sc-observability | Pin at `232c695…` | Reviewed full SHA advance and installer regeneration |
| sc-compose | Vendors `plugins/sc-publish`; `release/sc-publish-install.json`, `README.sc-publish.md` | Follow vendored layout; absence of standard pin path is not absence of installation |
| sc-lint | Installed manifest and preflight workflow; no standard pin path found | sc-lint team owns adoption; provide instructions only |
| sc-publish | Shared kit source | Qualify source and its own publication setup separately |

Rollout checklist, per consumer (not performed by this change):

- Verify settings freshly with the administrator and resolve runtime read access.
- Record a reviewed immutable full upstream commit in the consumer's actual pin
  or vendored-source mechanism; use an isolated checkout of that commit.
- Regenerate every installer-managed asset using the existing consumer input;
  repeat `--dry-run` and require zero drift. Do not fork shared workflow files.
- Run installed tests and independent QA, including disabled, API-denied,
  existing-mutable, absent/draft, and finalized-immutable scenarios.
- Run real nonpublishing Release Preflight with the intended runtime identity.
  Require a successful immutability check, not merely an admin's earlier result.
- Review the consumer PR before adoption. Publication is a separate authorization;
  verify final `immutable:true` before authorizing post-release channels.
