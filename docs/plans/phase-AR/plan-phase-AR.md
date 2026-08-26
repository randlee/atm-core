# Phase AR plan — requirements on atm-core for the codex-atm integration

Drafted 2026-08-16 (Rand + hendrix-side planning; source draft in the hendrix
repo `codex-ops/ATM-CORE-INTEGRATION-REQUIREMENTS.md`). Pending arch-ctm
review; R4 is the blocking architectural decision.

## Context

`randlee/codex-atm` (fork of openai/codex) will carry an ATM integration on
its `develop` branch: the codex CLI gains native agent-team-mail via the
`atm-graft` crate — `a)` atm-graft + integration + unit tests, `b)` sc-lint
boundary rules so the implementation does not leak into codex internals.
Integration updates are release-triggered (codex releases ~2×/week). The
maintenance workflow replicates the hermes-agent-atm pipeline (mechanical
runner → contessa → alpha-prime chain; beads; idempotent re-runs) with
**leonardo** (kimi-k3, general-purpose rust architect) as the escalation
agent. Deliberate constraint carried over from the hermes design: ALL
codex-specific knowledge lives in docs that travel with the codex patch stack
(`docs/atm/CODEX-PATCH-REQUIREMENTS.md` there), never in leonardo's system
prompt.

## Requirements

### R1 — Publishable, consumable crate
- `atm-graft` + dependency closure (incl. `agent-team-mail-core`) on
  crates.io, semver-disciplined. The 1.4.2 version-alignment gates
  (`check_version_sync.py`, `version_alignment.rs`) are the enforcement; the
  crates.io staging order (core first, per RELEASE_READINESS.md) must be
  unblocked.
- Committed codex-atm Cargo.toml references the crates.io version; local
  development uses `[patch.crates-io]`; CI/pipeline builds succeed WITHOUT a
  local atm-core checkout.
- Apache-2.0-compatible licensing; MSRV ≤ codex's pinned toolchain; no
  build.rs network/system-dep surprises.

### R2 — Rust-native host embedding API (the codex seam)
- Public, rustdoc'd, stability-stated Rust API on `atm-graft` for an external
  host: activate receiver (identity, team, host binding), clean shutdown —
  the Rust equivalent of what `atm-graft-python` gives hermes.
- **API shape: TWO Rust channels** (literally channels, e.g.
  `tokio::sync::mpsc::Receiver<Nudge>` × 2) handed to the host on
  activation — one for **steer**-mode nudges, one for **queue**-mode nudges.
  atm-graft carries one delivery channel today; growing the second (and the
  sender-side mode routing that feeds it) is atm-core work under this
  requirement. Host-side consumption design:
  hendrix `codex-ops/STEER-QUEUE-DESIGN.md`.
- **Queue-channel semantics — staged work, not nudges**: the queue stages
  tasks/beads (backlogs of hundreds; days of work) held DURABLY upstream
  (daemon/mailbox), trickled to the host through a BOUNDED channel (prefetch
  window) with backpressure when full. Delivery acknowledgment on
  consumption: a task is acked only when the host actually injects it into a
  turn; unacked tasks are redelivered on reconnect, so a host crash/restart
  loses nothing. The steer channel stays lightweight/unbuffered by contrast.
- Tokio compatibility: codex is tokio-based. Preferred: tokio-native
  activation (aligns with atm-http-runtime). Minimum: a documented thread
  model so the host bridges callbacks safely.
- Library-call send/read/ack (no shelling out to the `atm` binary from codex).

### R3 — Fail-loud activation (issue #900 becomes a prerequisite)
- Activation keyed on ATM_IDENTITY + ATM_TEAM (+ host binding). `.atm.toml`
  must not gate activation; every activation failure raises. The silent no-op
  is disqualifying for a crates.io consumer. Land #900 before or with the
  first codex integration release.

### R4 — Protocol sequencing (DECISION REQUIRED, blocks seam design)
- Does codex integrate against the current file-rendezvous/loopback graft
  protocol, or against the daemon long-poll session protocol (#899), which
  ADR-033…ADR-037 already point at (HTTP over UDS locally)?
- Position for review: codex is a greenfield consumer and the natural FIRST
  client of the session protocol. Integrating it against the legacy
  rendezvous means running the hermes-style migration twice. If even a
  minimal session slice can land first, codex never carries the legacy.
- Requirement: arch-ctm ruling + migration statement either way.

### R5 — Boundary enforcement (sc-lint)
- Packaged `sc-lint-*` boundary rule-set + config consumable from codex-atm:
  ATM code stays behind its module boundary; no atm types leak into codex
  internals beyond the sanctioned seam; no reaching into codex internals from
  ATM code beyond documented touch points. Versioned with atm-core releases;
  invoked as `just lint atm` in codex-atm.

### R6 — Hermetic test & smoke support
- Test-support surface (feature or fixtures crate): mock/loopback delivery,
  deterministic nudge injection, handshake fixtures — enough for codex-atm's
  `just test atm` to run with NO live daemon.
- `just smoke atm` = live round-trip against a local daemon (hermes-atm-smoke
  contract style); may require a running daemon and says so.
- The seam contract tests live IN the codex-atm patch stack (mirror of the
  hermes `test_inject_internal_message.py` suite); atm-core supplies the
  harness, codex-atm owns the contract.

### R7 — Cadence & compatibility
- Pinned, consumable releases (never yank-and-replace a version).
- Per-release compatibility statement: protocol/daemon versions supported —
  one host machine runs the hermes fleet AND codex agents against one daemon.

## Non-goals (owned elsewhere)

- Codex-side seam design, patch stack, CODEX-PATCH-REQUIREMENTS.md → codex-atm
  repo (develop branch).
- Maintenance pipeline (runner, review chain, beads, leonardo escalation) →
  hendrix `hermes-ops/` + grecon cron.
- Leonardo's system prompt stays general-purpose rust architecture.

## Staffing (initial integration)

The develop-branch integration is an **atm-dev team** project, not a hermes
one. The team runs out of the atm-dev repo AS FAR AS ATM IS CONCERNED — the
existing atm-core `.atm.toml`/rmux layout is the team anchor, extended with
panes for 1–2 additional dev agents whose WORKING DIRECTORY is the codex-atm
checkout (the proven multi-repo pattern: each agent's context stays focused on
a single repo; team plumbing stays in one place). **arch-ctm** is atm-side
architect but expects little atm-graft dev work — the crate is proven — so his
role is support and infrastructure (R5 rule packaging, R6 fixtures, R2 API
surface review). Dev-agent startup directives point at this plan and the R5
boundary rules. Coordination uses the existing terminal-agent atm path (tmux
nudge + `atm read`) until the native integration they are building supersedes
it — deliberate dogfooding. Ongoing MAINTENANCE (release-triggered sync
pipeline, leonardo escalation) stays on the hermes/hendrix side and is out of
scope for the dev team.

## Proposed sequencing

1. arch-ctm review of this plan; R4 ruling.
2. R1 unblock (crates.io staging) + R3 (#900) — independent of R4.
3. R2 API shaped by the R4 ruling; R5/R6 packaging alongside.
4. codex-atm develop stack work proceeds against R1–R3/R5–R6; the
  release-triggered pipeline is built in parallel on the hendrix side.
