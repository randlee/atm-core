# Member lifecycle transition boundary

`MemberStateTransitionSink` is the replacement runtime's narrow, sealed
notification boundary for a genuine heartbeat transition into `Idle`. The
runtime records the in-memory observation first, drops its mutex guard, and
then invokes the best-effort sink. It owns no storage or receiver implementation.

The active daemon composition supplies `DrainOnTransitionSink`. Its queue
claim is performed on a blocking task, is guarded by the AQ1 delivery-channel
classifier, and dispatches the rebuilt `NudgeKind::Queue` message through the
ordinary receiver selector. A periodic kind-agnostic recovery sweep enumerates
`PendingNudgeStore::list_pending_members` and repeats the same guarded atomic
claim, so missed heartbeats and process restarts do not lose durable nudges.

The transition callback is an optimization; the pending marker and recovery
sweep are authoritative. Herdr members are left to AQ2.7 and bare-CLI members
are handed off by AQ2.5, so neither path is claimed by AQ3.
