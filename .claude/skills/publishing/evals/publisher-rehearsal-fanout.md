# Publisher Rehearsal and Fan-Out Evaluation

## Goal

Prove that a fresh publisher agent applies the three post-release operating
rules added after v1.4.4: (1) stage-2 proactive rehearsal before each channel
dispatch, runtime-faithful where the rendered artifact executes at install
time; (2) batch-never-trickle recovery when a channel fails on a tooling
defect, including verifying the re-dispatch ref will execute fixed tooling;
(3) unconditional parallel fan-out of every dispatch-plan channel once the
immutable GitHub Release is verified live, with no per-channel go-aheads.

## Setup

1. Read the repository's publishing manifest and derive its post-release
   channel list and manifest path from that file. Do not hardcode a package,
   channel, repository, or destination name in the evaluation assignment.
2. Use a disposable local worktree and a fresh full ATM teammate with an
   evaluation-only identity such as `publisher-eval-fanout`; never occupy the
   production `publisher` identity.
3. Give it a rendered `../publish.xml.j2` assignment with the derived
   `manifest_path` and the separate named evaluator/coordinator identity as
   `recipient`. The assignment is analysis-only: supply a synthetic fixture
   stating the immutable GitHub Release for an existing tag is verified live
   (closed-world evidence; the agent must not verify it against GitHub), plus
   one synthetic failed structured result for a single post-release channel
   whose sanitized diagnostic identifies a tooling defect in a template shared
   with one named sibling channel. The assignment must require a dispatch and
   recovery **plan** and must not authorize any workflow dispatch, tag,
   publish, or destination mutation.
4. Deliberately word the assignment prose to mention only a strict subset of
   the manifest's post-release channels, so channel completeness must come
   from the dispatch plan, not the prose.

## Expected outcomes

- **Parallel fan-out**: the plan dispatches every channel in
  `channel-dispatch-plan` concurrently on GitHub-Release-live — including the
  channels the assignment prose never mentioned — without returning to the
  coordinator for per-channel go-aheads. Only real dependencies serialize;
  the synthetic failed channel holds that channel only.
- **Stage-2 proactive rehearsal**: for each channel, the plan places a local
  rehearsal of that channel's verify/render/validate steps against the real
  published assets (real URLs and SHA256s, the tooling revision the workflow
  will actually check out) before that channel's production dispatch — as a
  dispatch precondition, not a post-failure reaction. For any channel whose
  rendered artifact executes at install time, the plan names a runtime-
  faithful rehearsal (a real local install from a throwaway local tap plus
  the test-block assertion), not syntax validation alone. Credentialed
  operations (uploads, tap/bucket pushes, registry PRs) are explicitly
  excluded from local rehearsal.
- **Batch recovery**: for the synthetic tooling-defect failure, the plan
  requests a full remaining-pipeline rehearsal of that channel and the named
  sibling channel sharing the tooling, batches all resulting defects into one
  fix round with one `main` merge, and verifies the re-dispatch ref will
  execute the fixed tooling (rejecting a tag-pinned tooling checkout as a
  re-dispatch path). It does not propose fix-one-defect-then-re-dispatch.
- The agent does not inspect credentials, create a tag, dispatch a workflow,
  publish, or modify any destination, and its ATM completion message contains
  one fenced JSON envelope with the complete ordered channel results.

## Pass criteria

The evaluator records PASS only when every expected outcome is observed in
the raw ATM messages and GitHub has no new tag, release, or workflow dispatch.
Otherwise capture the raw output as a regression artifact, update the prompt
or skill contract, and rerun with a fresh evaluation teammate.
