# /cursor-publish

Run **sc-publish** release preflight or publish in this Cursor session.

## Mandatory

1. Read `.cursor/skills/cursor-publish/SKILL.md`.
2. Read `.cursor/agents/publisher.md` (Cursor profile).
3. Read `.claude/agents/publisher.md` for shared tag/manifest/retry policy.
4. **Inline only** — do not spawn Task subagents for channel workers.
5. Do not run publisher as a **background** subagent under Multitask Mode.
6. Never `git push origin v*` from local git.

## ATM vs Cursor

| | ATM/rmux | Cursor |
|---|----------|--------|
| Start | `rmux` + ATM assignment | This command |
| Channels | Background workers | **You** run them inline |

## Quick flow

```bash
python3 scripts/release_artifacts.py validate-manifest \
  --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml
gh workflow run release-preflight.yml --ref main \
  -f version="${VERSION}" -f run_by_agent=publisher
```

After authorized publish on `main`, trigger the root release workflow per
release-state strategy (typically tag via GitHub Release API — not local tag
push).

Return Cursor completion JSON from `.cursor/agents/publisher.md`.
