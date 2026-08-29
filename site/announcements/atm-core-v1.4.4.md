# atm-core v1.4.4 — mTLS Peer-Wire Transport and the First Kit-Era Release

**Released:** August 28, 2026 · **Install:** `cargo install agent-team-mail` (crates.io), `brew install randlee/tap/agent-team-mail` (Homebrew), `pip install hermes-atm atm-graft` (PyPI), or binary archives from [releases](https://github.com/randlee/atm-core/releases)

[Changelog](https://github.com/randlee/atm-core/blob/main/CHANGELOG.md) · [Release notes](https://github.com/randlee/atm-core/releases/tag/v1.4.4)

---

## Agent Orchestrator

**As an agent orchestrator, I want to route work across a fleet of agents and track who is doing what, so that a multi-agent campaign stays coordinated without me hand-wiring every message.**

v1.4.4 closes the cross-host gap. The opt-in mTLS peer-wire transport lets an orchestrator fan work out across hosts — not just processes on one machine — with mutually-authenticated TLS between peers. Peer connection pooling and TLS session reuse keep that fan-out from paying a handshake tax on every message, so a campaign spanning two or three machines routes through the same `atm send <agent>@<team>` surface you already use.

Roster management, pending-ack tracking, and cross-team addressing are unchanged; the new transport is opt-in, so existing single-host fleets behave exactly as before. If you've been holding a multi-host campaign to one box waiting on cross-host addressing, this is the release that unblocks it.

---

## Harness Integrator

**As a harness integrator, I want a stable transport and persistence layer to embed, so that my runtime inherits inter-agent messaging instead of building its own IPC.**

This is the transport headline. v1.4.4 ships the opt-in mTLS peer-wire mode for cross-host messaging, with peer connection pooling, TLS session reuse, and admission-writer batching validated by officially published benchmark campaigns under the new benchmark data contract (Phase AO2). Local UDS/loopback messaging is untouched; cross-host now rides a mutually-authenticated TLS channel instead of the legacy mail path.

Hot-path guardrails land alongside it: admission-writer batching keeps the storage writer from collapsing under burst load, and the daemon-switch signing gate requires signed macOS daemons before switching. The net effect is a transport surface a runtime can embed with confidence that it scales past one host.

---

## Agent (agent-first design)

**As an agent, I want messaging that treats me as the first-class user — my own identity, a simple send/read contract, and no plumbing knowledge required — so that I can message peers reliably without understanding the transport.**

Two quality-of-life wins for agents. The Python bridge (`hermes-atm` / `atm-graft`) gains a reproducible bootstrap closure, and `.atm.toml` activation is now optional, so a bare agent process can come up without a workspace config. Identity still resolves from `ATM_TEAM`/`ATM_IDENTITY`, and the auto-start daemon singleton is unchanged — "just works" still holds.

---

## Ops Engineer

**As an ops engineer, I want observability into messaging and the ability to improve and refine agent prompts out-of-band, so that a running fleet stays healthy and steerable without restarts.**

v1.4.4 is the first release whose entire publish surface is cut over to the installed `sc-publish` kit (Phase AT/AS) — manifest-driven publishing, installer parity checks, and version-lockstep validation all flow through the shared, pinned kit rather than bespoke steps. For ops, that means releases are now produced by a reviewed, reproducible pipeline.

Fleet health also gets specific fixes: graft receiver recovery (out-of-band injection no longer wedges after a receiver outage), the daemon-switch signing gate, and a Windows loopback ack-timing flake that could stall direct-peer delivery.

---

## Agent-Graph / DAG Coordinator

**As a workflow author, I want to express agent graphs and DAGs over ATM, so that multi-step agent pipelines pass messages between nodes deterministically.**

Cross-host mTLS gives DAG coordinators the missing piece for distributed graphs: typed, acknowledged hand-offs between agent nodes now work across hosts, not just within one. Pending-ack tracking and cross-team addressing remain the reliability backbone, so a pipeline spanning two machines keeps the same delivery guarantees it had on one.

---

## What's Next

With cross-host mTLS and the kit-era publish surface in place, subsequent work turns to the ATM Send-To surface (`atm send --attach`, the `ATM_TEMP` scratch-directory contract), queue/steer nudge delivery (Phase AQ), and a native `aarch64-unknown-linux-gnu` release archive.
