# Claude Instructions for atm-core

## ⚠️ CRITICAL: Branch Management Rules

**NEVER switch the main repository branch on disk from `develop`.**

- The main repo MUST remain on `develop` at all times
- **ALWAYS use `sc-git-worktree` skill** to create worktrees for all development work
- **ALWAYS create worktrees FROM `develop` branch** (not from `main`)
- Do NOT use `git checkout` or `git switch` in the main repository
- All sprint work happens in worktrees at `../atm-core-worktrees/<branch-name>`
- **All PRs target `develop` branch** (integration branch, not `main`)

**Why**: Switching branches in the main repo breaks worktree references and destabilizes the development environment.

**Worktree Creation Pattern**:
```bash
# ✅ CORRECT: Create worktree from develop
/sc-git-worktree --create feature/1-2a-work-bead develop

# ❌ WRONG: Creating from main
/sc-git-worktree --create feature/1-2a-work-bead main
```

---

## Project Overview

**atm-core** (`atm`) is a Rust CLI and daemon for mail-like messaging with Claude agent teams:
- Thin CLI over `~/.claude/teams/` file-based API (send, read, broadcast, inbox)
- Three-crate workspace: `atm-core` (library), `atm` (CLI), `atm-daemon` (plugin host)
- Atomic file I/O with conflict detection and guaranteed delivery
- Trait-based plugin system in daemon for extensibility (Issues, CI Monitor, Bridge, Chat, Beads, MCP)
- Provider-agnostic (GitHub, Azure DevOps, GitLab, Bitbucket)

**Goal**: Build a well-tested Rust CLI for agent team messaging, with a plugin-ready daemon.

---

## Project Plan

**Current Plan**: [`docs/project-plan.md`](./docs/project-plan.md)

- 5 phases, 18 sprints (Phase 6 open-ended for additional plugins)
- Parallel sprint tracks identified per phase
- Agent team execution: Scrum Master → Dev(s) + QA(s), Opus Architect on escalation
- All work on dedicated worktrees via `sc-git-worktree`

**Current Status**: Phase E complete (v0.15.0) — integration PR pending

---

## Key Documentation

**Primary references — read as needed:**

- [`docs/team-protocol.md`](./docs/team-protocol.md) - **MUST READ** ATM dogfooding messaging protocol (ack -> work -> completion -> acknowledgement)
- [`docs/requirements.md`](./docs/requirements.md) - System requirements, architecture, plugin design
- [`docs/project-plan.md`](./docs/project-plan.md) - Phased sprint plan with dependency graphs
- [`docs/agent-team-api.md`](./docs/agent-team-api.md) - Claude agent team API reference (schema baseline: Claude Code 2.1.39)
- [`docs/cross-platform-guidelines.md`](./docs/cross-platform-guidelines.md) - Mandatory Windows CI compliance patterns

**Rust development reference — read only when implementation decisions are needed:**

- [`.claude/skills/rust-development/guidelines.txt`](./.claude/skills/rust-development/guidelines.txt) - Pragmatic Rust Guidelines

**Repo-local orchestration and QA skills:**

- [`.claude/skills/team-lead/SKILL.md`](./.claude/skills/team-lead/SKILL.md) - session startup and restore flow for `team-lead`
- [`.claude/skills/codex-orchestration/SKILL.md`](./.claude/skills/codex-orchestration/SKILL.md) - phased Codex sprint orchestration with `quality-mgr`
- [`.claude/skills/phase-orchestration/SKILL.md`](./.claude/skills/phase-orchestration/SKILL.md) - phased sprint orchestration with fresh `scrum-master` coordinators
- [`.claude/skills/quality-management-gh/SKILL.md`](./.claude/skills/quality-management-gh/SKILL.md) - multi-pass QA status, CI monitoring, and PR report conventions

---

## Workflow

### Sprint Execution Pattern (Dev-QA Loop)

Every sprint follows this pattern:

1. **Create worktree** using `sc-git-worktree` skill
2. **Dev work** by assigned dev agent(s)
3. **QA validation** by assigned QA agent(s)
4. **Retry loop** if QA fails (max attempts configurable)
5. **Commit/Push/PR** to phase integration branch
6. **Agent-teams review** documenting what worked/didn't

### Phase Integration Branch Strategy

Each phase gets a dedicated integration branch off `develop`:

```
main
  └── develop
        └── integrate/phase-N              ← created at phase start
              ├── feature/pN-s1-...        ← PR targets integrate/phase-N
              ├── feature/pN-s2-...        ← PR targets integrate/phase-N
              └── feature/pN-s3-...        ← PR targets integrate/phase-N

        After all sprints merge → one PR: integrate/phase-N → develop
```

**Rules:**
- Sprint PRs target `integrate/phase-N` (not `develop` directly)
- After each sprint merges to the integration branch, subsequent sprints merge latest `integrate/phase-N` into their feature branch before creating their PR
- When all phase sprints are complete, one final PR merges `integrate/phase-N → develop`
- Phase integration branch is then cleaned up

### Worktree Cleanup Policy

**Do NOT clean up worktrees until the user has reviewed them.** The user reviews each sprint's worktree separately to check for design divergence before approving cleanup. Worktree cleanup is only performed when explicitly requested.

### Branch Flow

- Sprint PRs → `integrate/phase-N` (phase integration branch)
- Phase completion PR → `develop` (integration branch)
- Release PR → `main` (after user review/approval)
- Post-merge CI runs as safety net at each level

---

## Agent Model Selection

- **Haiku** - Exploration, test execution, simple validation
- **Sonnet** - Implementation work, documentation writing
- **Opus** - Critical planning, architecture decisions, complex review

---

## Environment

**Task List**: `agent-team-mail`
**Agent Teams**: Enabled (experimental feature)

---

## Agent Team Mail (ATM) Communication

### Team Configuration

- **Team**: `atm-dev` (persistent across sessions)
- **ARCH-ATM** (you) is `team-lead` — start and maintain the `atm-dev` team for the session duration
- **ARCH-CTM** is a Codex agent — communicates **exclusively** via ATM CLI messages (not Claude Code team API)
- **All other Claude agents** communicate using Claude Code's built-in team messaging API (`SendMessage` tool)

### Identity

`.atm.toml` at repo root sets `identity = "team-lead"` and `default_team = "atm-dev"`, so all ATM CLI commands automatically use the correct identity and team. No need to prefix with `ATM_IDENTITY=` or `--team`.

**Note**: ARCH-CTM gets his identity from `ATM_IDENTITY=arch-ctm` set in his tmux session (via rmux or manually).

### Communicating with ARCH-CTM (Codex)

ARCH-CTM does **not** monitor Claude Code messages. Use ATM CLI only:

**Send a message:**
```bash
atm send arch-ctm "your message here"
```

**Check your inbox for replies:**
```bash
atm read
```

**Check team inbox summary (who has unread messages):**
```bash
atm inbox
```

**Nudge ARCH-CTM to check inbox** (when he hasn't replied):

ARCH-CTM runs in a tmux pane. Discover the pane, then send-keys:
```bash
# Find arch-ctm's pane
tmux list-panes -a -F '#{session_name}:#{window_index}.#{pane_index} #{pane_title} #{pane_current_command}'

# Send nudge (use the correct pane ID from above)
tmux send-keys -t <pane-id> -l "You have unread ATM messages. Run: atm read --team atm-dev" && sleep 0.5 && tmux send-keys -t <pane-id> Enter
```

### Communication Rules

1. **No broadcast messages** — all communications are direct (team-lead ↔ specific agent)
2. **Poll for replies** — after sending to arch-ctm, wait 30-60s then `atm read`. If no reply after 2 minutes, nudge via tmux send-keys
3. **arch-ctm is async** — he processes messages on his next turn. Do not block waiting; continue other work and check back

### ATM CLI Quick Reference

| Action | Command |
|--------|---------|
| Send message | `atm send <agent> "msg"` |
| Read inbox | `atm read` |
| Inbox summary | `atm inbox` |
| List teams | `atm teams` |
| Team members | `atm members` |

---

## Initialization Process

**If `ATM_IDENTITY=team-lead`**: Run the `/team-lead` skill.
It confirms identity, detects whether a restore is needed, and either proceeds
directly to project status (fast path) or invokes the full restore procedure.
See `.claude/skills/team-lead/SKILL.md` for the startup steps and
`.claude/skills/team-lead/backup-and-restore-team.md` for the restore procedure.

**If `ATM_IDENTITY` is any other value**: Skip team restore — you are not the team lead.

> ⚠️ Do NOT use `atm teams resume` — it archives the team directory. The startup skill
> uses the correct restore procedure (backup → TeamDelete → TeamCreate → restore).

After startup completes:
1. Read project plan (`docs/project-plan.md`)
2. Check current status (branches, PRs, worktrees) via `atm gh pr list`
3. Output concise project summary and status to user
4. Identify the next sprint(s) ready to execute
5. Be prepared to begin the next sprint upon user approval

---

## gh Keychain — Do NOT Diagnose ACL Issues

gh tokens are stored as generic passwords in the macOS login keychain (via
go-keyring). Generic passwords have no per-application ACL — any user process
can read them. Do not recommend `gh auth login` to "fix" keychain permissions.
If `gh auth token` returns a token without error, authentication is working.
See `~/Documents/.configuration/git-config.md` for multi-account setup details.
