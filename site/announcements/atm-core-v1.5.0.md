# atm-core v1.5.0 — Queued Steering, Send-To Attachments, and Reader-Lane Scaling

**Released:** September 4, 2026 · **Install:** `cargo install agent-team-mail` (crates.io), `brew install randlee/tap/agent-team-mail` (Homebrew), `pip install hermes-atm atm-graft` (PyPI), or binary archives from [releases](https://github.com/randlee/atm-core/releases) (now including a native `aarch64-unknown-linux-gnu` arm64-Linux archive)

[Changelog](https://github.com/randlee/atm-core/blob/main/CHANGELOG.md) · [Release notes](https://github.com/randlee/atm-core/releases/tag/v1.5.0)

---

## Agent Orchestrator

**As an agent orchestrator, I want to route work across a fleet of agents and track who is doing what, so that a multi-agent campaign stays coordinated without me hand-wiring every message.**

v1.5.0 turns ATM from fire-and-forget messaging into a work-routing system. The new `atm queue` verb mirrors `atm send`'s full surface — including `--attach` — but defers the recipient nudge instead of firing it immediately, so you can stage work and have it delivered on your terms. Underneath it sits the nudge taxonomy and `PendingNudgeStore` contract (ADR-054) that every queued delivery builds on.

That deferred delivery is now *guaranteed* to drain, not merely queued. Dual-channel graft delivery wires queue messages to a real trigger: harness idle-signal heartbeats and a bare-CLI Stop-pull path, backed by a bounded per-member FIFO. On top of that, the Herdr poll-gated wake pump drains pending queue messages on roster-wide idle transitions, and tmux idle-drain plus a kind-agnostic recovery sweep catch any drain that missed or crashed. For an orchestrator, this means deferred work actually lands — without you polling or hand-holding.

---

## Agent (agent-first design)

**As an agent, I want messaging that treats me as the first-class user — my own identity, a simple send/read contract, and no plumbing knowledge required — so that I can message peers reliably without understanding the transport.**

The headline for agents is ATM Send-To. `atm send --attach` and `--from-json` let you attach files and send structured payloads directly, with the `ATM_TEMP` scratch-directory contract (30-day TTL sweeper) and per-host cross-host transfer scripts (`sftp.sh` / `sftp.ps1`) handling the plumbing. One-gesture per-OS shell entry points for Finder/Explorer/Nautilus plus native member pickers mean you can hand a file to a teammate without learning any transport internals.

Reads also get dramatically faster. Phase AV's dedicated reader lanes for mailbox and search queries mean your `atm read` no longer queues behind the writer lane, so inbox reads stay responsive even when the daemon is under write load. Identity, auto-start, and the singleton daemon contract are all unchanged — the "just works" ergonomics stay intact.

---

## Harness Integrator

**As a harness integrator, I want a stable transport and persistence layer to embed, so that my runtime inherits inter-agent messaging instead of building its own IPC.**

The persistence and transport layer gets a hardening pass aimed squarely at embedders. Phase AV adds bounded reader pools and queue depth for mailbox/search reads; Phase AQ enforces request-budget ordering between SQLite storage, the replacement daemon, and same-host clients, and rebuilds local TCP/Unix transports after a daemon generation change. `fix(peer-tls)` fails closed on legacy literal-IP trusted-peer rows, and storage errors now preserve the raw SQLite cause with a `Cause:` line in the CLI.

Distribution also widens: a native `aarch64-unknown-linux-gnu` release archive ships arm64-Linux binaries (Apple-Silicon colima/docker without emulation), and a one-time storage rebuild relaxes the legacy `mail_messages.message_text NOT NULL` constraint so template sends work on pre-2026-08-11 databases. `fix(http-runtime)` dials direct peers by routable address with a short-lived resolution cache and surfaces the connect cause in the CLI.

---

## Ops Engineer

**As an ops engineer, I want observability into messaging and the ability to improve and refine agent prompts out-of-band, so that a running fleet stays healthy and steerable without restarts.**

`atm doctor --json` now publishes the effective `reader_lanes` contract, so you can confirm read-lane configuration directly. Out-of-band steering gains a second, selector-gated backend: `atm-herdr` (Herdr) now sits alongside retained tmux, with its own health and circuit breaker (ADR-058), letting you steer agents through a poll-gated session model rather than only tmux panes.

The 1.5.0 bump itself is the first to run through the hardened release-readiness gate — the 1.4.7–1.4.13 readiness campaign on the isolated atmbench host and Windows, with smoke/benchmark evidence and benchmark floors recorded under `site/reports/`. Fleet health fixes round it out: retained-log startup failures keep their OS cause, the daemon observability boundary is hardened in `atm-observability`, a pinned self-signed daemon-dev signing identity is supported (Phase AS), and the sc-ecosystem dependency release-preflight gate checks Wyvern/sc-compose/sc-observability pins before every release.

---

## Agent-Graph / DAG Coordinator

**As a workflow author, I want to express agent graphs and DAGs over ATM, so that multi-step agent pipelines pass messages between nodes deterministically.**

The queue/steer delivery surface is the DAG-relevant piece: deferred nudges with guaranteed drain give pipeline authors a typed "stage work, deliver later" primitive on top of the existing pending-ack reliability backbone. Cross-team addressing and mailbox persistence are unchanged, so graph nodes keep the same deterministic hand-off guarantees while gaining a queued, deferred-delivery path.

---

## What's Next

With queued steering and Send-To landed, subsequent work extends the queue/steer taxonomy (Phase AQ follow-ups), completes the arm-linux Homebrew wiring, and finishes the live m5 restart-matrix verification for the `hermes-atm` wheel.
