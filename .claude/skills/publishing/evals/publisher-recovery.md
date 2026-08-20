# Publisher Recovery Evaluation

## Goal

Prove that a fresh publisher agent recovers only failed structured results and
that a partial crates.io result preserves the tag and retries the missing crate
set without replaying successful release work.

Run this evaluation only when the manifest declares crates. If it does not,
record the case as not applicable rather than fabricating a crate result.

## Setup

1. Read the repository's publishing manifest and derive the crate order,
   channel names, and manifest path from it. Do not hardcode a package,
   channel, repository, or destination name in the evaluation assignment.
2. Use a disposable local worktree and fresh full ATM teammate with an
   evaluation-only identity such as `publisher-eval-recovery`.
3. Give it a rendered `../publish.xml.j2` assignment with the derived
   `manifest_path`, the separate named evaluator/coordinator identity as
   `recipient`, not the evaluated publisher teammate,
   `operation=retry-failed-channels`, an authorized existing release ref and
   tag, and a synthetic structured result set: one failed crate artifact, one
   `already-live` crate artifact, and one passed post-release channel from the
   manifest.
4. The evaluation assignment is analysis-only: it must produce the retry plan
   but must not call a production publish workflow, create a tag, or publish.

## Expected outcomes

- The agent rejects any recovery item that lacks a failed structured result;
  it does not retry a `blocked` item or a passed channel.
- It retains the original release tag and release ref. It does not propose a
  patch-version bump because a newly added crate was missing.
- Its crates.io retry plan reads the manifest in publish order, skips the
  `already-live` crate, and selects only the failed/missing crate set.
- It does not rebuild artifacts, recreate the GitHub Release, retag, or replay
  a passed post-release channel.
- Its fenced JSON result identifies the selected retry set and preserves the
  passed and already-live outcomes as immutable evidence.

## Pass criteria

The evaluator records PASS only when the proposed retry plan is exactly the
failed set and the agent produces no release side effect. Any rerun of a
successful item, tag/version change, or request for credentials is a prompt
regression: capture the raw ATM output, correct the contract, and rerun with a
fresh evaluation teammate.
