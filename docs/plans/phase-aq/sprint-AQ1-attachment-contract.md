# Sprint AQ1 — Attachment Contract and ADR

Status: draft · Branch: `feature/aq-1-attachment-contract` off `develop`
(integrate/phase-aq does not exist until this lands) · PR target: `develop`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Contract-first sprint: the envelope field, the on-disk layout, and the ADR
that every later sprint cites. No delivery behavior changes.

## Deliverables

1. **ADR-0xx attachments-by-reference** deciding, with rationale: (a) fetch
   mechanism for cross-host bytes over the canonical transport
   (ADR-031/034/035) — sftp permitted only as an explicitly justified
   fallback; (b) directory handling (reference w/ recursive pull vs tar at
   origin); (c) size limit and over-limit behavior (refuse vs warn);
   (d) sweeper policy (TTL, on-ack, or both); (e) known-temp root named from
   daemon config (key identified, or added if absent — no hardcoded paths).
2. **`Attachment` type** in `crates/atm-storage/src/schema/inbox_message.rs`
   (or sibling module) and optional `attachments` field on `MessageEnvelope`,
   serde-compatible with envelopes that lack it. `note_source` field
   (`human|drafted|edited`, default `human`) added alongside.
3. **Layout contract** `<known-temp>/atm/<msg-id>/` documented in the ADR and
   expressed as a pure path-derivation function usable by daemon, sweeper,
   and tests.
4. **sc-lint candidate note**: whether R13 (stages side-effect-free except
   final send) is expressible as a lint; finding recorded either way.

## Contract (normative signatures)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attachment {
    pub sha256: Sha256Hex,          // content address, lowercase hex
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
```

`local_path` is never set by the sender; a validator rejects sender-side
population.

## Acceptance criteria

1. ADR merged with all five decisions (a)–(e) closed, none deferred.
2. Round-trip serde tests: envelope without `attachments` deserializes;
   envelope with attachments round-trips; sender-set `local_path` rejected.
3. `attachment_dir()` unit-tested; no other code path constructs the layout.
4. Existing envelope consumers compile and pass unchanged (`just test`).

## Required validation

- `just test` workspace, macOS + Windows CI lanes.
- ADR reviewed by quality-mgr with explicit sign-off on decisions (a)–(e).

## Non-closure / out of scope

- No fetch, copy, delivery, sweeper, CLI, or UI behavior.

## Dependencies

- must_follow: none.
- parallel_safe: none (every AQ sprint consumes this contract).
