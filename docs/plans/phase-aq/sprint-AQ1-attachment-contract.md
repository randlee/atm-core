# Sprint AQ1 — Attachment Contract and ADR

Status: draft · Branch: `feature/aq-1-attachment-contract` off
`integrate/phase-aq` (created from `develop` at phase start, per repo
integration-branch policy) · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Contract-first sprint: the envelope field, the on-disk layout, and the ADR
that every later sprint cites. No delivery behavior changes.

## Deliverables

1. **ADR-054 attachments-by-reference** deciding, with rationale: (a) fetch
   mechanism for cross-host bytes over the **accepted** canonical transport
   (ADR-034 HTTPS on 43101, ADR-035 canonical write ingress; ADR-028/031 are
   superseded) — the natural shape is a new authenticated peer HTTP endpoint
   serving content-addressed bytes; sftp/SSH permitted only as an explicitly
   justified fallback; (b) directory handling (reference w/ recursive pull vs
   tar at origin); (c) size limit and over-limit behavior (refuse vs warn),
   relative to the existing `max_message_bytes` config (attachment bytes ride
   outside the envelope, so the limit is a new, separate knob);
   (d) sweeper policy (TTL, on-ack via `mail_message_states.acknowledged_at`,
   or both); (e) known-temp root named from daemon config — `AtmConfig`
   (`crates/atm-core/src/config/types.rs`) has no such key today, so this ADR
   adds one, honoring existing `~/.atm/*` directory conventions
   (`crates/atm-core/src/home.rs`) — no hardcoded paths;
   (f) **pending-delivery semantics for cross-host pull**: under ADR-035,
   receiver-side persistence *is* delivery and ADR-034 rejects outbox/retry
   state, so this decision must pick — and explicitly extend or supersede
   ADR-034/035 for — one of: block the inbound peer write until fetch+verify
   completes, or persist in a new not-yet-deliverable (parked) state hidden
   from the read surface until `local_path` is set; including the storage
   update path that mutates the stored envelope post-fetch and any bounded
   retry scoping; (g) **msg-id allocation vs staging order**: who allocates
   the `AtmMessageId` ULID (CLI vs daemon) and when `attachment_dir(msg_id)`
   staging happens relative to the canonical write, so that cancel (R5/R13)
   provably stages nothing; and (h) **member-host sourcing**: a durable,
   optional `host` registration field is written by the existing
   `teams add-member`/`update-member --host` admin path, validated as a
   `HostName`, and never inferred from heartbeat, DNS, or socket state. A
   non-null remote host must match an enabled `TrustedPeer`; an unresolved
   member remains `host: null` and is not routable through `--from-json` until
   explicitly registered. This is a roster/CLI metadata extension only, not a
   daemon heartbeat or runtime-plumbing sprint.
2. **`Attachment` type** in `crates/atm-storage/src/schema/inbox_message.rs`
   (or sibling module) and optional `attachments` field on `MessageEnvelope`
   (`inbox_message.rs:137`), following the established back-compat patterns
   (`#[serde(default, skip_serializing_if = "Option::is_none")]`, the
   `RawMessageEnvelope` custom deserialize, `extra`-map preservation);
   serde-compatible with envelopes that lack it. `note_source` field
   (`human|drafted|edited`, default `human`) added alongside `text`/`summary`.
   The `sha256` field reuses or generalizes the existing validated 64-hex
   newtype pattern (`TemplateSha`, `crates/atm-storage/src/types.rs`) — there
   is no `Sha256Hex` type today; naming decided in-sprint.
3. **Layout contract** `<known-temp>/atm/<msg-id>/` documented in the ADR and
   expressed as a pure path-derivation function usable by daemon, sweeper,
   and tests.
   The receiver treats `origin_path` as display/audit metadata only; it is
   never a remote filesystem instruction. The fetch endpoint serves bytes
   only from an authenticated, content-addressed staging record.
4. **sc-lint candidate note**: whether R13 (stages side-effect-free except
   final send) is expressible as a lint; finding recorded either way.

## Contract (normative signatures)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attachment {
    pub sha256: AttachmentSha,      // validated lowercase 64-hex newtype
                                    // (TemplateSha pattern; final name per ADR)
    pub size: u64,                  // bytes
    pub name: String,               // basename presented to recipient
    pub kind: AttachmentKind,       // File | Dir (per ADR decision (b))
    pub origin_host: HostName,
    pub origin_path: String,        // absolute path on origin_host
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>, // set by RECEIVING daemon post-fetch
}

pub enum NoteSource { Human, Drafted, Edited }

pub fn attachment_dir(known_temp: &Path, msg_id: &AtmMessageId) -> PathBuf;

pub fn resolve_member_target(
    member_id: &str,
    roster: &dyn RosterStore,
    peers: &dyn PeerConfigStore,
) -> Result<AgentAddress, AddressResolutionError>;
```

`local_path` is never set by the sender; a validator rejects sender-side
population. `origin_path` is never dereferenced by the receiver and must not
escape the configured staging root; it is retained only for operator context.

## Acceptance criteria

1. ADR merged with all eight decisions (a)–(h) closed, none deferred.
2. Round-trip serde tests: envelope without `attachments` deserializes;
   envelope with attachments round-trips; sender-set `local_path` rejected.
3. `attachment_dir()` unit-tested; no other code path constructs the layout.
4. Existing envelope consumers compile and pass unchanged (`just test`).

## Paths to delete

None. AQ1 adds a backwards-compatible optional field and a new configured
attachment root; it must not delete or rename existing mailbox paths.

## Required validation

- `just test` workspace, all three CI lanes (ubuntu, macOS, Windows).
- ADR reviewed by quality-mgr with explicit sign-off on decisions (a)–(h).
- `cargo test -p atm-storage` and the focused envelope/path tests named in the
  implementation PR; the tests must run without a daemon or network.

## Non-closure / out of scope

- No fetch, copy, delivery, sweeper, CLI, or UI behavior.

## Dependencies

- must_follow: none — AQ1 is the contract root and is dispatched from the
  phase branch carrying the verified AO2 baseline.
- parallel_safe: none — every later AQ sprint consumes the schema, path
  function, configured root, and pending-delivery decisions, so parallel
  implementation would create competing contracts.
