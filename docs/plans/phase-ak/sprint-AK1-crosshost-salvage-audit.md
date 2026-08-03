---
title: AK.1 Cross-host ACK and provenance recovery
status: in_progress
branch: feature/pak-s1-crosshost-ack-provenance-recovery
worktree: ../atm-core-worktrees/feature/pak-s1-crosshost-ack-provenance-recovery
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: Phase AI merge to develop
parallel_safe: false
---

# AK.1 — cross-host ACK and provenance recovery

## Closure

Recover the useful cross-host ACK/provenance behavior from
`fix/crosshost-ack-provenance` onto `integrate/phase-ak`. This is the focused
Phase AK baseline, not an audit-only sprint: retain only independent
provenance or local-IPC fixes; discard legacy peer transport, ad-hoc handoff,
version, and unrelated branch changes.

The observed remote `curl` success proves this branch's receiver path:
certificate provisioning, TLS listener, shared HTTP decode/route, canonical
persistence, host rendering, and nudge. It does **not** prove the branch's
native sender: curl bypasses `HttpsTransport`, `peer_resolution`, the delivery
coordinator, and its per-message thread.

## Type and boundary inventory

| Item | AK.1 role |
| --- | --- |
| `HostName`, `WriteRequest`, `ResponseEnvelope` | Existing canonical provenance, immutable-write, and response values; AK.1 adds no peer-specific DTO. |
| `HttpsListenerSet`, `ListenerSecurity::MutualTls`, `PinnedClientVerifier` | Existing receiver-only TLS boundary proven by curl. AK.1 does not expand it; AK.4 separates the retained plain receiver and AK.6 retires TLS. |
| `AuthenticatedIngress::Peer`, `WriteIngress::Peer`, `validate_write_provenance` | Existing authenticated receiver path, source-host validation, and canonical persistence boundary. |
| `PostCommitWorkKey::LocalNudge`, `PostCommitWorkQueue` | Existing ordinary post-write nudge boundary. AK.1 keeps it as the one receiver notification path. |

No new struct, enum, trait, worker, or transport abstraction is authorized in
AK.1. The keep/discard ledger below is the full change authority.

## Deliverables

1. Publish a keep/discard ledger in this sprint doc, with commit, affected
   paths, decision, and rationale. It is authoritative for the branch.
2. Retain and test the cross-host provenance behavior from `3d83a68d`,
   `05834a4a`, and `8b91eb92`: a receiver-authenticated source hostname
   survives inbound persistence, mailbox rendering, nudge rendering, and a
   host-qualified ACK reply. Keep the current durable immutable outbound write
   only as the delivery record; do not retain worker/recovery control flow.
   AK.4 changes this host to claimed
   trusted-LAN display provenance; it must never authorize routing.
3. Independently assess `9b8c1003` macOS UDS timeout handling. Retain it only
   if its focused regression test proves a real capability-boundary fix; it is
   not cross-host delivery scope.
4. Reimplement/cherry-pick only the retained changes as small focused commits
   on `feature/pak-s1-crosshost-ack-provenance-recovery`; do not merge or
   cherry-pick the branch wholesale.
5. Prove M4→M5 and M5→M4 curl message delivery, receiver host rendering,
   ordinary nudge, host-qualified ACK reply, and reply receipt. This proves the
   preserved receiver/provenance behavior only; it does not claim native peer
   sender success.
6. Discard `d4681010` address-fallback/DNS-thread changes, `8fc886df` handoff
   prose, `c808547e` version normalization, and any merge/CI bookkeeping not
   needed by the retained changes. Do not cherry-pick the branch wholesale.
7. Do not retain `ListenerSecurity::PlaintextTest`/`UntrustedSmoke` as AK.4's
   production mode. It is a separate test ingress classification. AK.4 must
   route its plain trusted-LAN HTTP write through the same canonical receiver
   path as every other write, with host data used only for display provenance.

## Required validation

- Test: inbound authenticated host renders compactly in mailbox and nudge, and
  a host-qualified ACK retains its origin ULID/provenance.
- Live smoke: curl M4→M5 and M5→M4 each prove exact message ULID/body, host
  rendering, receiver nudge, and ordinary ACK reply receipt.
- Smoke: run `just smoke localhost` and `just smoke local-ip` against an
  isolated test home/database; each preserves the same canonical receiver,
  provenance rendering, and ordinary nudge path.
- `git diff --check`, `just lint`, and `just test` pass.

## Dependencies

Start after Phase AI merges to `develop`, on `integrate/phase-ak`. Push AK.1,
then start AK.2 immediately with an AK.1→AK.2 merge-forward; do not wait for
QA. AK.1 PR must merge before AK.2 PR completion. `must_follow` is required
because AK.1 defines the retained provenance baseline. It is not parallel-safe:
AK.2 deletes the same router/peer behavior that AK.1 audits.

## Keep/discard ledger

| Commit | Decision | Rationale |
| --- | --- | --- |
| `3d83a68d` | keep/adapt | ACK origin ULID/destination metadata is needed; discard its recovery-worker coupling. |
| `05834a4a` | keep/adapt | Keep host-aware ordinary nudge rendering; decouple it from TLS-only authentication. |
| `8b91eb92` | keep/adapt | Keep compact full-host mailbox rendering; decouple it from TLS-only authentication. |
| `9b8c1003` | keep | macOS AF_UNIX reports unsupported timeout setup as `EINVAL`; focused regression test proves deadline fallback remains active. |
| `d4681010` | discard | Legacy custom TLS/DNS address fallback. |
| `8fc886df` | discard | Superseded handoff prose. |
| `c808547e` | discard | Unrelated version change. |
| `a398a151` | discard as a commit | Mixed release-version rollback and refactors; extract only a separately justified compile/test fix. |
| merge/CI commits | discard as history | Reproduce only a verified still-required CI configuration change. |
