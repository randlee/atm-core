# Sprint AQ1 — ATM_TEMP Contract and Transfer-Script Seam (ADR)

Status: draft · Branch: `feature/aq-1-atm-temp-contract` off
`integrate/phase-aq` (created from `develop` at phase start, per repo
integration-branch policy) · PR target: `integrate/phase-aq`
recommended_agent: Cipher-311d · recommended_model: fast

Contract-first sprint: the system-level temp contract, the transfer-script
seam, and the small ADR every later sprint cites. **No envelope change, no
daemon transport machinery, no new storage traits.** Cross-host byte
transfer is a user-configured environment concern (SSH/Tailscale involve IT);
the product's job is a clean seam and an actionable failure message.

## Deliverables

1. **ADR-054 atm-temp-and-transfer-seam** (numbering note: ADR-047 — created
   by phase-AO sprint AO.1 — and ADR-053 are both physically present on the
   `integrate/phase-ao2` worktree, which merges to `develop` before
   `integrate/phase-aq` is cut — a dispatch precondition verified by the
   mechanical gate below — so 054 is the next free number) deciding, with
   rationale:
   (a) **`ATM_TEMP` system contract**: a mandatory environment variable
   naming the ATM scratch root for *all* ATM features, not just Send-To.
   Validation is split by consumer: the **daemon validates eagerly at
   startup** (it owns the sweeper) — unset, unresolvable, non-absolute, or
   non-writable fails boot with an actionable error naming this ADR's doc;
   the **CLI resolves lazily at first actual use** (staging, transfer, any
   future consumer) with the same error — commands that never touch scratch
   space (`atm read`, `atm send` without `--attach`, …) run unaffected in an
   unconfigured shell. Because daemon validation is eager, **this sprint
   owns the environment rollout**: `ATM_TEMP` is set in
   `.github/workflows/ci.yml` (all lanes), the `justfile` test targets, the
   daemon launch/service overlays, and the developer-setup docs, in the same
   PR. Config-value validation rides the same gate: a zero sweep interval or
   zero TTL is rejected at daemon startup (no disable sentinels — disabling
   the sweeper is not a supported configuration). Documented in the daemon
   config docs; no per-feature temp layouts.
   (b) **Sweep policy**: TTL-only, 30 days, over everything under
   `$ATM_TEMP`. No ack coupling, no storage traits, no ADR-018 §3
   involvement. Interval and TTL from daemon config with documented
   defaults (TTL default 30 d).
   (c) **Transfer-script seam**: cross-host transfer is performed by a
   specifically named, user-provided script per destination host, resolved
   at `~/.atm/transfer/<host>` (`.ps1` on Windows). Invocation contract:
   `<script> <host> <transfer-id> <file>...`; on success prints the landed
   destination directory (absolute path on the destination, conventionally
   `$ATM_TEMP/send-to/<transfer-id>/`) to stdout; nonzero exit means the
   transfer failed and stderr is propagated to the user verbatim. A missing
   script produces the canonical error, exactly:
   `File transfer to <host> not enabled. Read
   docs/cross-host-file-transfer.md to set up cross-host file transfer.`
   Any transfer failure fails the whole send invocation closed — zero
   messages sent (R5/R13).
   (d) **Message-text path convention**: the landed paths ride in the
   message *text* via a small documented template ("Attached files (on this
   host): …"). **No envelope schema change in Phase 1**; structured
   `attachments` metadata and `note_source` are recorded as Phase-2
   candidates only.
   (e) **Member-host sourcing**: a durable, optional `host` registration
   field written by the existing `teams add-member`/`update-member --host`
   admin path, validated as a `HostName`, never inferred from heartbeat,
   DNS, or socket state. An unresolved member remains `host: null` and is
   not routable through `--from-json` until explicitly registered. The
   resolution *algorithm* is AQ2's `resolve_picker_recipient` — the single
   canonical implementation; AQ1 owns only the roster field and validation
   rules. (Note: with script-based transfer, `TrustedPeer` enrollment is
   NOT required for a host to be a transfer destination — the script's own
   SSH/Tailscale auth governs; the roster binding only names the host.)
2. **`ATM_TEMP` validation** per decision (a): eager in daemon bootstrap,
   lazy-on-first-use in the CLI, with unit tests for
   unset/unresolvable/relative/unwritable cases and for zero
   interval/TTL rejection; plus the decision-(a) environment rollout
   (CI workflows, justfile, launch overlays, developer docs).
3. **`docs/cross-host-file-transfer.md`**: the setup document the canonical
   error points at — what to create at `~/.atm/transfer/<host>`, the
   invocation contract, and pointers to the shipped examples (AQ3).
4. **sc-lint candidate note**: whether R13 (stages side-effect-free except
   final send) is expressible as a lint; finding recorded either way.

## Contract (normative)

```rust
/// Resolved, validated ATM scratch root (decision (a)). Constructed only by
/// startup validation; all features take it from here — never read the env
/// var ad hoc.
pub struct AtmTemp(PathBuf);

pub fn resolve_atm_temp(env: &dyn EnvSource) -> Result<AtmTemp, AtmTempError>;

/// Staging convention for one Send-To transfer (not a schema — a documented
/// convention shared by the CLI and the example scripts).
pub fn send_to_staging_dir(atm_temp: &AtmTemp, transfer_id: &Ulid) -> PathBuf;
// = $ATM_TEMP/send-to/<transfer-id>/
```

`AtmTempError` inventory (variants normative): `Unset`, `NotAbsolute`,
`Unresolvable` (canonicalization failure), `NotWritable` — each with the
actionable recovery text from decision (a).

Transfer-script resolution and the canonical not-enabled error are CLI-side
(AQ2) behavior specified by decision (c); no daemon code participates in
transfer.

## Acceptance criteria

1. ADR merged with decisions (a)–(e) closed, none deferred.
2. Validation tests: unset / non-absolute / unresolvable / non-writable
   `ATM_TEMP` fails daemon startup and fails the CLI at first scratch-space
   use with the decision-(a) error text; commands that touch no scratch
   space succeed with `ATM_TEMP` unset; zero sweep-interval/TTL config
   rejected at daemon startup; a valid value round-trips into `AtmTemp`.
2a. Environment rollout verified: `ATM_TEMP` set in all three CI lanes and
   the justfile test targets, and pre-existing unrelated test suites pass
   unchanged after the rollout (this is what makes AC 5 satisfiable).
3. `send_to_staging_dir()` unit-tested; no other code path constructs the
   convention.
4. `docs/cross-host-file-transfer.md` exists and contains the invocation
   contract and the canonical error text verbatim.
5. Existing consumers compile and pass unchanged (`just test`).

## Paths to delete

None. AQ1 adds a config contract, one validated newtype, and documentation.

## Required validation

- Mechanical dispatch-precondition gate, run on the freshly cut
  `integrate/phase-aq` head before AQ1 dispatch and recorded in the dispatch
  message: `test -f docs/adr/ADR-047-*.md && test -f docs/adr/ADR-053-*.md`
  (fails fast if the AO2 merge has not reached the cut branch).
- `just test` workspace, all three CI lanes (ubuntu, macOS, Windows).
- ADR reviewed by quality-mgr with explicit sign-off on decisions (a)–(e).

## Non-closure / out of scope

- No transfer execution, CLI surface, sweeper implementation, or UI (AQ2–AQ5).
- No envelope schema change anywhere in Phase 1; `attachments[]` and
  `note_source` are Phase-2 candidates recorded in the ADR's follow-ons.
- No daemon byte-transfer endpoint, fetch/push machinery, or storage-trait
  additions — deliberately rejected; see ADR-054 rationale.

## Dependencies

- must_follow: none — AQ1 is the contract root.
- parallel_safe: none — every AQ sprint consumes `ATM_TEMP`, the staging
  convention, and the transfer-script seam.
