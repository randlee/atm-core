---
title: AI.5 REST router and local UDS
status: proposed
branch: feature/pAI-s5-http-uds-router
worktree: ../atm-core-worktrees/feature/pAI-s5-http-uds-router
target: integrate/phase-AI
---

# AI.5 — REST router and local UDS

## Deliverables

1. Finalize the versioned OpenAPI 3.1 contract in
   `docs/atm-daemon/http-api.md` rooted at `/v1/atm`: messages, message
   detail/read/ack, doctor, teams, and team detail.
2. Implement one REST router from that contract. It calls application handlers
   and never SQLite or nudge code directly.
3. Replace the current custom local framing/client codec with HTTP over UDS.
   The same UDS contract must run on Unix and Windows AF_UNIX.
4. Delete Windows named-pipe mapping/fallback and the custom ATM frame codec
   after all retained operations are represented by the REST contract.
5. Embed/publish the OpenAPI JSON through `atm api spec` and contract-test it
   against router requests/responses.

## Acceptance criteria

- `atm` and graft local daemon calls use HTTP/UDS; no production local client
  uses the retired frame codec.
- Windows CI proves AF_UNIX HTTP message and error behavior.
- No source/dependency reference to named pipes or the retired custom frame
  protocol remains.
- Router tests prove adapters cannot reach storage or post-send boundaries.

## Required validation

Unix and Windows UDS REST integration tests; OpenAPI contract tests; `just
lint`; `just test`; local CLI send/read/ack smoke.
