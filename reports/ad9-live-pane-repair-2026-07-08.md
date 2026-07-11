# AD.9 Live Pane Repair Evidence

Date: `2026-07-08`
Branch: `feature/pAD-s20-read-body-search-metadata-consistency-repair`

## Purpose

Provide repository evidence that the accepted live `atm-dev` baseline roster
was repaired through the retained `teams update-member` path, not only through
an isolated tempdir fixture.

## Notes

- The installed `atm` binary on PATH does not expose `teams update-member` in
  this environment, so the evidence run used the branch-built CLI under review:
  `target/debug/atm`.
- Before the repair pass, the live baseline already showed non-blank pane ids
  for the targeted members:

```text
Team: atm-dev
  team-lead | type=team-lead model=claude-sonnet-4-6 cwd=/Users/randlee/Documents/github/atm-core-worktrees/feature/pAA-smoke-fixes pane=%0
  arch-ctm | type=codex model=high cwd=/Users/randlee/Documents/github/atm-core-worktrees/feature/pAA-smoke-fixes pane=%5
  quality-mgr | type=quality-mgr model=unknown cwd=/Users/randlee/Documents/github/atm-core pane=%2
```

## Applied Repair Commands

```text
target/debug/atm teams update-member atm-dev team-lead --home-dir /Users/randlee/Documents/github/atm-core --harness claude-code --agent-type team-lead --model claude-sonnet-4-6 --pane-id %0
Updated member team-lead in atm-dev

target/debug/atm teams update-member atm-dev arch-ctm --home-dir /Users/randlee/Documents/github/atm-core --harness codex-cli --agent-type codex --model high --pane-id %5
Updated member arch-ctm in atm-dev
```

## Post-Repair Canonical Roster Evidence

Output from `target/debug/atm members --team atm-dev --json` after the repair
commands:

```json
{
  "team": "atm-dev",
  "members": [
    {
      "name": "team-lead",
      "agent_id": "team-lead@atm-dev",
      "agent_type": "team-lead",
      "model": "claude-sonnet-4-6",
      "joined_at": 1780770571075,
      "tmux_pane_id": "%0",
      "home_dir": "/Users/randlee/Documents/github/atm-core",
      "extra": {
        "backendType": "tmux",
        "isActive": true
      }
    },
    {
      "name": "arch-ctm",
      "agent_id": "arch-ctm@atm-dev",
      "agent_type": "codex",
      "model": "high",
      "joined_at": 1780770571079,
      "tmux_pane_id": "%5",
      "home_dir": "/Users/randlee/Documents/github/atm-core",
      "live_cwd": "/Users/randlee/Documents/github/atm-core-worktrees/feature/pAD-s20-read-body-search-metadata-consistency-repair",
      "extra": {
        "backendType": "tmux",
        "isActive": true
      }
    },
    {
      "name": "quality-mgr",
      "agent_id": "quality-mgr@atm-dev",
      "agent_type": "quality-mgr",
      "model": "unknown",
      "joined_at": 1783063157542,
      "tmux_pane_id": "%2",
      "home_dir": "/Users/randlee/Documents/github/atm-core",
      "extra": {
        "backendType": "tmux",
        "isActive": true
      }
    }
  ]
}
```

## Post-Repair Compatibility Projection Evidence

Relevant rows from `~/.claude/teams/atm-dev/config.json` after the repair:

```json
{
  "name": "team-lead",
  "agentId": "team-lead@atm-dev",
  "agentType": "team-lead",
  "model": "claude-sonnet-4-6",
  "tmuxPaneId": "%0",
  "home_dir": "/Users/randlee/Documents/github/atm-core"
}
{
  "name": "arch-ctm",
  "agentId": "arch-ctm@atm-dev",
  "agentType": "codex",
  "model": "high",
  "tmuxPaneId": "%5",
  "home_dir": "/Users/randlee/Documents/github/atm-core"
}
```
