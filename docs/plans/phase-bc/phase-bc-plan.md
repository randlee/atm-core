# phase-bc — immutable releases and observability consolidation

| field | value |
| --- | --- |
| user authority | Rand, 2026-09-20 |
| appointed lead | `solar@atm-dev` |
| discovery owners | `arch-ctm@atm-dev` (sc-observability and PR #101 acceptance), `cipher@atm-dev` (release immutability audit) |
| base | `develop` at `11a5ee8bb1acf577931ae3d5427db5208db3ab94` |
| planning branch | `plan/phase-bc` |
| implementation integration branch | `integrate/phase-bc` |
| status | QA-1 failed; documentation/remediation active; phase not complete |
| stack rule | one append-only linear `gh stack`; lowest-risk changes are lowest |

## 1. naming rule

Every new phase and sprint identifier in documents and branch names is
lowercase. This is a new user-directed standard, not a claim about historical
repository naming. This plan therefore uses `phase-bc`, `bc.1`, and branch
names such as `feature/bc1-sc-observability-1-4-0`.

## 2. outcomes

This phase will:

1. qualify atm-core against the published `sc-observability` `1.4.0` family,
   then advance to the coordinated `1.4.1` republish only after it exists;
2. remove duplicated atm-core error and health mapping where stable upstream
   typed APIs preserve the ATM contract;
3. qualify the new log macros and `#[instrument]` attribute without changing
   global logging ownership prematurely;
4. pin and locally qualify the ATM consumer against the accepted `sc-publish`
   revision while recording any inherited or deferred lifecycle boundary;
5. enable immutable releases for `randlee/atm-core` only after compatible
   release code and credentials are live on the release branch; and
6. retain machine-readable proof that the setting, future release, tag, asset
   digests, and attestation agree.

## 3. facts established by discovery

- atm-core now pins `sc-observability` and
  `sc-observability-types` at exact `=1.4.1` workspace versions on bc.6.
- `1.4.1` is published and independently verified, including npm
  `@synaptic-canvas/sc-observability@1.4.1`; the consumed lockfile is the
  authoritative package inventory.
- sc-observability
  [PR #197](https://github.com/randlee/sc-observability/pull/197) carries the
  `1.4.1` execution checklist. `cobs@sc-obs` owns upstream version, manifest,
  and validation
  preparation; `aobs@sc-obs` will notify atm-dev only after the coordinated
  release is actually published and verified.
- The existing installed sc-observability shared workflow remains the `1.4.1`
  release base. sc-publish PR #101 is not adopted for that publication, and
  ATM's broader immutable-release work must not block upstream preparation.
- User authority states that `1.4.1` is code-compatible with `1.4.0` and is
  being republished only because the npm publication leg failed when the
  `1.4.0` release was not immutable. Therefore bc.6 is a dependency, lockfile,
  current version-reference, and evidence repin; it does not budget
  production-code adaptation.
- Upstream reports that PR #197 targets `main` for this version-only recovery,
  candidate provenance is established, and immutable releases are enabled in
  the sc-observability repository. QA/readiness and metadata corrections remain
  in progress; these facts do not close `u5` before registry verification and
  the coordinated published/verified inventory arrive.
- Upstream `1.4.0` adds `sc-observability-log`,
  `sc-observability-log-macros`, `sc-observability-dto`, and
  `sc-observability-binding-runtime`.
- There is no Rust crate named `sc-observability-tokio`. Python has asyncio
  receipt/flush APIs; Rust `#[sc_observability_log::instrument]` is
  executor-agnostic. No sprint may invent a Tokio-specific dependency.
- atm-core's custom tracing bridge owns retained-target selection, field
  allowlisting/redaction, correlation IDs, diagnostic-timeline forwarding,
  counters, process-global subscriber installation, and shutdown behavior.
  `sc-observability-log` is not a drop-in replacement for that contract.
- The GitHub API reports `randlee/atm-core` immutable releases enabled as of
  2026-09-22. This setting observation does not close bc.5: preflight and
  first-release evidence remain pending. Existing releases, including
  `v1.6.0`, remain historical mutable releases.
- Current stable release logic exposes `replace_release_assets`, publishes
  without an explicit draft-first protocol, lacks root per-tag concurrency,
  and permits an existing tag at an ancestor while building newer `main`.
- ADR-050 assigns shared workflows, actions, helpers, prompts, and tests to
  `sc-publish`. atm-core must not locally patch synced shared files.
- The combined-stack `sc-publish` PR #106 source at
  `22137c2da13bf4638b4267b69c6c2f021617da73` is frozen historical evidence,
  not the current u4 qualification source. The active merged PR99/100/101/108
  stack has accepted develop head `98a75ba3e`; a clean consumer layer must
  repin and reinstall that result before u4 closes.

## 4. non-goals and retained ownership

- No legacy or synchronous daemon work exists. Every runtime change targets
  the maintained Tokio/Axum architecture.
- Keep ATM's sealed `ObservabilityPort`, `CommandEvent`, doctor projection,
  diagnostic timeline, retained-field policy, `ATM_LOG` policy, path readiness
  checks, and daemon bootstrap ownership.
- Do not use Python binding runtime or asyncio APIs in the Rust daemon or CLI.
- Do not install a second process-global logger beside the existing tracing
  subscriber without an accepted ownership decision and parity proof.
- Do not treat repository-setting enablement as bc.5 completion while the
  deployed release workflow or its credential contract lacks recorded evidence.
- Do not retrofit or mutate historical releases, tags, or assets.
- External Homebrew/Scoop branch protection, full SLSA/reproducible builds,
  and organization-wide immutable-release policy are follow-up scope unless
  separately authorized.

## 5. upstream gates

| gate | required evidence | blocks |
| --- | --- | --- |
| `u1` | [sc-publish PR #106 combined-stack acceptance](./bc.4-upstream-owner-acceptance.md); PR #101 is historical evidence | bc.4 |
| `u2` | separately authorized bc.5 setting/credential activation evidence; not passed before bc.4 and still incomplete | bc.5 |
| `u3` | inherited/deferred upstream lifecycle work: draft-first publication, replacement refusal, exact tag/build binding, per-tag concurrency, trusted dispatch refs, and digest/checksum verification; not claimed by bc.4 | upstream follow-up; not a bc.4 completion gate |
| `u4` | merged sc-publish develop result at accepted head `98a75ba3e`, plus a clean consumer-layer pin/reinstall and local qualification; **OPEN** pending that evidence | bc.4 |
| `u5` | the coordinated sc-observability `1.4.1` Rust/npm inventory is published and independently verified from the PR #197 checklist and existing installed release workflow; atm-dev has received the upstream completion notice | bc.6 |

No missing upstream gate may be replaced by a local atm-core hotfix to a
synced shared file.

### owner-approved sequencing amendment — 2026-09-23

The owner-approved amendment records that bc.4 local adoption proceeded before
u2 and u3. u2 credential/setting activation was not passed as a bc.4 gate and
belongs to separately authorized bc.5; no administrator appointment or
execution evidence is asserted here. u3's broad draft-first, tag-binding,
concurrency, and digest/checksum expansion was explicitly removed from the
accepted sc-publish scope and is not claimed. The amendment documents the
sequencing decision; it does not close u2, u3, or u4.

The docs lane records QA-004 as pre-existing/upstream-kit-owned and does not
modify it; the upstream tracking issue is
<https://github.com/randlee/sc-publish/issues/92>. RBQA-F201 is rebutted as
false; no boundary record or dependency allowlist change belongs in this
documentation layer.

### upstream ownership and dependencies

| work | phase accountability | external delivery authority and handoff |
| --- | --- | --- |
| PR #106 combined-stack acceptance | `solar@atm-dev` owns gate evidence | sc-publish maintainer delivers; atm-dev supplies exact-head acceptance review |
| repository setting and credential activation | `solar@atm-dev` owns the separately authorized bc.5 gate | no appointment or activation is required to enter bc.4 |
| publication and recovery contract follow-ups | inherited/deferred upstream work; not claimed by bc.4 | sc-publish owns any future implementation and qualification; bc.4 records the boundary only |
| accepted revision and consumer qualification | `solar@atm-dev` owns the consumer gate | sc-publish maintainer hands off the accepted open head; bc.4 records exact-pin, installer-parity, and local consumer evidence without claiming merge or live qualification |
| atm-core pin and evidence | `solar@atm-dev` coordinates; assigned layer writer implements | consumes the `u1`/`u4` evidence and records the `u2`/`u3` boundary |
| repository setting activation | `solar@atm-dev` owns the gate and evidence | a named atm-core repository administrator executes only with separate explicit authorization from Rand in bc.5 |
| sc-observability `1.4.1` publication | `solar@atm-dev` owns dependency tracking | `cobs@sc-obs` owns version/manifests/validation preparation under PR #197; `aobs@sc-obs` sends the published/verified notice; the existing installed workflow is the release base and PR #101 is not adopted |

The expanded shared publication contract is not implicitly assigned to the
sc-observability team. Each upstream owner must explicitly accept their item.
Other consumers retain their own adoption authority: sc-compose and wyvern own
their rollout, while the sc-lint team owns sc-lint rollout.

The sc-publish maintainer's PR #106 acceptance and evidence handoff are recorded
in `docs/plans/phase-bc/bc.4-upstream-owner-acceptance.md`. Setting and
credential activation remain a separately authorized bc.5 activity.

## 6. sprint sequence and linear stack

| sprint | accountable owner | branch | risk | relation | authoritative doc |
| --- | --- | --- | --- | --- | --- |
| `bc.1` | `arch-ctm@atm-dev` | `feature/bc1-sc-observability-1-4-0` | low | stack bottom | [published 1.4.0 qualification](./sprint-bc.1-sc-observability-1.4.0.md) |
| `bc.2` | `arch-ctm@atm-dev` | `feature/bc2-typed-observability` | low/medium | `must_follow bc.1` | [typed consolidation](./sprint-bc.2-typed-observability.md) |
| `bc.3` | `arch-ctm@atm-dev` | `feature/bc3-log-macro-qualification` | medium | `must_follow bc.2` | [macro and bridge qualification](./sprint-bc.3-log-macro-qualification.md) |
| `bc.4` | `cipher@atm-dev` | `feature/bc4-sc-publish-immutable-consumer` | medium/high | `must_follow bc.3`; consumes `u1`/`u4` evidence and records the `u2`/`u3` boundary | [qualified consumer adoption](./sprint-bc.4-sc-publish-immutable-consumer.md) |
| `bc.5` | `solar@atm-dev` | `evidence/bc5-immutable-release-activation` | operational/high | separate authorized evidence gate after bc.4 compatibility reaches every release writer | [setting activation and proof](./sprint-bc.5-immutable-release-activation.md) |
| `bc.6` | `arch-ctm@atm-dev` | `feature/bc6-sc-observability-1-4-1` | low, availability-gated | `must_follow bc.1`; priority append-next when `u5` closes; never gated by bc.4 or `u1`–`u4` | [1.4.1 final repin](./sprint-bc.6-sc-observability-1.4.1.md) |

While `u5` remains open, the default implementation chain is:

```text
integrate/phase-bc
  -> feature/bc1-sc-observability-1-4-0
  -> feature/bc2-typed-observability
  -> feature/bc3-log-macro-qualification
  -> feature/bc4-sc-publish-immutable-consumer
```

`bc.5` is a separately authorized operational evidence gate, not a layer in
the implementation PR stack. It branches from the released compatible head
and cannot execute until compatible code is present on every reachable
release writer.

`bc.6` is independent of the sc-publish/immutable-release lane. When `u5`
closes, it becomes the next append-only layer above the current frozen top,
before any not-yet-started higher-risk layer. Later layers branch from bc.6.
For example, if publication completes before bc.2 starts, the chain is
bc.1 → bc.6 → bc.2 → bc.3 → bc.4; if bc.2 is already frozen, the chain is
bc.1 → bc.2 → bc.6 → bc.3 → bc.4. No frozen layer is reordered or rewritten,
but bc.6 is never held for bc.4 or gates `u1` through `u4`.

All branch creation uses `/sc-git-worktree` from the immediate parent. The
primary checkout remains on `develop`. The lead owns non-interactive `gh stack`
operations; developers own exactly one worktree/layer. Lower layers freeze when
their task closes.

### closure evidence ledger

| gate or sprint | authoritative artifact |
| --- | --- |
| `u1` | `docs/plans/phase-bc/bc.4-upstream-owner-acceptance.md` (PR #101 remains historical only) |
| `bc.1` | `docs/plans/phase-bc/bc.1-1.4.0-qualification.md` |
| `bc.2` | `docs/plans/phase-bc/bc.2-retained-mapping-and-reduction.md` |
| `bc.3` | `docs/plans/phase-bc/bc.3-log-surface-decision.md` |
| `u2` | `docs/plans/phase-bc/bc.5-credential-preflight.md` |
| `u3` | inherited/deferred upstream lifecycle record in `docs/plans/phase-bc/bc.4-upstream-owner-acceptance.md`; not claimed by bc.4 |
| `u4`, `bc.4` | `docs/plans/phase-bc/bc.4-sc-publish-consumer-qualification.md` (historical receipt superseded; clean repin/reinstall pending) |
| `bc.5` credential | `docs/plans/phase-bc/bc.5-credential-preflight.md` |
| `bc.5` setting/release | `docs/plans/phase-bc/bc.5-immutable-release-evidence.md` |
| `u5`, `bc.6` | `docs/plans/phase-bc/bc.6-1.4.1-requalification.md` |

Future artifact paths are contractual outputs and need not exist before their
gated work executes. The phase cannot close with a missing ledger artifact.

## 7. immutable release invariant

A future atm-core release is acceptable only when all are true:

1. repository API reports immutable releases enabled;
2. release assembly is draft-first and publishes once after the full manifest
   asset set is present;
3. tag commit, gated source SHA, build checkout SHA, and release receipt SHA
   are identical;
4. published assets cannot be replaced or deleted and the tag cannot be moved;
5. API asset digests and downloaded hashes agree with `checksums.txt`;
6. `gh release verify <tag>` succeeds for the future immutable release;
7. retry of the same tag is verify-only and never rebuilds or mutates bytes;
8. production dispatch code comes from the reviewed trusted ref and qualified
   sc-publish revision; and
9. an indeterminate policy, permission, API, digest, or receipt result fails
   closed before tag, registry, or channel mutation.

Exact tag/build equality deliberately replaces the existing ancestor-tag
recovery behavior. It requires an explicit upstream policy decision and tests;
it does not rewrite any pre-enablement historical release. The discovery audit
specifically identified mutable, tag/build-divergent `v1.6.0`.

Pre-publication gates cover the repository setting, trusted workflow,
provenance, credential, exact source, and complete draft. The immutable receipt
and GitHub attestation exist only after publication: they gate downstream
registry and channel promotion, never the first publication itself.

## 8. observability boundary invariant

Any reduction in atm-core code must preserve:

- emitted retained JSONL semantics and golden fixtures;
- CLI durability (`log` plus explicit `flush`);
- daemon non-blocking admission and bounded shutdown;
- doctor health/error codes and recovery guidance;
- retained field allowlist, redaction, target selection, correlation IDs,
  counters, and diagnostic-timeline behavior; and
- one process-global logging owner.

Applicable Rust practices are RBP-001, RBP-002, RBP-003, RBP-004, RBP-006,
RBP-008, and RBP-010. ADR-001 must be read before any sealed-boundary change;
this plan authorizes none.

## 9. phase validation and merge gate

- Every sprint's required validation passes at its frozen head.
- QA runs once on the top implementation layer per the append-only stack rule.
- The top head passes the repository's full local gate set and exact landing
  CI against `integrate/phase-bc`.
- No immutable-release setting write, tag, release, registry publish, or
  external channel mutation occurs without the explicit sprint gate and
  operator authorization.
- phase closure requires proof for the setting and the first future immutable
  release; enabling alone is not closure.
- just validate publish dry-run failures caused solely by the workspace version still equalling the published 1.6.0 are deferred to the prerelease patch bump (Rand, 2026-09-23).
- bc.5 remains enabled but open: the first immutable-release proof is deferred to the release from main and is not open phase work or a blocker to landing into integrate/phase-bc.
