# Install a prerelease

Install the requested version, or the latest matching published prerelease
when no version was supplied. The helper verifies the manifest-declared host
archive and checksum, stages the binary set, updates the platform selector,
and runs the manifest's post-install and verification commands. The same
manifest declares all published wheel and source-distribution assets; install
selects a compatible host wheel only when staging Python packages is needed.

This is the binary-only dogfood path for randlee, Colima, m5-atmbench, and
other tester hosts: it downloads and checksum-verifies GitHub prerelease
assets only, with no Cargo build or package-registry publish. Then switch the
selected pair through daemon-switch:

```bash
python3 .claude/skills/prerelease/scripts/prerelease.py --install X.Y.Z
python3 .claude/skills/daemon-switch/scripts/daemon-switch.py switch --prerelease X.Y.Z --yes --discover-managed-service
```

```bash
set -euo pipefail
version="${VERSION:-latest}"
python3 .claude/skills/prerelease/scripts/prerelease.py --install "$version" --dry-run
python3 .claude/skills/prerelease/scripts/prerelease.py --install "$version"
```

Set `VERSION` only from the optional `--install X.Y.Z` argument. A staged
version may skip downloading, but selector repair and manifest-declared
post-install verification still run.
