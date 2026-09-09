# Create a prerelease

Creation pushes a tag and causes `prerelease-archive.yml` to publish GitHub
Release assets. Treat it as an external state change. Proceed beyond the dry
run only when the operator explicitly requested `--create` in writing and the
working branch is clean, non-protected, and attached.

Without an explicit version, invoke the consumer's manifest-declared tag
script. That script owns the repository's patch-version bump and tag policy.

```bash
set -euo pipefail
python3 .claude/skills/prerelease/scripts/prerelease.py --bump --dry-run
python3 .claude/skills/prerelease/scripts/prerelease.py --bump --authorized
```

When `--create X.Y.Z` supplies an explicit version, create that prerelease tag
without invoking the bump script.

```bash
set -euo pipefail
version="$VERSION"
python3 .claude/skills/prerelease/scripts/prerelease.py --publish "$version" --dry-run
python3 .claude/skills/prerelease/scripts/prerelease.py --publish "$version" --authorized
```

Set `VERSION` only from the validated `--create X.Y.Z` argument. The helper
waits for the archive workflow and verifies every manifest-declared asset and
checksum before reporting success.
