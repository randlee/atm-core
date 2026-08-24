# Sprint AQ1.7 — Graft Endpoint Consumer Cutover (Registry Becomes Truth)

Status: draft · Branch: `feature/aq-1-7-graft-endpoint-cutover` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Third graft connection-model sprint (see AQ1.5). Every production consumer
of the file record switches to the daemon-side registry; after this sprint
the file is written but no longer read by anything in-tree.

## Deliverables

1. **Delivery path**: `deliver_published_receiver_hook` and
   `deliver_graft_post_send` (`crates/atm-core/src/graft.rs`) resolve the
   receiver endpoint from the `GraftReceiverEndpointStore` lease instead of
   `read_receiver_record`. On connect failure they call
   `mark_unreachable` (staleness is data, never a wedge) and surface the
   same delivery error they do today — no retry-semantics change.
2. **CLI**: `atm _internal-nudge`
   (`crates/atm/src/commands/internal_nudge.rs`) resolves via a daemon
   query (same envelope as delivery uses) instead of
   `graft_receiver_record_path_from_home`.
3. **Doctor visibility**: `atm doctor --json` gains a graft-receivers
   section (team/agent, endpoint, last_seen age, reachable-at-last-use) —
   read-only over the store; this is the operator's replacement for
   inspecting record files by hand.
4. **Fallback removal is explicit**: no silent file fallback remains in any
   cutover consumer — if the lease is absent, the error says the receiver
   is not registered (actionable: receiver not running or daemon missed
   its announce), never a file-read error. (The file keeps being written by
   AQ1.6's dual-write; it is simply unread.)

## Acceptance criteria

1. Delivery integration test: message to a registered receiver delivers
   via the lease endpoint with the file record DELETED beforehand —
   proving the file is no longer load-bearing.
2. Absent-lease delivery and `_internal-nudge` produce the
   receiver-not-registered error naming (team, agent) — no file-path
   errors anywhere (grep gate: `read_receiver_record` and
   `graft_receiver_record_path` have zero call sites outside
   `atm-core/src/graft.rs`'s own write/republish code and tests slated for
   AQ1.8 deletion).
3. Connect-failure path: dead endpoint → `mark_unreachable` recorded +
   today's delivery error surfaced (no new error shapes).
4. Doctor section renders for a live receiver and for a stale lease
   (deterministic fixture).
5. The daemon-restart / receiver-restart matrix (both orders) passes an
   end-to-end test with zero manual steps — the Hermes profile-reset bug
   class is regression-locked here.

## Required validation

- `cargo test` workspace green on both CI lanes.

## Non-closure / out of scope

- File-record write path and its machinery still exist (deleted in AQ1.8).
- hermes-atm wheel bump (AQ1.9).
- **Version-skew posture**: `atm` CLI and daemon ship and switch as a
  matched release pair in this repo (the daemon-switch tooling enforces
  the pairing), so AC #2's "no file-path errors" claim applies to
  same-version fleets; a pre-AQ1.7 CLI binary against a post-AQ1.8 daemon
  is out of scope and unsupported, like any other unmatched pair.

## Dependencies

- must_follow: AQ1.6 (leases must be populated before consumers depend on
  them). Merge-forward trigger: AQ1.6 dev push.
- parallel_safe: none claimed.
- Downstream: AQ2 must_follow this sprint (queue-graft channel resolves
  endpoints via the registry; recorded in AQ2's Dependencies).
