---
title: AI.39 buffered local HTTP framing
status: proposed
branch: feature/pAI-s39-buffered-local-http-framing
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_mode: after_merge
execution_dependencies:
  - AI.33
dependencies_relation:
  - sprint: AI.33
    relation: must_follow
    rationale: Shares the local admission/framing path that AI.33 establishes.
target: integrate/phase-ai-31-33
depends_on: AI.33
---

# AI.39 — Buffered local HTTP framing

## Recommended Agent / Model

`arch-ctm` / deep-reasoning: the shared parser, transport parity, and
performance-sensitive buffering design require architectural review. This is a
planning-time recommendation, not a binding assignment.

## Execution Dependencies

Must begin after AI.33 merges into `integrate/phase-ai-31-33`; before coding,
merge-forward that updated target into this sprint branch and verify AI.33 is
an ancestor. It cannot close in parallel because it changes the same local
admission/framing path whose AI.33 behavior is the baseline.

## Dependency Relations

| Sprint | Relation | Rationale |
| --- | --- | --- |
| AI.33 | must_follow | It owns the local admission baseline in the same framing path. |

```yaml
plan_type: sprint_plan
phase: AI
sprint: AI.39
worktree: feature/pAI-s39-buffered-local-http-framing
branch: feature/pAI-s39-buffered-local-http-framing
status: proposed
estimated_scope: one shared framing primitive and two local adapters
```

## Goal

Replace the byte-at-a-time HTTP header reader with one bounded buffered frame
reader shared by Unix UDS and loopback TCP. It finds `\r\n\r\n` in received
chunks, retains bytes belonging to the next frame, and preserves every current
route, size limit, error contract, and local authentication rule.

## Governing requirements and ADRs

- `REQ-CORE-TRANSPORT-001`, `REQ-CORE-TRANSPORT-002B`, and `REQ-CORE-TRANSPORT-005B`
- ADR-032 — unified error contract
- ADR-033 — HTTP endpoint contract
- ADR-035 — canonical write ingress

## Deliverables

1. Introduce one private, bounded `HttpFrameReader` in `atm-core::api` for a
   stream of HTTP request or response frames. Its explicit state owns unread
   bytes; a read operation may consume a chunk larger than one frame and must
   retain the exact surplus for the next read.

   ```rust
   pub(crate) struct HttpFrameReader { unread: Vec<u8> }

   impl HttpFrameReader {
       pub(crate) fn read_request(
           &mut self,
           reader: &mut impl Read,
       ) -> Result<Option<HttpRequest>, AtmError>;
   }
   ```

   Header delimiter search operates on bounded received slices, not one
   `Read::read(&mut [u8; 1])` call per byte. Use `memchr::memmem::Finder` as a
   direct dependency rather than a custom SIMD implementation: its portable
   scalar path is the required baseline and its supported runtime-dispatched
   vector path is only an optional optimization. No target may require
   `target-cpu=native`, an architecture-specific build, or SIMD to parse a
   frame. The chunk buffer must remain small and fixed-capacity/bounded so a
   normal header scan is cache-local; do not make an L1-cache performance claim
   without measured evidence.

2. Preserve current caps and errors: header/body size enforcement, malformed
   start line/header/content-length handling, EOF before a first frame, and
   EOF during a declared body remain typed `AtmError` results. The reader must
   not allocate based on an untrusted content length before enforcing the cap.

3. Route UDS and loopback TCP through the same framing primitive. Both local
   transports support opt-in `Connection: keep-alive`, with the same bounded
   maximum request count and default `Connection: close` behavior. Windows
   remains TCP-only; Unix retains UDS as normal client transport and TCP as a
   parity/diagnostic transport.

4. A failed or disconnected local TCP client is contained to that connection;
   it must not stop the loopback listener or turn later independent requests
   into connection-refused failures.

## Required validation

- Fragment every header delimiter and every body boundary across arbitrary
  chunk splits; parse exactly one valid frame.
- Place two and then 64 complete request frames in one read chunk; prove every
  frame is decoded once, in byte order, with no byte loss or duplication.
- Place a body end and the next frame start line in one read chunk; prove
  surplus bytes are retained.
- Exercise route-specific `POST /v1/atm/messages` writes over UDS and TCP on
  Unix and TCP on Windows.
- Prove default close remains one request per connection; an explicit
  keep-alive serves 1, 2, 8, 16, and 64 writes and closes at the bound.
- Prove a reset/broken-pipe client does not terminate the Unix TCP listener.
- Run all framing correctness cases once with the scalar implementation forced
  (the test-only library capability/feature hook) before any SIMD-enabled run.
  On supported x86_64 and aarch64 runners, run the same corpus with the normal
  runtime-dispatched implementation and assert byte-for-byte identical parsed
  frames and error results. The scalar case is the compatibility gate; SIMD is
  never a substitute for it.
- Run `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --no-fail-fast`, `just lint`, and `just test`.

## Acceptance criteria

- No production HTTP parser discovers headers with a one-byte system-read loop.
- The shared reader preserves over-read bytes exactly across coalesced frames.
- UDS and TCP have equivalent local framing and bounded keep-alive behavior;
  existing close-per-request clients remain compatible.
- Every listed validation passes on its supported platform.

## Non-goals

No HTTP/2, request reordering, remote HTTPS framing redesign, adversarial fuzz
campaign tooling, or canonical write/storage change. AI.40 owns comparative
throughput evidence and the 1,000/s closure. AI.41 and AI.42 own fuzz campaign
tooling and the first real HTTP-framing campaign respectively.
