---
title: AI.10 proof and closeout
status: proposed
branch: feature/pAI-s10-crosshost-proof-closeout
worktree: ../atm-core-worktrees/feature/pAI-s10-crosshost-proof-closeout
target: integrate/phase-AI
---

# AI.10 — proof and closeout

## Deliverables

1. Automate the full local UDS, own-IP HTTPS, two-Mac, and Windows peer proof
   matrix in the readiness record.
2. Prove bidirectional send and ack, duplicate ULID idempotence, nudge, failed
   remote ack non-mutation, unavailable peer, and mTLS/allowlist rejection.
3. Prove that `agent`, `agent:chat-a`, and `agent:chat-b` remain independent
   through local, own-IP, and two-host send/read/nudge/ack flows.
4. Remove obsolete Phase AG cross-host runbooks/claims and reconcile user,
   developer, doctor, and architecture documentation with ADR-032–037. Delete
   the retired custom-frame ICD and deprecated boundary records if AI.6 has no
   remaining consumer; otherwise list the concrete retained consumer as a
   release blocker rather than preserving a historical fallback.
5. Run final architecture gates and publish one accepted-tip evidence set.
6. Prove daemon startup rejects invalid enabled HTTPS configuration without a
   partial listener, and prove bounded UDS/HTTPS shutdown drains or cancels
   tracked work within the configured deadline.
7. Add the two additions-only compatibility gates for the HTTP migration:
   - generate canonical JSON from the live `clap::Command` tree using
     `get_subcommands()` and `get_arguments()`—never parsed `--help` text—for
     every command path and argument/flag name, short/long form, requiredness,
     arity, and default; compare it to a checked-in pre-Phase-AI baseline;
   - put the generator/diff test beside the structural architecture gates and
     allow baseline regeneration only through an explicit `--bless`/update
     command. Bless may append additions only: a removed/renamed baseline
     entry, or changed requiredness/arity/default, hard-fails even under
     `--bless`. An intentional breaking change requires a separately
     human-reviewed, versioned baseline reset before its implementation PR.
     New surface is allowed, but a baseline/live mismatch in either direction
     fails until the same reviewed change updates the baseline;
   - apply the same checked-in additions-only baseline-diff pattern to the
     OpenAPI artifact AI.6 emits: removed path, method, required field, or
     response/error semantic hard-fails, including during baseline update;
     additive paths, operations, and fields are allowed. The OpenAPI gate
     consumes the schema, not rendered prose.

## Proof record

Every evidence row records the exact commit, command, sender address,
recipient address, transport, result, and artifact path. Raw TCP connection
success is never a message-delivery proof.

## Acceptance criteria

- Every readiness row names command, exact commit, hosts, and result.
- Cross-host success is a remote write acceptance plus receiver-visible message
  and nudge; raw TCP reachability is insufficient.
- A displayed `from` address and every reply/ack `to` address preserve a
  present chat-id exactly; no operator training or separate session command is
  needed to reply.
- No prior custom frame, the pre-AI.6 local frame transport, the pre-AI.11
  Windows named-pipe/AF_UNIX transport, peer/replay, duplicate write-path, or
  runtime SQLite escape-hatch source remains. Unix HTTP/UDS remains required
  by REQ-CORE-TRANSPORT-001 and ADR-033.
- The CLI compatibility gate proves the pre-Phase-AI public CLI surface is
  additions-only; the OpenAPI compatibility gate proves the published HTTP API
  is additions-only from its AI.6 baseline.

## Required validation

All readiness commands; `just lint`; `just test`; Windows CI; two-Mac and
Windows-host evidence; local/own-IP/two-host chat-identity proof; final
boundary/error/transport/storage gates; CLI metadata and OpenAPI additions-only
gates.
