# AM.1 — Legacy Removal Ledger and Guards

**recommended_agent:** Cipher-311d/fast for inventory, arch-ctm/deep-reasoning
for guard design.
**must_follow:** AL.1's pushed integration commit for the replacement boundary.
Merge it forward before every AM.1 inventory/fix round; AL.1 PR merge is not
required because AM.1 remains non-production until AL.9 acceptance.
**parallel_safe:** AL.2–AL.8 only while AM.1 stays non-production: inventory,
test design, and a branch-local guard that is not merged while legacy modules
are live.
**unblocks:** only the draft inventory; AM.2–AM.5 require the AL.9 graph and
AM.1's accepted frozen ledger.

**traceability:** `REQ-CORE-TRANSPORT-003/006`,
`REQ-DAEMON-TRANSPORT-002/006/007`, ADR-033, ADR-036; all deferred replay text
is dispositioned in the shared traceability record.

## Deliverables

1. Draft the deletion ledger: module/path, current callers, AL replacement,
   deletion owner, validation command, and call-graph edges. Compute a
   topological deletion order. Numeric sprint labels are never ordering
   authority: AM.2–AM.5 must follow the frozen graph, so no compiled caller is
   left referring to a removed symbol.
2. Define negative architecture guards for raw framing, peer-only ingress,
   resend/replay, direct SQLite, and daemon references to tmux/graft.
3. Identify test fixtures and Cargo dependencies that become orphaned after
   each deletion.
4. Separate an architecture guard's creation from activation: a guard may be
   drafted before AL activation but may be merged/enabled only in the deletion
   PR that has removed every symbol it forbids.
5. Inventory transport observability, capacity/state registries, doctor output,
   dashboards/events, and config keys. Name each consumer and a retain/remove
   disposition. `peer_delivery_observability` and a ledger-confirmed obsolete
   peer capacity/state registry are AM.5 candidates; an active request registry
   is an explicit retain unless the frozen graph proves otherwise.

## Acceptance criteria

- Every production legacy reference has one ledger row or is proven dead; the
  ledger lifecycle is explicit (AM.1 draft → AL.9 actual graph → AM.1 freeze →
  AM.2–AM.5 deletion consumption).
- Each guard has a representative mutation test that demonstrates failure when
  a prohibited symbol returns.
- No guard is merged early in a way that makes `develop` fail before AL.9.

## Required validation

- `rg` inventory captured in the ledger
- architecture test mutation proof
- review that the TLS quarantine is explicitly excluded from deletion

## Non-closure

AM.1 deletes no live production code and cannot claim the old stack removed.
