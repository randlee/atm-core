# Sprint bo-11: lite templates — the stack block and the pour block

Repo: **atm-bd-orchestration**. Branch `sprint/bo-11-lite-templates`. One dev. Owned paths: `skills/atm-bd-lite/templates/`, `skills/atm-bd-lite/tests/test_templates.py`, `skills/atm-bd-lite/tests/fakes/` (shell fakes of `bdc` and `gh`), and the "Dispatch" section of `skills/atm-bd-lite/SKILL.md`.

Source paths are `skills/atm-bd-lite/templates/<file>` (ANC-012). Installed, they sit at the same place inside the installed skill directory (ANC-031: "inside that profile's own skill directory"): `.claude/skills/atm-bd-lite/templates/<file>`. That is how orchestration's templates are installed today; the phase files name `.claude/skills/atm-bd-orchestration/templates/dev-template.xml.j2`.

## Goal
- intent.md: "Python is for gates. Sequences are for agents." Anything that is do A, then B, then C, where any step can fail, is a fenced bash block in the agent's template. The agent runs it, reads the error when a step fails, and runs it again.
- The stack bead is the ledger (ANC-069). Its assignee is the lock and its `layers` are the record. Only the dev/fix template's stack block writes it, through `bdc`, while holding the lock.
- QA findings are poured by the QA agent through `bdc` in the QA template's pour block. No hook creates a bead.
- The hooks only check what these blocks did: T-CLOSE-02 `STACK_NOT_RECORDED` and T-CLOSE-04 `FINDINGS_NOT_POURED` (`hooks/task-close.md`). Each block writes exactly the facts those rows read.

## Dependencies
- **bo-9** (`atm-bd-types` 0.2.0): the metadata key names and shapes the blocks write and the hooks read. These are `BeadMeta.work` (`branch`, `worktree`, `sprint`), `BeadMeta.stack_bead`, and `Stack` (`trunk`, `layers`), where each layer is a `Layer` `{base, branch, head, pr}` with `head` as 40 hex digits (`Sha.__str__`). `test_templates.py` decodes the recorded ledger with `atm_bd_types.Stack.decode` and calls `layer_for(branch)`. This is the same lookup T-CLOSE-02 makes.
- **bo-12 gate**: nothing from it is consumed (see "The bo-12 gate on templates").
- **bo-10**: none. bo-10's close hook reads the phase-file template keys this sprint defines (deliverable 5). That is a dependency of bo-10 on bo-11, not the other way round.

## bdc verbs the blocks use (beads-cache `develop` @ `5196acd`, contract `docs/bd-cache-cli/contract.md` 0.2.0)
The `bdc` contract has no `lock`, `record` or `pour` subcommand. Each step maps onto a real verb, as follows:

| block step | bdc command | beads-cache behaviour | exists today |
|---|---|---|---|
| lock | `bdc issues update <stack> --actor <me> --idempotency-key <k> --assignee <me> --expected-assignee ""` | One journaled update. The `expected_assignee` guard (`""` decodes to `Assignee::Unassigned`, `crates/bd-cache-api/src/ops/beads.rs` `guards_take_the_wire_assignee`) is checked when the write is journaled and again at flush. A held lock gives `PRECONDITION` (exit 3), with `details.unmet.assignee {expected, actual}` naming the holder. | yes (verb). Binary not published, see below. |
| read top / layers | `bdc issues get <stack>` | The cached row with pending writes replayed. `data.metadata.trunk` and `data.metadata.layers`. | yes |
| record + release | `bdc issues update <stack> --actor <me> --idempotency-key <k> --set-metadata '{"layers":[…]}' --assignee "" --expected-assignee <me>` | One journaled update that sets the whole `layers` array and unassigns. It is guarded on the holder, so a lost lock is `PRECONDITION` and nothing is written. `metadata.set` replaces the key, so the block reads `layers` while it holds the lock and writes the full array back. | yes (verb) |
| pour (create) | **none** | `createIssue` is "not served (`UNSUPPORTED`)" (contract §5.4). | **no**: randlee/beads-cache#104 (OPEN) |
| pour edge | `bdc dependencies add <blocked> <finding> --type blocks` | One journaled edge (exactly one per call). | yes |

- **The `bdc` binary is not released.** `prerelease/v0.1.0` ships only the `bd_cache_rs` wheels. The README says the CLI channels "publish nothing" until the first stable release, which is blocked on randlee/sc-publish#135. No beads-cache issue tracks a `bdc` binary prerelease (see the questions below). This blocks running either block live (bo-14). It does not block this sprint: the tests run the blocks against a fake `bdc` that follows the contract's envelopes.
- **#104 blocks the pour block's create step only.** The stack block, the dispatch block, the close lines and the pour block's `dependencies add` step do not wait on it.
- **The lock is atomic within one cache file.** The guard is checked against the local cache view when the write is journaled, and again at flush against bd. Two agents on one host share the cache file. On different hosts, the flush check is the only arbiter.

## Deliverables
1. **Dev and fix templates** `skills/atm-bd-lite/templates/dev-template.xml.j2` and `fix-template.xml.j2` (ANC-026, 055, 057, 060, 067, 069). Required variables are exactly `task_id`, `phase` and `profile`. Every other fact is looked up by the agent at run time, with `bdc issues get {{ task_id }}`. Each template contains, in order:
   - **Profile refusal** (ANC-026, template part): the first line refuses to render unless `profile == "lite"`. sc-compose 1.7.0 has no raise builtin, so the refusal is a forced render error: `{% if profile != "lite" %}{{ ("PROFILE_MISMATCH: phase file profile is '" ~ profile ~ "', not lite") | int }}{% endif %}`. Measured on 2026-11-01 with `sc-compose render --strict`: `profile=lite` exits 0, and `profile=orchestration` exits 2 with `template rendering failed: invalid operation` and no stdout.
   - **Stack block**: one fenced bash block, the only fenced block in the template whose lines begin `bdc` or `git` (ANC-067, 069). Read lines are command substitutions. Exactly four lines begin with `bdc`/`git`, each tagged with its step:
     ```bash
     set -euo pipefail
     ME="$ATM_IDENTITY"; BEAD={{ task_id }}
     ROW=$(bdc issues get "$BEAD")
     BRANCH=$(jq -er '.data.metadata.work.branch' <<<"$ROW"); WT=$(jq -er '.data.metadata.work.worktree' <<<"$ROW")
     STACK=$(jq -er '.data.metadata.stack_bead' <<<"$ROW")
     bdc issues update "$STACK" --actor "$ME" --idempotency-key "$(uuidgen)" --assignee "$ME" --expected-assignee ""   # lock
     LEDGER=$(bdc issues get "$STACK")
     TOP=$(jq -er --arg b "$BRANCH" '.data.metadata as $m | ([$m.layers // [] | .[] | select(.branch != $b)] | last | .branch) // $m.trunk' <<<"$LEDGER")
     cd "$WT"
     git fetch origin "$TOP" && git rebase FETCH_HEAD   # rebase
     git push --force-with-lease origin "HEAD:$BRANCH"   # push
     HEAD_SHA=$(git rev-parse HEAD)
     PR=$(gh pr view "$BRANCH" --json number -q .number) || PR=$(gh pr create --base "$TOP" --head "$BRANCH" --fill | sed 's#.*/##')
     gh pr edit "$PR" --base "$TOP"
     LAYERS=$(jq -c --arg b "$BRANCH" --arg base "$TOP" --arg h "$HEAD_SHA" --argjson pr "$PR" '.data.metadata.layers // [] | (map(.branch) | index($b)) as $i | {base: $base, branch: $b, head: $h, pr: $pr} as $l | if $i == null then . + [$l] else .[$i] = $l end' <<<"$LEDGER")
     bdc issues update "$STACK" --actor "$ME" --idempotency-key "$(uuidgen)" --set-metadata "{\"layers\":$LAYERS}" --assignee "" --expected-assignee "$ME"   # record
     ```
     The prose after the block says three things. If a step after the lock fails, fix the cause and rerun from the `# rebase` line; do not rerun the `# lock` line, because you hold the lock. To give the lock back without recording, run the single command `bdc issues update "$STACK" --actor "$ME" --idempotency-key "$(uuidgen)" --assignee "" --expected-assignee "$ME"`. A `PRECONDITION` on `# lock` names the holder in `details.unmet.assignee.actual`: wait for that holder; do not take the lock.
   - **Close line** (ANC-057): exactly one `atm task close` in the template: `atm task close {{ task_id }} completed --stdin <<'EOF'` followed by the report. The prose says that a dev that cannot do the work runs the same command with `refused` in place of `completed` and the reason as the body (T-CLOSE-01). No `atm send`, no `atm ack`.
   - No close-vars, no vars file, no per-task state file. The only file the agent writes is its code (ANC-055). No `gh pr list`, `gh pr checks`, `gh run watch` or `--watch`. CI is not the dev's to wait on (ANC-060).
2. **QA template** `skills/atm-bd-lite/templates/qa-template.xml.j2` (ANC-026, 055, 060, 067). It has the same required variables (`task_id`, `phase`, `profile`), the same profile refusal and the same single close line. It contains:
   - **Target read** (fenced bash, reads only). The target layer is the layer recorded for the QA bead's closed dev/fix blocker (`bdc dependencies list --issue-id {{ task_id }}`, then `bdc issues get` on the blocker for `work.branch`), or the lead's `checked_bead` if it is set. The ledger's layer for that branch gives `head` and `pr`. This is the same rule T-CLOSE-03/04 use ("Target layer", `hooks/task-close.md`). QA reviews that `head`.
   - **Pour block**: one fenced bash block with one iteration per finding. Each finding's id is `<QA bead id>-f<N>`, with N counted from 1 in report order. The id is caller-supplied, as #104 requires ("the caller supplies the id ... must match the database prefix"). The create step is written as `bdc issues create …` **only once #104 ships its CLI form**; until then the line is the marker `# BLOCKED: beads-cache#104 (create)` and the deliverable stays open. What each poured bead carries, from the spec rows that read it:

     | set by the pour | value | spec row that reads it |
     |---|---|---|
     | status | `open` | T-ASSIGN-02, T-START-01 (assignable and startable) |
     | assignee | `""` | T-ASSIGN-02: the lead assigns it |
     | label `stage:fix`, label `phase-<phase>` | | T-CLOSE-02: "stage dev, fix" |
     | `metadata.work` | `{branch: "fix/<sprint>-f<N>", worktree: "<worktree root>/fix/<sprint>-f<N>", sprint: <sprint id>}` | T-CLOSE-02: "ledger has a layer for `work.branch`"; the stack block reads `work.branch` and `work.worktree` |
     | `metadata.stack_bead` | the QA bead's `stack_bead` | T-CLOSE-02 reads `cache.get(stack_bead)` |
     | edge, blocking finding | `bdc dependencies add <sprint id> <finding id> --type blocks` | T-CLOSE-04: "each blocking finding's poured bead exists and blocks the sprint bead" |
     | edge, non-blocking finding | `bdc dependencies add <phase bead> <finding id> --type blocks` | T-CLOSE-04: "each non-blocking one blocks the phase bead" |

     The sprint id is the QA bead's `metadata.work.sprint` (bo-9 `Work.sprint`). The phase bead is the phase file's `root`. Both lookups are pending a question to Rand (Q2).
   - **Verdict and close**: the report ends with the one fenced block from the `QA_VERDICT` fix text: `{"verdict": "PASS" | "FINDINGS", "sha": <target head>, "findings": [poured ids]}`. The QA closes with the single line `atm task close {{ task_id }} completed --stdin`.
3. **Dispatch** (ANC-026, 055, 060, 067): the "Dispatch" section of `skills/atm-bd-lite/SKILL.md` is one fenced bash block that the lead runs for a dev, fix or QA bead:
   ```bash
   set -euo pipefail
   PHASE=<x>; BEAD=<bead id>; AGENT=<member>; STAGE=<dev|fix|qa>
   PF=".atm-bd/phase-$PHASE.toml"
   PROFILE=$(sed -n 's/^profile *= *"\(.*\)"$/\1/p' "$PF")
   TEMPLATE=$(sed -n '/^\[templates\]/,/^\[/s/^'"$STAGE"' *= *"\(.*\)"$/\1/p' "$PF")
   atm task assign "$AGENT" --task-id "$BEAD" --template "$TEMPLATE" --vars "$(jq -nc --arg t "$BEAD" --arg p "$PHASE" --arg f "$PROFILE" '{task_id: $t, phase: $p, profile: $f}')"
   ```
   The profile comes from the phase file only. An orchestration phase file has no `profile` key, so `PROFILE` is empty and the template refuses (deliverable 1). The vars are inline JSON, never a file (ANC-055). `{task_id, phase, profile}` is the whole variable contract, so the lite cron's QA assign (ANC-074) passes the same three.
4. **Close templates** named by the hook's `handled` replies (T-CLOSE-01..04): `refuse.md.j2` (`reason`), `dev-close.md.j2` (`branch`, `head`, `pr`, `base`), `qa-pass.md.j2` (`sha`) and `qa-findings.md.j2` (`sha`, `ids`), all under `skills/atm-bd-lite/templates/`. Their required variables are exactly the hook's evidence and nothing else. They contain no commands.
5. **Phase-file template keys**: lite phase files carry `profile = "lite"` and `[templates]` with `dev`, `fix`, `qa`, `refuse`, `dev_close`, `qa_pass` and `qa_findings`. Each value is an installed path `.claude/skills/atm-bd-lite/templates/<file>`. `skills/atm-bd-lite/templates/phase-file.example.toml` is the example that the tests and bo-14 copy.

## Acceptance (every line: command, then expected output)
- [ ] ANC-026: `uv run pytest skills/atm-bd-lite/tests/test_templates.py -k profile` passes. For each of dev, fix and QA, the test renders the template with `sc-compose render --file <t> --var-file <vars> --strict`. The vars come from running the deliverable 3 `PROFILE` line on `tests/fixtures/phase-lite.toml` (exit 0, non-empty stdout) and on `tests/fixtures/phase-orchestration.toml` (no `profile` key; exit 2, empty stdout), and on a file with `profile = "orchestration"` (exit 2, empty stdout).
- [ ] ANC-055: `grep -rnE 'close-vars|close_vars|vars\.json|write_text|open\(' skills/atm-bd-lite/templates skills/atm-bd-lite/SKILL.md` prints nothing, and `pytest -k required_variables` asserts each dispatch template's front-matter `required_variables` equals `[task_id, phase, profile]`.
- [ ] ANC-057: `for t in dev fix qa; do grep -c 'atm task close' skills/atm-bd-lite/templates/$t-template.xml.j2; done` prints `1` three times. Each matched line contains `completed --stdin`. `grep -nE 'atm send|atm ack' skills/atm-bd-lite/templates/*-template.xml.j2` prints nothing.
- [ ] ANC-060: `grep -rnE 'gh pr list|gh pr checks|gh run watch|--watch' skills/atm-bd-lite/templates skills/atm-bd-lite/SKILL.md` prints nothing.
- [ ] ANC-067: the test extracts the fenced bash blocks from the rendered dev and fix templates. The lines matching `^(bdc|git) ` are exactly four, with tags `# lock`, `# rebase`, `# push`, `# record` in that order: `grep -oE '# (lock|rebase|push|record)$'` prints `# lock`, `# rebase`, `# push`, `# record`. The rendered QA template has one fenced block with a `bdc dependencies add` line and the create step (the #104 marker until it ships). `grep -rlnE 'git rebase|git push|bdc issues update|bdc dependencies add' --include='*.py' --exclude-dir=tests skills/atm-bd-lite` prints nothing.
- [ ] ANC-069, sequence: `uv run pytest skills/atm-bd-lite/tests/test_templates.py -k stack_block` passes. The test runs the rendered stack block with `bash` against a local bare-repo origin and the shell fakes in `tests/fakes/` (`bdc`, `gh`), which record their argv and return contract envelopes. Asserted:
  - the recorded argv sequence is `bdc issues get <bead>`, `bdc issues update <stack> … --assignee <me> --expected-assignee ""`, `bdc issues get <stack>`, `git` rebase onto the ledger top, push, `gh pr view`, `gh pr edit --base <top>`, `bdc issues update <stack> … --set-metadata {"layers":[…]} --assignee "" --expected-assignee <me>`;
  - `atm_bd_types.Stack.decode` of the fake's final stack row gives `layer_for(<branch>) == Layer(base=<top>, branch=<branch>, head=<40-hex HEAD>, pr=<n>)`. This is the fact T-CLOSE-02 checks.
- [ ] ANC-069, lock held: the same test with the fake `bdc` answering `# lock` with `PRECONDITION` (exit 3, `unmet.assignee.actual = "other"`). The block exits 3, and the recorded argv has no `git rebase`, no `git push` and no second `bdc issues update`.
- [ ] ANC-069, first layer: with `layers` absent, `TOP` is the stack bead's `trunk` and the recorded layer's `base` is the trunk (bo-9 `Stack` docstring: "A new stack has a trunk and no layers").
- [ ] Pour edges: `pytest -k pour_block` runs the rendered pour block against the fake. For one blocking and one non-blocking finding, the recorded argv has `bdc dependencies add <sprint> <qa>-f1 --type blocks` and `bdc dependencies add <phase root> <qa>-f2 --type blocks`. The create assertion is `xfail(strict=True, reason="beads-cache#104")` until #104 ships.
- [ ] Deliverable 4: `pytest -k close_templates` renders each close template with exactly the hook's evidence variables (exit 0) and with one variable missing (strict render, exit 2).

## The bo-12 gate on templates
The draft gate `design/lite-code/atm-bd-lite-gate.py <hook.py> <hooks/spec.md>` parses one hook file with `ast` and one spec. Its deterministic checks are ANC-052 (rows == arms), ANC-064 (writes), ANC-065 (`stack_bead` only read), ANC-066 (no subprocess, CLI adapter, `bd`/`gh`/`git`) and ANC-070 (≤ 100 lines). Its two Jev question sets ask whether every row is implemented (ANC-053) and every arm is listed (ANC-054), both on hook source. **It reads no template.** On a push that touches only `skills/atm-bd-lite/templates/` (brief §B.6), it re-runs those hook checks and nothing else. The template anchors (026, 055, 057, 060, 067, 069) are held by `test_templates.py` in `just test`. A template change that breaks a block's shape fails there, not in the gate. See Q4.

## Not in this sprint
Hooks (bo-10). Re-review QA beads after a fix (Q3). Any script that runs the blocks; there is none (ANC-067 fail fixture).

## Validation
`just lint`, `just test` (includes `uv run pytest skills/atm-bd-lite/tests/test_templates.py`).

## Open (Rand)
- Q1: ANC-067's check names `bdc lock` / `bdc record` / `bdc pour`. bdc has none of these. The blocks use `bdc issues update` with the step tags `# lock` / `# record`, and the acceptance greps the tags. Should the check be re-worded to the real verbs, or should beads-cache add named verbs?
- Q2: T-CLOSE-04 needs the sprint and phase bead ids, but the `hooks/task-close.md` Facts table has no row for them. Proposed: the QA bead's `metadata.work.sprint`, and the phase file `root`. The hook needs the same lookup.
- Q3: Who pours the re-review QA bead for a fix? "Target layer ... the QA bead's closed dev/fix blocker" implies one exists. Proposed: the pour block also pours one `stage:qa` bead per blocking finding, blocked by the fix bead. That is a second create per finding, so it is not in this sprint until you rule on it.
- Q4: The brief (§B.6) runs the gate on template pushes, but the gate reads no template. Should bo-12 add the ANC-057/060/067 template checks to the gate, or is `test_templates.py` enough?
- Q5: The spec's replies say `atm task close --template <t> --args …`, but the CLI flag is `--vars <json>`.
- Q6: `QA_VERDICT`'s block lists finding ids with no blocking/non-blocking flag, and T-CLOSE-04 checks the two differently. Should the block carry `{"id", "blocking"}` per finding?
- Q7: The installed path is `.claude/skills/atm-bd-lite/` here (ANC-031). The bo-10 doc and `design/jev-check-proposal.md` say `.atm-bd/atm-bd-lite/`. Which one?
- Q8: There is no `bdc` binary release; it is blocked on randlee/sc-publish#135, and no beads-cache issue tracks a prerelease. bo-14 cannot run either block live without one.
