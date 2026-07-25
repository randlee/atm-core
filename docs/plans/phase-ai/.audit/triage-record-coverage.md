# QA verdict → triage-record coverage

This is a read-only reconciliation of the QA verdict artifacts in
`qa-evidence-master.json` against `.triage/phase-AI/findings` at audit branch
HEAD `8dd3e622` (the merge of `origin/integrate/phase-AI` tip `8e595a03`). No
TTL files were edited by this audit. “Present” means an exact finding ID or a
documented semantic alias exists; “missing” means no canonical record was
found. A semantic alias is still flagged when the verdict introduced a new
ID, because aliases are not safe for automated closure.

## Run-by-run coverage — pre-remediation baseline

This table records the original audit state at baseline HEAD `8dd3e622`, before
the 17 TTL files were merged from `integrate/phase-AI`. The current state is in
the [post-remediation section](#current-post-remediation-state) below.

| Run | Verdict artifact | Expected finding records | Coverage | Missing / stale items |
|---|---|---:|---|---|
| AICH-S9 QA-1 | `ai30-qa1-verdict.md` | 5 | Present | None |
| AICH-S9 recheck | `ai30-recheck-verdict.md` | 5 | Present | All five are marked fixed |
| AICH-S1 QA-1 | `ai21pre-qa1-verdict.md` | 10 | Present | None |
| AICH-S1 FIX-1 | `aich-s1-fix1-recheck-verdict.md` | 5 | Partial | `AI21-QA2-001` has no exact TTL; related to `AI21-IMPORTANT-006` |
| AICH-S2 QA-1 | `ai22-qa1-verdict.txt` | 8 canonical + lost minor | Partial | Lost `RBP-F002` was explicitly never triaged |
| AICH-S2 RBP recovery | `ai22-rbpf002-recovery-report.md` | 2 fresh findings | Present, not recovery | `AI22-IMPORTANT-004/005` exist, but report says they cannot prove recovery of lost `RBP-F002` |
| AICH-S3 QA-1 | `ai23-qa1-verdict.txt` | 10 | Present | None |
| AICH-S3 round 2 | `ai23-round2-verdict.md` | 3 open + carried aliases | Present | Closure/status semantics require review; no new missing canonical ID found |
| AICH-S4 FIX-1 | `ai24-fix-batch-verdict.md` | Original + 3 regressions + 2 important | Present | Several records remain open despite later closure claims |
| AICH-S4 FIX-2 | `ai24-fix2-verdict.md` | 10+ referenced records | Present | Three new AI24 blocks, two AI24 importants, five carry-forward records, and RSH-002 are present; the three AI24 blocks remain open despite “closed” verdict text |
| AICH-S4 FIX-3 | `ai24-fix3-verdict.md` | 5 referenced records | Present | `AI24-IMPORTANT-001` remains open despite “closed”; residual records have mixed severity/status |
| AICH-S5 QA-1 | `ai25-qa1-verdict.md` | Canonical QA-1 set | Present | None material; one process note was explicitly not a code finding |
| AICH-S5 FIX-1 | `ai25-fix1-recheck-verdict.md` | Recheck findings | Partial | `AI25-RBQA-F003` covers the bind-preflight regression; exact aliases `ATM-QA-101/102/103/104/105` are absent, with underlying issues only semantically mapped; `ATM-QA-102` was improperly closed by rescoping |
| AICH-S6 FIX-1 | `aich-s6-fix1-verdict.md` | `AI26-ATMQA-001/002/003` | Present, stale status | `AI26-ATMQA-001` remains `open` although the verdict says it is closed |
| AICH-S6 FIX-2 | `aich-s6-fix2-verdict.md` | `AI26-ATMQA-001/002/003` | Present, stale status | `AI26-ATMQA-001` still lacks a closure update; 002/003 are fixed |
| AICH-S7 QA-1 | `aich-s7-qa1-verdict.md` | 13 unique findings | **Missing** | No `AI27-*` records and no canonical records for the verdict’s ATM-QA/RBP/RBQA/RSH/ARCH IDs |
| AICH-S8 QA-1 | `aich-s8-qa1-verdict.md` | 14 unique findings | **Missing** | No `AI28-*` records and no canonical records for the verdict’s aliases |
| AICH-S10 | No QA verdict located | — | Not run | Assignment exists after the integrate merge; no QA result or triage records |

## Missing AICH-S7 records — baseline state

No exact TTL or canonical text match was found for these S7 findings:

- `ATM-QA-001` / `RBP-F001` / `RBQA-F004` (one merged error-classification issue)
- `ATM-QA-002`, `ATM-QA-003`, `ATM-QA-004`, `ATM-QA-005`
- `RBP-F002`, `RBP-F003`, `RBP-F004`, `RBP-F005`
- `RBQA-F001`, `RBQA-F002`, `RBQA-F003`
- `AI27-RSH-001`, `ARCH-001`, `ARCH-002`

The first bullet is one underlying issue reported under three reviewer labels;
the verdict still needs one canonical record with those aliases, rather than
zero records.

## Missing AICH-S8 records — baseline state

No exact TTL or canonical text match was found for these S8 findings:

- `AI28-QA-001` (function-length lint failure)
- `AI28-QA1-F002` / `AI28-RBQA-F001` / `AI28-ARCH-002` / `RBQA-AI28-2` (missing concurrency/race tests)
- `AI28-QA1-F001` / reopened `AI27-RBQA-F001` (fabricated `next_attempt_at`)
- `AI28-ARCH-001`, `AI28-ARCH-003`, `AI28-ARCH-004`
- `RBQA-AI28-1`, `RBQA-AI28-3`, `RBQA-AI28-4`
- `AI28-RSH-001`, `AI28-RSH-002`, `AI28-RSH-003`, `AI28-RSH-004`
- `AI28-QA1-F003`

## Existing-record status defects

These are not missing files, but they make the triage ledger disagree with the
verdict history:

- `AI24-BLOCK-001`, `AI24-BLOCK-002`, `AI24-BLOCK-003` remain `open` after the
  FIX-2 verdict says all three are closed.
- `AI24-IMPORTANT-001` remains `open` after FIX-3 says it is closed.
- `AI22-BLOCK-004`, `AI22-IMPORTANT-004`, `AI22-IMPORTANT-005`,
  `AI23-IMPORTANT-004`, and `AI23-IMPORTANT-008` remain open after FIX-1
  source-level closure claims; historical closure records are absent.
- AICH-S3 round 2 says `AI23-MINOR-001`, `AI23-MINOR-002`,
  `AI23-MINOR-003B`, `AI23-MINOR-005`, and the carried RSH finding are closed,
  but their records remain open or `fixed_partial` with no recheck closure.
- `AI26-ATMQA-001` remains `open` after both S6 FIX-1 and the later S6 FIX-2
  evidence say it was genuinely closed.
- `AI25-RSH-001` was closed by a later post-FIX-2 triage commit, but was still
  explicitly open in the indexed S5 FIX-1 verdict; the later closure must not
  be back-projected onto that earlier QA result.

## Cross-cutting metadata defect — baseline and current legacy state

The baseline validator reported 63 scoped AICH records with 125 errors and
zero warnings. After the remediation merge, 77 scoped records are selected;
the same 125 errors remain on the 63 legacy records: every legacy record lacks
`triage:foundIn`, and every legacy record except `AI21-BLOCK-001` also lacks
`triage:foundAt`. Thus “record present” does not automatically mean the record
is graph-complete or usable for cursor invalidation.

## Current post-remediation state

The latest audit merge (`c40c689c`, integrating `aa0916e5`) imported 17 TTL
files. Re-running the scoped validator now sees **77 findings, 125 errors, and
0 warnings**: the 17 new records validate, while the legacy records retain the
same missing-field errors.

| Area | Current state |
|---|---|
| AICH-S1 | `AI21-QA2-001` exact record now present and field-complete |
| AICH-S2 | `AI22-RBP-F002` exact record now present; status remains unknown because the original detail was irretrievable |
| AICH-S5 | `AI25-ATMQA-105` exact record now present and field-complete |
| AICH-S8 | 14/14 canonical AI28 records now present and field-complete; aliases are grouped in canonical records |
| AICH-S7 | 0/13 records present on audit/integrate; all 13 now exist on feature-branch HEAD `599c8a67` across commits `be2387cc`, `baaee8ad`, and `599c8a67`, but those commits are not merged into integrate/audit |
| Legacy AICH records | `foundIn` remains missing on all legacy records; `foundAt` remains missing on 62 legacy records |

### New timestamp defect

The newly imported records contain `foundAt` literals that appear to be local
PDT wall-clock values mislabeled with a `Z` UTC suffix. For example,
`AI21-QA2-001` is `2026-07-24T22:48:06Z`, approximately seven hours before
its QA result at `2026-07-25T05:48:10Z`; the same pattern occurs on the S2,
S5, and S8 records. Presence validation passes, but temporal validation must
correct these values from the UTC `result_time_utc` field in the master index.

### S7 source-branch remediation

The S7 records are no longer merely uncommitted: the feature branch
`feature/pAI-s27-peer-delivery-observability` is clean at `599c8a67` and carries
13 canonical AI27 records plus the AI26 closure correction. The commits are
available for a dedicated triage PR, but they are not yet present on
`origin/integrate/phase-AI` or `audit/phase-ai`; therefore the audit branch must
continue to report S7 as missing until that PR is merged.

## Recommended remediation order

1. Merge the dedicated S7 triage PR containing the 13 AI27 records and the
   AI26 closure correction; preserve reviewer aliases in each record.
2. Correct all new `foundAt` values from the master index's UTC result times;
   the presence validator does not catch timezone mislabeling.
3. Append separate closure/resolution records for the stale open findings;
   do not rewrite existing TTL history.
4. Populate legacy `triage:foundIn` and authoritative `triage:foundAt` from the QA
   evidence index. Any TTL changes must be committed and pushed separately
   from narrative audit documentation for cherry-pickability.
