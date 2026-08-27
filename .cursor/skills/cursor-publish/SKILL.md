---
name: cursor-publish
description: >-
  Cursor-native sc-publish release flow. Runs publisher inline with manifest-
  driven preflight and channel verification; no nested Task subagents.
---

# Cursor publish (sc-publish)

Use when releasing a consumer repository that vendored the sc-publish kit via
`install.py`.

## Runtime

| ATM/rmux | Cursor (this skill) |
|----------|---------------------|
| Named teammate + background channel workers | **One foreground publisher**; channel-agent files are **inline playbooks** |

## Prerequisites

After install, the consumer should have:

- `release/publish-artifacts.toml`, `release/publish-channel-contracts.toml`
- `.github/workflows/release-preflight.yml`, `release.yml`, channel workflows
- `.github/scripts/release_artifacts.py`, `.github/scripts/release_gate.sh`
- `.claude/agents/publisher.md` + channel agents
- `.cursor/agents/publisher.md` (this kit)

## Invocation

1. `/cursor-publish` (`.cursor/commands/cursor-publish.md`)
2. Single **foreground** session following `.cursor/agents/publisher.md`
3. Run each channel-agent playbook inline and sequentially; do **not** use
   Multitask background delegation or Task subagents.

## Tool recipes

### Manifest validation

```bash
python3 .github/scripts/release_artifacts.py validate-manifest \
  --manifest release/publish-artifacts.toml \
  --workspace-toml Cargo.toml
python3 .github/scripts/release_artifacts.py preflight-secret-plan \
  --manifest release/publish-artifacts.toml
```

### CI preflight

```bash
gh workflow run release-preflight.yml --ref main \
  -f version="${VERSION}" -f run_by_agent=publisher
gh run watch --exit-status
```

Adjust `--ref` to the branch/commit under release per release-state strategy.

### Channel inquiry (inline — replaces background worker)

```bash
python3 .github/scripts/release_artifacts.py public-registry-inquiry-plan \
  --contracts release/publish-channel-contracts.toml \
  --channel crates_io --name "${CRATE}" --version "${VERSION}"
# curl each URL from plan output; classify per channel-contracts.md
```

### Post-release dispatch plan

```bash
python3 .github/scripts/release_artifacts.py channel-dispatch-plan \
  --manifest release/publish-artifacts.toml --tag "v${VERSION}"
# For each entry: gh workflow run ... ; gh run watch
```

## Assignment snippet

```xml
<cursor-publish-assignment runtime="cursor">
  <mode>preflight|publish|retry</mode>
  <version>X.Y.Z</version>
  <recipient>{{ recipient }}</recipient>
  <constraints>Inline channel steps only; no Task spawns.</constraints>
</cursor-publish-assignment>
```

## Related

- `.claude/skills/publishing/SKILL.md` — ATM/rmux delegation
- `.claude/agents/publisher.md` — shared orchestration policy
