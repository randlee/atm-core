# Install a prerelease

Install the requested version, or the latest matching published prerelease
when no version was supplied. The helper verifies the manifest-declared host
archive and checksum, stages the binary set, updates the platform selector,
and runs the manifest's post-install and verification commands.

```bash
set -euo pipefail
version="${VERSION:-latest}"
python3 .claude/skills/prerelease/scripts/prerelease.py --install "$version" --dry-run
python3 .claude/skills/prerelease/scripts/prerelease.py --install "$version"
```

Set `VERSION` only from the optional `--install X.Y.Z` argument. A staged
version may skip downloading, but selector repair and manifest-declared
post-install verification still run. ATM's post-install command asks
daemon-switch to discover exactly one existing current-user managed service;
an unmanaged host or ambiguous service set fails closed and must be provisioned
or selected explicitly outside this static manifest. If activation or
verification fails, the installer restores the prerelease selector files it
found before the attempt.
