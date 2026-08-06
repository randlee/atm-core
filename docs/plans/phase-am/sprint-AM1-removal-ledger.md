# AM.1 — Legacy Removal Ledger and Guards

**recommended_agent:** Cipher-311d/fast for inventory, arch-ctm/deep-reasoning
for guard design.
**must_follow:** AL.1 for the final replacement boundary.
**parallel_safe:** AL.2–AL.8 only while AM.1 stays non-production: inventory,
test design, and a branch-local guard that is not merged while legacy modules
are live.
**unblocks:** AM.2 and AM.3.

**traceability:** `REQ-CORE-TRANSPORT-003/006`,
`REQ-DAEMON-TRANSPORT-002/006/007`, ADR-033, ADR-036; all deferred replay text
is dispositioned in the shared traceability record.

## Deliverables

1. Produce the authoritative deletion ledger: module/path, current callers,
   AL replacement, deletion owner, and validation command.
2. Define negative architecture guards for raw framing, peer-only ingress,
   resend/replay, direct SQLite, and daemon references to tmux/graft.
3. Identify test fixtures and Cargo dependencies that become orphaned after
   each deletion.
4. Separate an architecture guard's creation from activation: a guard may be
   drafted before AL activation but may be merged/enabled only in the deletion
   PR that has removed every symbol it forbids.

## Acceptance criteria

- Every production legacy reference has one ledger row or is proven dead.
- Each guard has a representative mutation test that demonstrates failure when
  a prohibited symbol returns.
- No guard is merged early in a way that makes `develop` fail before AL.8.

## Required validation

- `rg` inventory captured in the ledger
- architecture test mutation proof
- review that the TLS quarantine is explicitly excluded from deletion

## Non-closure

AM.1 deletes no live production code and cannot claim the old stack removed.
