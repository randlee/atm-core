# atm-core v1.4.3 — Tokio HTTP Runtime, Template Messages, and First PyPI Publish

**Released:** August 17, 2026 · **Install:** `cargo install agent-team-mail` (crates.io), `brew install randlee/tap/agent-team-mail` (Homebrew), or binary archives from [releases](https://github.com/randlee/atm-core/releases)

[Changelog](https://github.com/randlee/atm-core/blob/main/CHANGELOG.md) · [Release notes](https://github.com/randlee/atm-core/releases/tag/v1.4.3)

> **⚠️ v1.4.2 is abandoned — do not use it.** The `v1.4.2` tag pointed at a commit that predated a required packaging fix: `cargo publish`'s isolated verification build failed because the CLI embedded `docs/atm-http-runtime/openapi.yaml` via `include_str!` from outside the crate directory, so it was absent from the tarball. v1.4.3 republishes all 12 crates from a clean `release/v1.4.3` line (PR #930). The 11 crates already published at `1.4.2` on crates.io remain there permanently; use `1.4.3` instead.

---

## Agent Orchestrator

**As an agent orchestrator, I want to route work across a fleet of agents and track who is doing what, so that a multi-agent campaign stays coordinated without me hand-wiring every message.**

v1.4.3 lands the decomposed template message and queryable-message line (Phase AN). A durable template catalog with render-on-read means an orchestrator can define a message template once and fan it out without re-authoring each send. Typed search and raw read-only analyst queries give deterministic lookups into the mailbox, and the generic `atm compose` workflow wraps the catalog so template-driven messages become a first-class orchestration step rather than a hand-built string.

Roster management gains `atm teams remove-member`, so an authorized local orchestrator can remove a team member without touching the store directly. The pending-ack and cross-team addressing surfaces are unchanged, so existing fan-out flows keep working.

---

## Harness Integrator

**As a harness integrator, I want a stable transport and persistence layer to embed, so that my runtime inherits inter-agent messaging instead of building its own IPC.**

The transport layer got its biggest rewrite since the SQLite backbone: v1.4.3 ships the minimal Tokio/Axum HTTP runtime (`atm-http-runtime`), replacing hand-written synchronous HTTP framing for local and cross-host listeners (Phase AL). Phase AM then deleted the now-redundant legacy machinery — legacy HTTP framing, local/peer transport workers, resend/replay, and the obsolete cross-host subsystem — resetting the daemon to a clean local-IPC-only singleton. The result is a smaller, more auditable transport surface with the same mailbox contract.

The published crate set grows from 9 to 12: `atm-error`, `atm-http-runtime`, and `atm-template-sc-compose` are now on crates.io, alongside the existing line. `hermes-atm` and `atm-graft` ship a full manylinux/musllinux/Windows/aarch64 wheel and sdist pipeline (CPython 3.11–3.14) — the first PyPI publishing for the ATM Python surface.

---

## Agent (agent-first design)

**As an agent, I want messaging that treats me as the first-class user — my own identity, a simple send/read contract, and no plumbing knowledge required — so that I can message peers reliably without understanding the transport.**

The PyPI publish is the headline for agents: `hermes-atm` and `atm-graft` are now installable via `pip`, giving native `atm_send`/`atm_read` tooling and graft-based injection without a Rust toolchain. Identity still resolves from `ATM_TEAM`/`ATM_IDENTITY`, and the auto-start daemon singleton is unchanged, so the agent experience remains "just works" — only now with a pip-installable path.

---

## Ops Engineer

**As an ops engineer, I want observability into messaging and the ability to improve and refine agent prompts out-of-band, so that a running fleet stays healthy and steerable without restarts.**

Daemon runtime session/pid observation converges into diagnostic-only heartbeat caching (Phase AJ), trading aggressive runtime tracking for a leaner singleton that still surfaces health through `atm doctor`. `atm-graft` remains the out-of-band steering path, now pip-installable and easier to script into fleet operations.

---

## Agent-Graph / DAG Coordinator

**As a workflow author, I want to express agent graphs and DAGs over ATM, so that multi-step agent pipelines pass messages between nodes deterministically.**

The template message and queryable-message line directly strengthens typed hand-offs between agent nodes: a template catalog plus typed search gives DAG coordinators a reliable, queryable message contract between nodes. Cross-team addressing and pending-ack tracking remain the reliability backbone this persona builds on.

---

## What's Next

v1.4.3 carries the full Phase AL/AM/AN feature set (originally authored for the abandoned 1.4.2) plus the recovery fix. With the Tokio runtime in place and legacy transport deleted, subsequent work turns to the PyPI production publish (currently on TestPyPI), Phase AI cross-host targets, and deeper analyst-query surfaces.
