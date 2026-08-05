# `[scmux]` launcher configuration — proposal

**Status:** Proposal; this document defines no ATM runtime behavior and does
not require `atm-core` to parse, validate, or execute `[scmux]`.

`[scmux]` is the launcher-owned successor namespace to the existing `[rmux]`
layout. It is deliberately outside the ATM-owned `[atm]` contract: launchers
own session lifecycle and the scmux daemon consumes the resulting monitoring
definition read-only.

## Why this seam exists

Two incidents on 2026-08-05 exposed the boundary that the configuration must
preserve: a dead `arch-hitl` pane left mail without a reader, and a Codex wake
was typed but remained staged rather than submitted. A daemon must therefore
not restart ATM sessions through tmuxp: it would orphan the ATM pane
registration. When supervision detects a dead session, it delegates to the
launcher that creates and registers panes.

## Proposed shape

The layout is the additive union of the proven `[rmux]` session/window/pane
shape and ATM's declarative identity/runtime needs. `command` is argv, not a
shell string. `{team_name}` is the only interpolation in version 1.

```toml
[scmux]
version = 1

[scmux.session]
name = "aidw-platform"
project = "AIDevWorkspace"

[scmux.launch]
command = ["scripts/aidw", "team", "start", "{team_name}"]

[scmux.supervision]
enabled = false
stale_after_seconds = 1800
fallback = "launch_command"

[[scmux.session.windows]]
name = "agents"
layout = "even-horizontal"

[[scmux.session.windows.panes]]
name = "aidw-lead"
shell_command = "claude --model sonnet"
atm_identity = "aidw-lead@aidw-platform"
atm_runtime = "tmux-pane"
```

Windows/headless teams can be mail-only from the outset; they do not declare a
tmux session, window, or pane:

```toml
[scmux]
version = 1

[scmux.supervision]
enabled = true
fallback = "mail-only"
stale_after_seconds = 1800
```

## Contract rules

- `version` is required and starts at `1`.
- The `[rmux]` layout remains supported during migration. A launcher may read
  either section, but must reject an ambiguous simultaneous definition rather
  than merge incompatible layouts implicitly.
- Every supervised `tmux-pane` declares `atm_identity`. Launchers set the
  corresponding `@atm_identity`, `@atm_team`, and `@atm_runtime` pane options
  before registering it in the ATM roster. Discovery is fail-closed when those
  options are absent or ambiguous.
- `atm_runtime` is `tmux-pane` by default and may be `mail-only`. Mail-only is
  a detached, non-interactive execution mode; it is appropriate for reviewed
  task-scoped roles and supplies the Windows/headless path.
- `launch.command` is required when `supervision.enabled = true` and
  `fallback = "launch_command"`; it is called by the daemon after a
  dead-session decision instead of `tmuxp start`.
- Supervision is opt-in. `fallback = "launch_command"` is the proposed
  session-recovery path and `fallback = "mail-only"` is the detached
  per-message reader path. The daemon records launcher failure and never loops
  or starts a session itself.
- Coordinate-based post-send targets are deprecated for new configurations.
  Post-send routing uses recipient plus argv and runtime discovery through
  `@atm_identity`, never a fixed `session:window.pane` address.
- The daemon remains monitoring-only for ATM teams: `auto_start` stays false,
  dashboard start/stop controls must not be used, and `[atm].allow_shutdown`
  remains false.

## Protocol boundary

The scmux `[atm]` poller should be a client of atm-core's typed
`RequestEnvelope` / `ResponseEnvelope` protocol. Its read-only use needs only
`List` and `Doctor`; it must not add the retired `list-agents` or `agent-state`
JSON command protocol to atm-core. `allow_shutdown` remains a no-op.

## Non-goals

This proposal does not move launcher behavior into `atm-core`, prescribe a
tmux implementation on Windows, or authorize scmux to start/stop/restart an
ATM team directly. It is intentionally documentation-first so the contract
owner can settle field names and validation before code adoption.
