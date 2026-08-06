# AL.5 — Unix UDS Adapter

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.2 and AL.4.
**unblocks:** AL.8.
**parallel_safe:** AL.3, AL.6, and AL.7 after AL.4 is merged; this sprint owns
only Unix physical setup.

**traceability:** `REQ-CORE-TRANSPORT-001/001B`, `REQ-DAEMON-TRANSPORT-001`,
`005`, `008`, ADR-033.

## Deliverables

1. Implement the Unix UDS connector and listener using Tokio/framework support.
   It performs owner-only path/permission setup and connection setup only.
2. Attach the UDS listener to the AL.2 router and attach the UDS connector to
   the AL.4 shared client. No UDS-specific handler, serializer, result, or
   storage call is permitted.
3. Preserve the current UDS default-selection/no-silent-loopback-fallback
   behavior in existing configuration/client policy; the runtime does not
   invent a second selector.

## Acceptance criteria

- A Unix UDS request reaches the same route body extractor, `ApiRouter`,
  storage trait, hook call site, and result serializer as the in-process route.
- Existing public JSON snapshots are byte/typed-identical to AL.1's oracle.
- No raw socket read/write loop, UDS-only decoder, detached task, or fallback
  route is introduced.

## Required validation

- UDS integration smoke through the active AL runtime
- UDS owner-permission negative test
- route/call-path instrumentation assertion and API snapshot regression test
- shutdown/drain test for a UDS request

## Non-closure

This sprint does not delete the legacy UDS implementation and does not prove
Windows or cross-host transport.
