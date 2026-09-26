# atm-core v1.6.1 — Typed Observability Consolidation and Immutable Releases

**Released:** September 23, 2026 · **Install:** `cargo install agent-team-mail` (crates.io), `brew install randlee/tap/agent-team-mail` (Homebrew), `pip install hermes-atm atm-graft atm-query` (PyPI), or binary archives from [releases](https://github.com/randlee/atm-core/releases)

[Changelog](https://github.com/randlee/atm-core/blob/main/CHANGELOG.md) · [Release notes](https://github.com/randlee/atm-core/releases/tag/v1.6.1)

---

## Harness Integrator

**As a harness integrator, I want a stable transport and persistence layer to embed, so that my runtime inherits inter-agent messaging instead of building its own IPC.**

v1.6.1 consolidates ATM's observability surface onto the published `sc-observability` / `sc-observability-types` `=1.4.1` family. The duplicated error and health mapping that lived in `atm` and `atm-observability` is replaced by the upstream typed APIs — `InitFailure`, `LogFailure`, `FlushFailure`, and `LoggingHealthReport` — so a runtime embedding ATM inherits one canonical, versioned error contract instead of a parallel set of ATM-local types. The ATM error contract is retained and matrix-tested, so existing callers keep their semantics while the surface becomes genuinely upstream rather than a fork.

Alongside the typed error adoption, the new log macros and the `#[instrument]` attribute are qualified with compile-pass and compile-fail UI tests, locking down the macro surface without changing logging ownership. For an integrator, the practical result is a single dependency family (`=1.4.1`) whose error types and logging attributes are published and semver-pinned, instead of ATM-specific definitions that could drift from the rest of the Synaptic Canvas stack.

---

## Ops Engineer

**As an ops engineer, I want observability into messaging and the ability to improve and refine agent prompts out-of-band, so that a running fleet stays healthy and steerable without restarts.**

The observability consolidation is the release's backbone: fleet logging now flows through the published `sc-observability` 1.4.1 error/health types, and the benchmark floor is lowered (`m5-atmbench` TCP p50 floor to 16000 msg/s at baselines revision 6) to keep the throughput signal honest while the TCP root-cause task is tracked separately.

Release hygiene gets the bigger headline. ATM now conforms its `sc-publish` release consumer byte-for-byte to the canonical kit — release manifests, channel contracts, and scoop/winget publish workflows are rendered from the kit templates, and `check_version_sync` now tracks `[[python_distributions]]`. The immutable-releases repository setting is enabled, and the first immutable release is produced from `main`. In practice that means a release, once cut, can't be silently mutated after the fact — the kind of guarantee that keeps a running fleet's installs reproducible and auditable.

---

## Agent Orchestrator

**As an agent orchestrator, I want to route work across a fleet of agents and track who is doing what, so that a multi-agent campaign stays coordinated without me hand-wiring every message.**

Team-lead startup now verifies that `.atm.toml` aliases, the ATM roster, and the live Herdr agents all agree, repairing the roster with `atm teams update-member` instead of the old backup-and-restore dance. For an orchestrator, that means the routing identity you see is the identity that's actually live — drift between alias, roster, and running agent is caught and corrected at startup rather than surfacing as a misrouted message mid-campaign.

---

## Agent (agent-first design)

**As an agent, I want messaging that treats me as the first-class user — my own identity, a simple send/read contract, and no plumbing knowledge required — so that I can message peers reliably without understanding the transport.**

A mutating `atm read` used to re-query mailbox metadata after persisting read state, then index the refreshed rows with a selection index from the earlier snapshot — so a concurrent `clear` could make the read fail or show the wrong row. The display now reloads from the selection snapshot and the durable record is loaded fresh by key, so what you read is what's actually in your mailbox. Task templates rendered from `.xml.j2` also deliver their variables verbatim under sc-compose 1.6 autoescaping, so the work orders you receive aren't mangled by template escaping.

---

## Agent-Graph / DAG Coordinator

**As a workflow author, I want to express agent graphs and DAGs over ATM, so that multi-step agent pipelines pass messages between nodes deterministically.**

No direct DAG-surface changes ship in v1.6.1 — this release's typed-error consolidation and read-reliability fixes harden the message backbone the DAG layer builds on, but no new graph, task, or hand-off capability is added. (No-impact for this persona this release.)

---

## What's Next

With observability consolidated onto the published 1.4.1 family and immutable releases enabled, subsequent work continues the release-readiness hardening and the TCP throughput root-cause task flagged by the benchmark floor adjustment.
