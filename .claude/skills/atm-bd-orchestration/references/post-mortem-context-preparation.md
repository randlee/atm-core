# Collect context and run the phase post-mortem

Quality-mgr owns this workflow. Use the complete inventory from
[post-mortem.md](post-mortem.md), not a sample or only open beads. Keep the
inventory and every ID's disposition even when preparation or evaluation fails.

## Collect original findings

Save the reconciled IDs as a JSON array in `finding-ids.json`. From the repository
root, collect the original bead records at the assigned integration commit:

```bash
python3 .claude/skills/atm-bd-orchestration/scripts/post_mortem_context.py --inventory finding-ids.json --repo . --commit <full-integration-sha> --out <run-directory>
```

The source repository passed as `--repo` must contain the pinned SHA locally;
fetch it before collection if necessary. Parallel workers read one shared clone
with `git -C <clone> show/grep` and receive disjoint finding-ID batches; they
do not check out branches or modify that clone.

The collector reads `bd show` and Git only. Its output contains full bead
records, the pinned commit and preparation assignments. Collection errors stay
in its index; repair them before declaring the inventory complete. It does not
infer semantic scope, select source snippets, call JEV, or close findings.

Classify receipt-based determinations before packet preparation: bead metadata,
lead rulings, ADR/policy decisions, deferrals, duplicate/superseded dispositions,
and process-only checks. Verify their actual receipts outside JEV. Mixed findings
use packets for the source-checkable part with receipt obligations listed as
limitations; missing receipts stay pending.

## Prepare each manifest

Use the generated assignment yourself or give disjoint findings to authorized
background workers. Workers return a manifest and evidence notes; quality-mgr
checks coverage before invoking JEV. Workers do not file findings.

1. Quote each original obligation verbatim, identifying its bead field. Read
   description, design, acceptance criteria, and recorded amendments together.
   Resolve precedence from actual rulings; do not turn incidental observations
   into new requirements. A title is a summary, not a substitute for the full
   obligation. For wildcard scope, show why the selected operation belongs to
   the named category.
2. Inspect current code with `git show <sha>:<path>` and scoped `git grep`.
   Select complete deciding functions plus callers, configuration and tests
   when necessary. Follow equivalent replacements; non-ancestry of the original
   fix commit does not prove a lost fix. A documentation claim needs the
   implementation it describes when the obligation promises behavior.
3. Write a manifest matching [post-mortem-jev.md](post-mortem-jev.md): original
   finding text, acceptance predicates, coverage, exact source selections,
   atomic `questions`, and `question_specs` as objects keyed by identical IDs.
   Each predicate cites its original quote. Each presence question addresses
   one concrete proposition with `yes`, `no`, `insufficient`; declare expected
   polarity from the requirement before evaluation. Include separate serious
   fix-issue and advisory quality questions. Style or optional test strength
   must not masquerade as serious correctness defects.
4. Preserve all original obligations. Mark executed-check claims, external
   settings and approval receipts as requiring separate evidence. For oversized
   context (the API limit is 24,000 request bytes), split into named subchecks
   `<finding-id>.part-a`, `.part-b`, etc., recording the base finding ID. Divide
   original obligation IDs across parts and reconcile their full union. If one
   compound obligation is itself too large, give its constituent checks stable
   IDs (for example `P1.row01` through `P1.row14`), retain their common original
   quote and parent `P1`, and require every constituent before supporting `P1`.
   Do not omit rows or weaken the parent obligation. Each part retains separate
   issue and quality questions. Never truncate deciding evidence or relabel partial
   coverage as whole-finding. Do not infer absence from omitted code.

## Prepare packets, evaluate, investigate

```bash
python3 .claude/skills/atm-bd-orchestration/scripts/post_mortem_jev.py prepare --repo . --manifest <manifest.json> --out <packet.json>
python3 .claude/skills/atm-bd-orchestration/scripts/post_mortem_jev.py run --phase phase-d --run-id <run-id> --output-dir .sc/qa-logs <packet.json>
```

`prepare` extracts the selected source from Git and rejects invalid ranges or
oversized requests. `run` retains immutable packets and appends UTC results.
Pass multiple packet paths to process a batch. For another phase, replace
`phase-d`; for repaired context use `--attempt-role context_repair` and retain
the original attempt. A failed attempt remains an error, not a passed finding.
For transient `SANITY.JEV_RESPONSE_INVALID` / "Invalid Choice answer", retry
once with a new run ID, the same packet and attempt role, linking the original
attempt in the investigation record. Persistent failure is an attempted
evaluation error with a manual disposition, never PASS or pending preparation.
Context changes instead use `context_repair`.

Investigate every unsure or flagged result against the original obligation and
source. A worker's determination is a claim: the filing reviewer adjudicates
every proposed defect against the original text and deciding evidence. Compare
the pinned integration source with the identified current stack head so an
already-fixed issue links to its existing bead instead of being filed again.
A fix only on the stack does not count as landed on the pinned integration head. Record a linked determination with finding ID, evaluation IDs, pinned
SHA, evidence, disposition, and UTC timestamp in
`.sc/qa-logs/post-mortem-jev-phase-<x>-investigation.jsonl`. Verify non-code
obligations from receipts. File only confirmed, deduplicated defects; send them
to the lead for fixes and verify only the carried gaps afterward.

## Completion evidence

Reconcile unique finding IDs, including preparation errors and unchecked
receipts. Report question, obligation and finding denominators separately.
The runner's `summary` reports evaluation-attempt routing only. Subcheck scope
or coverage limitations deliberately route to `needs_context`; otherwise the
chosen-answer probability floor is 0.80. Routing is not the model's verdict.
Finding-level aggregation is not yet bundled: reconcile the ledger explicitly,
reporting factual presence support separately from issue/quality investigation
and fully cleared findings. For split findings, require the complete original
obligation union. Never select
only the highest-confidence answer or silently fall back to an older success
when the latest attempt failed. Raw API success, advisory rank and closed bead
status are not proof of a landed fix.

The review completion's required `post_mortem_jev` object contains `status`
(`completed`, `unavailable`, or `not_applicable`), `run_ids`, `jsonl_path`,
`integration_sha`, and `reason`. `completed` records execution, not PASS. Check
that the cited JSONL rows exist at the reviewed SHA. `not_applicable` requires
an inventory showing no code-screenable findings; `unavailable` leaves phase
review pending. The existing post-mortem totals still account for every finding
as verified fixed, justified nonfix, or unresolved.
