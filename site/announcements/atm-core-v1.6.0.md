# atm-core v1.6.0 — Task-State Tracking, Native-IPC Herdr Transport, and Roster Aliases

**Released:** September 15, 2026 · **Install:** `cargo install agent-team-mail` (crates.io), `brew install randlee/tap/agent-team-mail` (Homebrew), `pip install hermes-atm atm-graft` (PyPI), or binary archives from [releases](https://github.com/randlee/atm-core/releases)

[Changelog](https://github.com/randlee/atm-core/blob/main/CHANGELOG.md) · [Release notes](https://github.com/randlee/atm-core/releases/tag/v1.6.0)

---

## Harness Integrator

**As a harness integrator, I want a stable transport and persistence layer to embed, so that my runtime inherits inter-agent messaging instead of building its own IPC.**

v1.6.0 makes Herdr a first-class native transport. Phase AY cuts the six-operation Herdr client over from a per-nudge CLI process to Herdr's own native IPC — a Unix socket on macOS/Linux, a named pipe on Windows — so a runtime that embeds ATM no longer forks a CLI process for every nudge. The CLI path is retained as a bounded fallback (`herdr.transport = "cli"`), and `atm doctor` reports the active transport plus a sanitized endpoint, so an integrator can confirm which delivery path a fleet is actually using.

Nudge templating also becomes transport-neutral. Phase AX has Herdr render the built-in templates — and gives `atm queue` its own template class — instead of injecting fixed wake text, so a sender's message body is never interpolated as terminal input on any code path. Underneath, `fix(graft)` carries the rendered template only, persists before the received hook, and reports handoff acceptance on graft reads, keeping the hook inside the request budget.

---

## Agent Orchestrator

**As an agent orchestrator, I want to route work across a fleet of agents and track who is doing what, so that a multi-agent campaign stays coordinated without me hand-wiring every message.**

Task-state tracking is the headline. Phase AX makes a task-tagged message impossible to double-ack, and adds `atm task start` plus a reminder cycle and lead-notification escalation, with `atm doctor` surfacing task state. Phase BB then replaces the two coarse task-family nudge kinds with six per-transition kinds — assigned, started, reminded, completed, refused, and cancelled — so every state change is visible in the recipient's prompt line, and `atm task start` is exposed to the assignee. Together they turn a task from "a message" into a tracked lifecycle you can observe and escalate without hand-rolling any of it.

Roster aliases sharpen routing. `atm teams add-member|update-member --alias <name>` / `--clear-alias` give a member a durable alias that Herdr uses as its live-agent target, while the canonical name stays the routing/audit identity. `atm send`, `ATM_IDENTITY`, and `--as` all resolve aliases, and unique-name (`alias ?? name`) must be unique across the database — a roster write creating a collision is rejected, and `atm doctor` reports legacy collisions — so a fleet stays unambiguous even as agents are renamed or aliased.

---

## Ops Engineer

**As an ops engineer, I want observability into messaging and the ability to improve and refine agent prompts out-of-band, so that a running fleet stays healthy and steerable without restarts.**

v1.6.0 is a heavy observability-and-correctness release. `atm doctor` now surfaces task state, the active Herdr transport and endpoint, and legacy unique-name collisions — fleet health and routing ambiguity become visible in one command rather than inferred. The EQ-* batch closes a long tail of real-world failures: mailbox selection and counts move into SQL, fixing `atm read`/`atm list` timeouts against mailboxes over roughly 1.5k messages (and counting 888k+ message mailboxes, EQ-016); CLI command-error log events now persist the actual failure text instead of always writing `message: null` (EQ-017); `atm peer trust replace/add` no longer reports exit 4 for a write that actually persisted offline (EQ-007); and `atm task assign` by a non-assigner refuses instead of silently no-opping (EQ-008).

Security and release hygiene round it out. rustls 0.23.43 → 0.23.45 closes RUSTSEC-2026-0285 — a TLS 1.3 handshake wrong-encryption-level acceptance on the daemon/peer mTLS path (EQ-013). The prerelease `daemon-switch` step now supplies `--service` (EQ-009), a concurrent schema-migration race on `decomposed_messages` view creation is fixed (EQ-010), `atm` exits quietly on a broken stdout pipe instead of panicking (EQ-012), and smoke/benchmark scripts assert through the `atm` CLI instead of reaching into sqlite3 directly (EQ-006).

---

## Agent (agent-first design)

**As an agent, I want messaging that treats me as the first-class user — my own identity, a simple send/read contract, and no plumbing knowledge required — so that I can message peers reliably without understanding the transport.**

Task transitions now appear directly in your prompt line — assigned, started, reminded, completed, refused, cancelled — six distinct kinds replacing the two coarse task nudges, so you can tell at a glance what's being asked of you. Nudges carry Herdr-rendered templates rather than fixed wake text, and `ATM_IDENTITY` / `--as` / `atm send` resolve your roster alias, so you're addressed by the name the team actually uses while the canonical identity stays intact for routing and audit.

---

## Agent-Graph / DAG Coordinator

**As a workflow author, I want to express agent graphs and DAGs over ATM, so that multi-step agent pipelines pass messages between nodes deterministically.**

Task-state tracking strengthens the typed hand-off the DAG layer relies on: a task-tagged message can no longer be double-acked, and the six per-transition nudge kinds give pipeline authors a deterministic signal at each node boundary. Cross-team addressing and mailbox persistence are unchanged, so graph nodes keep the same reliability guarantees while gaining a first-class task lifecycle on top of the pending-ack backbone.

---

## What's Next

With task-state tracking, native-IPC Herdr transport, and roster aliases landed, subsequent work extends the task domain (Phase AZ's bounded task-nudge contract and attention scheduler are already in flight) and continues the release-readiness hardening loop.
