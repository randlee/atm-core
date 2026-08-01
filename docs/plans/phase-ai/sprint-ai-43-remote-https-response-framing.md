---
title: AI.43 remote HTTPS response framing
status: complete
branch: feature/pAI-s43-remote-https-response-framing
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_mode: after_merge
execution_dependencies:
  - AI.39
dependencies_relation:
  - sprint: AI.39
    relation: must_follow
    rationale: Reuses AI.39's bounded shared frame-reader primitive.
target: integrate/phase-ai-31-33
depends_on: AI.39
---

# AI.43 — Remote HTTPS response framing

## Recommended Agent / Model

`arch-ctm` / deep-reasoning: remote response framing crosses the shared parser,
client deadline wrapper, and mTLS transport boundary.

## Execution Dependencies

AI.43 `must_follow`s AI.39. Merge-forward trigger: AI.39 development is
pushed, not QA; before every round merge AI.39 into this branch. PR-completion
trigger: AI.39's PR merges into `integrate/phase-ai-31-33` first.

## Goal

Move `read_http_response` and remote HTTPS response call sites from the
byte-at-a-time reader to AI.39's bounded shared framing primitive without
changing mTLS authorization, deadlines, routes, or typed error contracts.

## Governing requirements and ADRs

- `REQ-CORE-TRANSPORT-001`, `REQ-CORE-TRANSPORT-002B`, and `REQ-CORE-TRANSPORT-005B`
- ADR-032 — unified error contract
- ADR-033 — HTTP endpoint contract
- AI.39 bounded frame-reader contract

## Exact Targets

- `crates/atm-core/src/api.rs` and `crates/atm-core/src/api/http_frame_reader.rs`
- `crates/atm-daemon/src/https_transport.rs`
- `crates/atm-daemon-client/src/http_exchange.rs`
- response-framing tests in the owning crates

## Deliverables

1. Make `read_http_response` use the shared bounded response-frame reader; no
   remote response parser may retain a one-byte system-read loop.
2. Route daemon peer HTTPS and daemon-client response reads through that public
   compatibility entry point. Preserve mTLS authorization, request deadlines,
   response-body limits, routes, and typed errors.
3. Add deterministic tests for fragmented delimiter/body boundaries, coalesced
   response frames with retained surplus, malformed/oversized responses, and
   mTLS peer response compatibility.

## Acceptance Criteria

- HTTPS and local response readers share one bounded framing primitive.
- Existing remote mTLS and deadline behavior is unchanged.
- No production response parser uses a one-byte read loop.

## Required Validation

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --no-fail-fast`
- `just lint`
- `just test`

## Non-goals

No HTTP/2, remote keep-alive policy change, route change, or throughput claim.
