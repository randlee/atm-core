# Sprint AQ1 — Attachment Contract and ADR

Status: draft · Branch: `feature/aq-1-attachment-contract` off
`integrate/phase-aq` (created from `develop` at phase start, per repo
integration-branch policy) · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Contract-first sprint: the envelope field, the on-disk layout, and the ADR
that every later sprint cites. No delivery behavior changes.

## Deliverables

1. **ADR-054 attachments-by-reference** (numbering note: ADR-047 — created
   by phase-AO sprint AO.1 — and ADR-053 are both physically present on the
   `integrate/phase-ao2` worktree, which merges to `develop` before
   `integrate/phase-aq` is cut — a dispatch precondition verified by the
   mechanical gate below; this plan branch predates that merge — so 054 is
   the next free number at AQ1 dispatch time) deciding, with rationale: (a) fetch mechanism for cross-host bytes
   over the **accepted** canonical transport stack — ADR-035 canonical write
   ingress plus ADR-047 layered peer-wire security (`PeerWireMode`, default
   mTLS), which supersedes ADR-034's transport wording and ADR-040
   (Proposed-status header notwithstanding; ADR-054 notes that resolution);
   the byte-fetch endpoint must be demonstrated compatible with ADR-047's
   wire modes (mTLS default, plaintext test mode), and ADR-034 remains the
   single-router HTTP shape reference; ADR-028/031 are superseded — the natural shape is a new authenticated peer HTTP endpoint
   serving content-addressed bytes, registered as a route on the existing
   single `ApiRouter`/listener behind the same TLS/peer-allowlist adapter
   path as canonical writes (never a second listener, port, or service;
   verified structurally, e.g. route-table inspection), with an explicit
   server-side idle-read and total-transfer timeout on the serving side (in
   addition to the client-side fetch deadline) and a bounded in-flight fetch
   concurrency limit per origin host recorded alongside the size limit.
   **Contract-clause reconciliation is part of this decision**: the existing
   `boundaries/atm-http-runtime/http-runtime.toml` `[contracts]` section
   authorizes only `atm-core` `ApiRequest`/`ApiResponse`/`AtmError`
   (`core_contracts_only` gate), so ADR-054 must pick and record one of:
   (1) model the fetch exchange inside the existing `ApiRequest`/
   `ApiResponse` envelope (no TOML contract change — preferred if the byte
   stream fits the one-wire-contract rule), or (2) amend that `[contracts]`
   clause in the same PR that adds the `PeerAttachmentSource` record, naming
   `AttachmentFetchRequest`/`VerifiedAttachment`/`AttachmentFetchError` as a
   narrowly scoped authorized exception, with `boundary-guard` explicitly
   reviewing the widening as a policy relaxation. Silent collision at AQ3
   time is not an option;
   sftp/SSH permitted only as an explicitly justified fallback; (b) directory handling (reference w/ recursive pull vs
   tar at origin), including an explicit cap on file count and recursion
   depth for `kind: dir` attachments and refusal of symlinks that escape the
   directory root during recursive pull (mirroring AQ4's sweeper rail); (c) size limit and over-limit behavior (refuse vs warn),
   relative to the existing `max_message_bytes` config (attachment bytes ride
   outside the envelope, so the limit is a new, separate knob);
   (d) sweeper policy (TTL, on-ack via `mail_message_states.acknowledged_at`,
   or both); (e) known-temp root named from daemon config — `AtmConfig`
   (`crates/atm-core/src/config/types.rs`) has no such key today, so this ADR
   adds one, honoring existing `~/.atm/*` directory conventions
   (`crates/atm-core/src/home.rs`) — no hardcoded paths;
   (f) **pending-delivery semantics for cross-host pull**: under ADR-035,
   receiver-side persistence *is* delivery, its write handler forbids
   pre-persistence transport work ("no host inspection before persistence
   outside PostWriteRouter") and cross-host-specific persistence/nudge
   handlers, and ADR-034 rejects outbox/retry state. The decision must
   therefore be demonstrated compatible with ADR-035's write-ordering and
   Prohibitions in the ADR-054 text itself. The default-candidate shape that
   satisfies both: persist the inbound envelope through the ordinary canonical
   write (attachments present with `local_path: None` as an ordinary field
   state), run fetch+verify strictly post-write, and gate only the *read
   surface projection* of the message on all attachments having verified
   `local_path` — no blocked inbound write, no hidden cross-host-specific
   persistence state. If ADR-054 instead chooses an option that genuinely
   conflicts with ADR-034/035 text, it must add an explicit scoped amendment
   note to ADR-035 (and ADR-034 if the fetch endpoint touches its transport
   responsibilities), mirroring the superseded-by convention ADR-034 already
   uses — a reader of ADR-035 alone must see the exception. Includes the
   storage update path that sets `local_path` post-fetch. Retry scoping must
   be concrete: a stated max-attempt count and backoff-with-jitter policy, or
   an explicit "no retry — single attempt then park"; an ADR that names retry
   without pinning bounds does not close this decision; (g) **msg-id allocation vs staging order**: who allocates
   the `AtmMessageId` ULID (CLI vs daemon) and when `attachment_dir(msg_id)`
   staging happens relative to the canonical write, so that cancel (R5/R13)
   provably stages nothing; and (h) **member-host sourcing**: a durable,
   optional `host` registration field is written by the existing
   `teams add-member`/`update-member --host` admin path, validated as a
   `HostName`, and never inferred from heartbeat, DNS, or socket state. A
   non-null remote host must match an enabled `TrustedPeer`; an unresolved
   member remains `host: null` and is not routable through `--from-json` until
   explicitly registered. This is a roster/CLI metadata extension only, not a
   daemon heartbeat or runtime-plumbing sprint. The resolution *algorithm* is
   NOT an AQ1 deliverable: AQ2's `resolve_picker_recipient` (see AQ2
   "Normative CLI contracts") is the single canonical implementation; AQ1
   owns only the durable roster `host` field and its validation rules;
   and (i) **dedupe storage mechanism**: how a second message referencing an
   already-present `sha256` on the same host shares bytes — hardlink vs copy —
   and how the sweeper detects "still referenced" (link-count vs refcount vs
   ack-state query), so AQ3's reuse path and AQ4's reclamation check implement
   one compatible mechanism. Must state the Windows/NTFS behavior explicitly.
2. **`Attachment` type** in `crates/atm-storage/src/schema/inbox_message.rs`
   (or sibling module) and optional `attachments` field on `MessageEnvelope`
   (`inbox_message.rs:137`), following the established back-compat patterns
   (`#[serde(default, skip_serializing_if = "Option::is_none")]`, the
   `RawMessageEnvelope` custom deserialize, `extra`-map preservation);
   serde-compatible with envelopes that lack it. `note_source` field
   (`human|drafted|edited`, default `human`) added alongside `text`/`summary`.
   The `sha256` field reuses or generalizes the existing validated 64-hex
   newtype pattern (`TemplateSha`, `crates/atm-storage/src/types.rs`) — there
   is no `Sha256Hex` type today; naming decided in-sprint. **Export-surface
   classification is mandatory**: `docs/atm-message-schema.md` restricts the
   Claude-compat JSON surface to a closed additive-field list, so ADR-054
   must state whether `attachments`/`note_source` stay SQLite-only (the
   `pendingAckAt`/`expiresAt` precedent) or join that list via an approved
   `atm-message-schema.md` amendment — silent leakage into the compat export
   is a defect.
3. **Layout contract** `<known-temp>/atm/<msg-id>/` documented in the ADR and
   expressed as a pure path-derivation function usable by daemon, sweeper,
   and tests.
   The receiver treats `origin_path` as display/audit metadata only; it is
   never a remote filesystem instruction. The fetch endpoint serves bytes
   only from an authenticated, content-addressed staging record.
4. **sc-lint candidate note**: whether R13 (stages side-effect-free except
   final send) is expressible as a lint; finding recorded either way.
5. **Storage-boundary governance for the new traits**: ADR-054 carries the
   ADR-018 §3 follow-up amendment to ADR-036 authorizing exactly two new
   optional storage capability traits — `AttachmentDeliveryStore` (AQ3) and
   `AttachmentSweepStore` (AQ4) — with rationale, AND adds a "Phase AQ
   extension" subsection to ADR-036 itself (mirroring its existing "Phase AN
   extension"), so a reader of ADR-036 alone sees the current capability
   list — the same reader-of-the-ADR-alone principle decision (f) applies to
   ADR-035; `docs/atm-storage/
   boundaries.md` and new `docs/boundaries/atm-storage/*.toml` records are
   updated in the same PR (records live at repo-root
   `boundaries/atm-storage/*.toml`, e.g. `peer-config-store.toml`;
   `PeerAttachmentSource`'s record under `boundaries/atm-http-runtime/`),
   `cargo test -p atm-architecture` passes over the new records, and
   `boundary-guard` review is a merge precondition. `PeerAttachmentSource` is a transport-adapter trait
   owned by `atm-http-runtime`, outside the ADR-018 §3 storage cap — ADR-054
   states this placement explicitly. All new cross-crate traits in this phase
   (`PeerAttachmentSource`, `AttachmentDeliveryStore`, `AttachmentSweepStore`,
   plus the roster/peer read traits AQ2 consumes) are fixed/internal
   implementation sets — not plugin extension points — and adopt the ADR-001
   sealed-supertrait pattern; traits intended for `dyn` dispatch declare
   object-safe async methods (boxed-future style per the repo's async-trait
   convention), matching the `&dyn` usage shown in AQ2/AQ3/AQ4 contracts.

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
    pub local_path: Option<StagingPath>, // set by RECEIVING daemon post-fetch
}

// StagingPath: validated newtype constructible only from a path under the
// AQ1-configured staging root (attachment_dir derivation); serializes as a
// plain string. origin_path stays String deliberately: foreign, display/audit
// only, never dereferenced locally.

pub enum NoteSource { Human, Drafted, Edited }

pub fn attachment_dir(known_temp: &Path, msg_id: &AtmMessageId) -> PathBuf;

pub enum SweepPolicy { Ttl, OnAck, Both } // decision (d) selects the default

// The single "is this content still referenced?" authority (decision (i));
// AQ3's reuse path and AQ4's sweeper both call this — no second check.
pub trait AttachmentReferenceCheck {
    fn is_referenced(&self, sha256: &AttachmentSha) -> Result<bool, StorageError>;
}

// The second ADR-018 §3-authorized storage capability (deliverable 5;
// AQ4 consumes as &dyn). Extends — never duplicates — the reference
// authority; per-message ack/expiry state backs the SweepPolicy decision.
pub trait AttachmentSweepStore: AttachmentReferenceCheck {
    /// None = msg-id unknown to storage (dir treated as foreign, left+logged).
    fn message_sweep_state(
        &self,
        msg_id: &AtmMessageId,
    ) -> Result<Option<MessageSweepState>, StorageError>;
}

pub struct MessageSweepState {
    pub acknowledged_at: Option<IsoTimestamp>,
    pub expires_at: Option<IsoTimestamp>,
}
```

Member-to-address resolution is deliberately absent here: AQ2 owns the single
canonical `resolve_picker_recipient` implementation (decision (h) defines only
the roster field it reads). Later sprints needing resolution (e.g. AQ3) reuse
AQ2's function; defining a second resolver anywhere in the phase is a
boundary violation.

`local_path` is never set by the sender; a validator rejects sender-side
population. `origin_path` is never dereferenced by the receiver and must not
escape the configured staging root; it is retained only for operator context.

## Acceptance criteria

1. ADR merged with all nine decisions (a)–(i) closed, none deferred; the
   decision-(f) text demonstrates ADR-035 write-ordering/Prohibitions
   compatibility or carries the required scoped amendment notes in
   ADR-035/ADR-034 themselves.
2. Round-trip serde tests: envelope without `attachments` deserializes;
   envelope with attachments round-trips; sender-set `local_path` rejected.
3. `attachment_dir()` unit-tested; no other code path constructs the layout.
4. Existing envelope consumers compile and pass unchanged (`just test`).
5. New `AtmConfig` keys (staging root, sweep interval/TTL) are validated at
   daemon startup: non-writable/unresolvable staging root, zero interval, or
   zero TTL fail startup with an actionable error (or are documented
   explicitly as disable sentinels) — never deferred to first attach/sweep.
6. The ADR-018 §3 follow-up amendment, `docs/atm-storage/boundaries.md`
   update, and boundary TOML records (deliverable 5) land in the AQ1 PR and
   pass `boundary-guard` review.

## Paths to delete

None. AQ1 adds a backwards-compatible optional field and a new configured
attachment root; it must not delete or rename existing mailbox paths.

## Required validation

- Mechanical dispatch-precondition gate, run on the freshly cut
  `integrate/phase-aq` head before AQ1 dispatch and recorded in the dispatch
  message: `test -f docs/adr/ADR-047-*.md && test -f docs/adr/ADR-053-*.md`
  (fails fast if the AO2 merge has not reached the cut branch).
- `just test` workspace, all three CI lanes (ubuntu, macOS, Windows).
- `cargo test -p atm-architecture` green over the new boundary records.
- ADR reviewed by quality-mgr with explicit sign-off on decisions (a)–(i),
  the ADR-047 wire-mode compatibility statement, and the export-surface
  classification.
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
