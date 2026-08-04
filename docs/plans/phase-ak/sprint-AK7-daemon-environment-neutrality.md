---
title: AK.7 Daemon environment neutrality
status: proposed
branch: feature/pak-s7-daemon-environment-neutrality
worktree: ../atm-core-worktrees/feature/pak-s7-daemon-environment-neutrality
target: integrate/phase-ak
recommended_agent: Cipher-311d
recommended_model: deep-reasoning
must_follow: Phase AI merge to develop
parallel_safe: true
---

# AK.7 — daemon environment neutrality

## Closure

Make the daemon team-neutral at process launch and in production source. Every
standard launcher strips `ATM_TEAM`, `ATM_IDENTITY`, and `ATM_ENVIRONMENT`
before `atm-daemon` executes. The shared CLI/graft auto-start path is the
primary required case. The daemon never reads those variables as caller
context, diagnostic scope, configuration, routing input, or fallback.

This is a narrow runtime hardening sprint. It does not modify the caller-owned
CLI environment contract, request DTOs, peer delivery, nudge, state/session
tracking, worker topology, or any AK.1–AK.6 path.

## Fixed contract

```rust
const DAEMON_STRIPPED_ENVIRONMENT: [&str; 3] = [
    "ATM_TEAM",
    "ATM_IDENTITY",
    "ATM_ENVIRONMENT",
];

fn sanitize_daemon_child_environment(command: &mut std::process::Command);
```

`sanitize_daemon_child_environment` is private to `atm-daemon-client`'s
shared daemon auto-start composition, immediately before `Command::spawn`.
It calls `Command::env_remove` for exactly the listed variables. It neither
reads their values nor alters the invoking CLI process environment.

The only daemon start boundary used by `atm` and `atm-graft` is
`DaemonSupervisor::spawn_daemon` in `crates/atm-daemon-client/src/lib.rs`.
It must invoke this helper. OS-native launch templates/scripts must perform
the equivalent removal before their daemon `exec`; do not rely on a daemon
self-check after process start.

`atm-daemon` may read its documented daemon settings (for example `ATM_HOME`
and test-only readiness controls), but production daemon source must not read,
default from, or report `ATM_TEAM`, `ATM_IDENTITY`, or `ATM_ENVIRONMENT`.
Caller identity/team continue to arrive only in typed daemon request DTOs
resolved by the invoking CLI/graft process.

## Type and boundary inventory

| Item | AK.7 role |
| --- | --- |
| `DAEMON_STRIPPED_ENVIRONMENT` | New private `atm-daemon-client` constant. The sole listed set of caller-context variables removed from daemon child processes. |
| `sanitize_daemon_child_environment` | New private `atm-daemon-client` helper. It owns the three `env_remove` calls and has no I/O, thread, state, or retry behavior. |
| `DaemonSupervisor::spawn_daemon` | Existing shared auto-start boundary. It calls the helper once immediately before spawn; `atm` and graft retain the same bootstrap path. |
| OS-native daemon launch template/script | Existing managed launcher boundary. It removes the same three names before `atm-daemon` exec, rather than forwarding a session environment. |
| daemon request DTO caller fields | Existing authoritative caller context. AK.7 retains these fields unchanged and never synthesizes them from daemon process environment. |

No new trait, service, worker, thread, timer, queue, persistence table, route,
state machine, or diagnostic data model is authorized.

## Deliverables

1. In `crates/atm-daemon-client/src/lib.rs`, add the fixed private constant and
   helper, then call it from `DaemonSupervisor::spawn_daemon` after standard
   stdio setup and before `spawn`. Preserve the existing launch gate, child
   cleanup, deadline, trace, and error semantics exactly.
2. Inventory every repository-owned standard daemon launcher: the shared
   CLI/graft auto-start path and every managed OS-native service template,
   launch script, installer, or generated plist. Each must strip exactly the
   three fixed variables before daemon exec. If this repository has no
   OS-native daemon-launch template, document that absence in the closure
   evidence; do not invent a second daemon launcher merely to satisfy this
   sprint.
3. Prove that no production read of `ATM_TEAM`, `ATM_IDENTITY`, or
   `ATM_ENVIRONMENT` exists below `crates/atm-daemon/src` or
   `crates/atm-daemon-bootstrap/src`; retain and extend
   `doctor_client_context_reflects_caller_over_daemon_launch_environment` as
   the regression proof. Preserve caller information already carried by typed
   request data. Test fixtures may set hostile parent variables solely to
   prove they do not reach the child.
4. Extend the existing `.just/lint-config.toml` `[env_var_boundary]` contract:
   add `ATM_ENVIRONMENT` to `forbidden_env_vars` and
   `crates/atm-daemon-bootstrap/src` to `restricted_crate_roots`. The existing
   `.just/check_env_var_boundary.py` gate, already run by `just lint`, then
   rejects daemon ambient-context reads. Do not add a second gate or tooling.
5. Implement `docs/requirements.md`'s `REQ-P-RUNTIME-006` as the governing
   requirement and keep its shared auto-start boundary/evidence text current;
   then update
   `docs/atm-daemon-client/boundaries.md`, `docs/atm-daemon/requirements.md`,
   and `docs/atm-daemon/startup-state-machine.md`. State that daemon
   environment sanitation is pre-exec defense in depth and not a replacement
   for typed caller context. Do not create an ADR.

## Explicit prohibitions

- Do not inspect a stripped value to select team, identity, doctor scope,
  config, routing, peer, nudge, retry, session, state, or persistence behavior.
- Do not mutate the parent CLI/graft environment, add a daemon runtime
  self-sanitizer, or add a second auto-start/LaunchAgent path.
- Do not change caller environment precedence, user-facing CLI behavior,
  request DTO shape, agent lifecycle, or cross-host delivery.

## Required validation

- Unit: use `DaemonSupervisor`'s private `spawn_daemon` test seam under hostile
  parent values for all three variables and assert its child command
  environment contains an explicit removal for each; unrelated environment
  entries remain intact.
- Process integration: a disposable daemon-child fixture launched through the
  shared auto-start path records its inherited environment. With all three
  hostile variables set in the parent, the fixture records none of them. This
  test uses no real daemon/database and does not create an alternate production
  launch path.
- Launcher audit: render/inspect every repository-owned OS-native daemon
  launcher and prove each pre-exec command removes all three names. If none is
  repository-owned, the evidence states that fact and proves the shared
  auto-start path instead.
- Source gate: the existing `env_var_boundary` lint configuration covers
  production `atm-daemon` and `atm-daemon-bootstrap` code and rejects
  `ATM_TEAM`, `ATM_IDENTITY`, `ATM_CHAT_ID`, or `ATM_ENVIRONMENT` reads; it
  runs in `just lint`.
- Regression: environment-attested CLI `send`, `read`, and `ack` still resolve
  caller identity/team before daemon dispatch and succeed against an isolated
  daemon. `just smoke localhost` and `just smoke local-ip` each prove normal
  receiver persistence and exactly one nudge.
- `just lint` and `just test` pass.

## Dependencies

AK.7 may start immediately after Phase AI merges to `develop`, from the
`integrate/phase-ak` baseline. It is parallel-safe with AK.1–AK.6: its owned
files are the shared daemon child launcher, repository-owned launcher
templates/scripts, source gate, and daemon-launch documentation; it does not
touch peer routing, TLS preservation, aliases, sender, receiver, or resend
code. Start immediately; do not wait for any AK.1–AK.6 QA result.

AK.7 has no AK predecessor merge-forward. Its PR may merge immediately after
its own QA approval. Before any AK.7 fix round, merge the current
`integrate/phase-ak` baseline and retain the fixed pre-exec sanitation boundary.
