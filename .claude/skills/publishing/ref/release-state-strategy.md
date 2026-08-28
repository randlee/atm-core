# Release-State Strategy

This is the single authoritative policy for deciding where release work runs.
It applies before every preflight and publish task. The release manifest remains
the source of truth for artifacts, channels, and publish order.

## Invariants

- Production `vX.Y.Z` tags and publication originate only from `main`.
- Ordinary new code must land on `develop` before `main`.
- For every version, `release-candidate-vX.Y.Z` is the immutable provenance
  tag. The `Release Candidate` workflow creates it at `origin/develop`, or
  reuses it only after proving that it remains an ancestor of `origin/develop`.
- The release branch starts from that release-candidate tag. A release fix may
  remain on `release/*` through publication and return to `develop` afterward.
- A readiness preflight before merging to `main` and the final preflight of the
  exact `main` commit are separate checks. Neither substitutes for the other.
- The final release gate proves that `release-candidate-vX.Y.Z` is an ancestor
  of `origin/main`; it never requires the current tips of `main` and `develop`
  to have identical content. New work may continue on `develop` after the
  candidate is cut.

| Starting state | Correct path |
| --- | --- |
| Code only on `feature/*` or `fix/*` | Merge it to `develop` first, then follow the `develop` path. Only a release-branch fix may bypass `develop`. |
| Code on `develop` | Under explicit publisher assignment, dispatch `release-candidate.yml` for the version. Create `release/*` from `release-candidate-vX.Y.Z`, prepare the version and release PR, then run readiness preflight on that branch. Fix readiness failures there. After merge, run final preflight on the exact `main` commit; publish only if it passes. |
| Code on `main` | Run final preflight on `main` and publish if it passes. The matching release-candidate tag must already be an ancestor of `main`. If it does not, return to the `develop` path and cut the candidate before proceeding. |
| Code on `release/*` | Confirm the branch descends from `release-candidate-vX.Y.Z`, run readiness preflight there, and fix failures there. Merge to `main`, then run final preflight on the exact merged `main` commit before publishing. |

## Candidate Cut and Post-Cut Drift

Only the `Release Candidate` GitHub workflow may create a release-candidate
tag. Under an explicit assignment, `publisher` dispatches that workflow before
creating the release branch. It must not use a local `git tag` or `git push`
command. Reusing a tag is safe only when the workflow proves it is an ancestor
of the current `origin/develop`.

Before every readiness or final preflight, `publisher` records the complete
diff from `release-candidate-vX.Y.Z` to the checked release ref:

```bash
git diff --name-status "release-candidate-vX.Y.Z"..<release-ref>
```

Release metadata and release-branch fixes are allowed. If the recorded diff
includes non-trivial implementation or dependency changes, `publisher` must
flag them to the named coordinator and obtain an explicit decision before
publishing. It must never silently treat them as metadata. `develop` commits
made after the candidate tag are outside this comparison and do not delay the
release.

## Preflight and Recovery

Run readiness preflight as early as the correct state permits; do not wait for
the `main` PR to complete. If code has already reached `main`, run final
preflight there once. A failure creates the release-branch recovery path shown
above; that branch must retain the matching release-candidate provenance.

All credentials are standardized GitHub Actions secrets. Preflight checks only
non-disclosing availability and authorized server-side rehearsal evidence; no
agent asks about, reads, prints, substitutes, or re-enters a token.

For a partial crates.io publication, keep the same tag and release ref. The
manifest-ordered crates.io job skips crates already live and retries only the
missing crate set. Do not bump a version or replay successful channels solely
because a newly added crate was missing on the first attempt.
