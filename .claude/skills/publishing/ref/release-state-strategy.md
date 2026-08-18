# Release-State Strategy

This is the single authoritative policy for deciding where release work runs.
It applies before every preflight and publish task. The release manifest remains
the source of truth for artifacts, channels, and publish order.

## Invariants

- Tags and publication originate only from `main`.
- Ordinary new code must land on `develop` before `main`.
- A release-blocking fix is the only exception: it may use `release/*` from
  `main` and return to `develop` after publication.
- A readiness preflight before merging to `main` and the final preflight of the
  exact `main` commit are separate checks. Neither substitutes for the other.

| Starting state | Correct path |
| --- | --- |
| Code only on `feature/*` or `fix/*` | Merge it to `develop` first, then follow the `develop` path. Only a release-branch fix may bypass `develop`. |
| Code on `develop` | Create `release/*`, prepare the version and release PR, and run readiness preflight on that branch before its `main` merge. Fix readiness failures there. After merge, run final preflight on the exact `main` commit; publish only if it passes. |
| Code on `main` | Run final preflight on `main` and publish if it passes. If it fails, create `release/*` from `main`, fix and readiness-preflight there, merge back to `main`, then run final preflight again on the merged `main` commit. |
| Code on `release/*` | Run readiness preflight there and fix failures there. Merge to `main`, then run final preflight on the exact merged `main` commit before publishing. |

## Preflight and Recovery

Run readiness preflight as early as the correct state permits; do not wait for
the `main` PR to complete. If code has already reached `main`, run final
preflight there once before creating a release branch. A failure creates the
release-branch recovery path shown above.

All credentials are standardized GitHub Actions secrets. Preflight checks only
non-disclosing availability and authorized server-side rehearsal evidence; no
agent asks about, reads, prints, substitutes, or re-enters a token.

For a partial crates.io publication, keep the same tag and release ref. The
manifest-ordered crates.io job skips crates already live and retries only the
missing crate set. Do not bump a version or replay successful channels solely
because a newly added crate was missing on the first attempt.
