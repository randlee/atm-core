# Publish a prerelease

Publishing pushes a tag and causes `prerelease-archive.yml` to publish GitHub
Release assets. Treat it as an external state change. Proceed beyond the dry
run only when the operator explicitly requested `--publish` (or its
compatibility alias `--create`) in writing and the working branch is clean,
non-protected, and attached.

Always invoke the consumer's manifest-declared tag script. That script owns
the repository's version-selection and tag policy: it may use the current
workspace version only when that version has never been published; otherwise
it must patch-bump before tagging. Reject an explicit version. The script and
workflow must fail closed rather than overwrite or republish a version.

```bash
set -euo pipefail
python3 .claude/skills/prerelease/scripts/prerelease.py --publish --dry-run
python3 .claude/skills/prerelease/scripts/prerelease.py --publish --authorized
```

The helper waits for the archive workflow for the newly tagged commit and
verifies every manifest-declared asset and checksum before reporting success.
