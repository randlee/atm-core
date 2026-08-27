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
# 1. Provision the pinned sc-compose renderer bindings into a virtualenv.
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
(`crates-io-publisher`, `github-release-publisher`, `pypi-publisher`,
`homebrew-publisher`, `scoop-publisher`, `winget-publisher`). Cursor executes
the same channel playbooks inline and sequentially. Both profiles consume the
same non-disclosing credential preflight before any publication.

## The channel model

Each publish channel — `github_release`, `crates_io`, `pypi`, `homebrew`,
`scoop`, `winget` — is a separate, idempotent leg:

- Root legs run inside `release.yml` (build, crates.io publication, GitHub
  Release creation). Post-release legs are standalone `workflow_dispatch`
  workflows (`crates-publish.yml`, `pypi-publish.yml`, `homebrew-publish.yml`,
  `scoop-publish.yml`, `winget-publish.yml`) anchored on the already-published
  GitHub Release for a tag.
- Every leg detects already-published state and skips instead of
  republishing, so a failed leg is independently retryable **by tag** without
  touching the channels that already succeeded.
- Channel identity, standardized secret names, and public registry endpoints
  come from the vendored `release/publish-channel-contracts.toml`; the
  repository-specific destinations come from `release/publish-artifacts.toml`.

## Where to look next

- `.claude/skills/publishing/ref/publish-kit-requirements.md` — normative
  requirements for the kit.
- `.claude/skills/publishing/ref/channel-contracts.md` — per-channel worker
  contracts and inquiry protocol.
- `.claude/skills/publishing/ref/release-state-strategy.md` — release state
  machine (develop → release candidate → release → main), provenance gate,
  and post-cut drift handling.
