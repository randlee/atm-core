---
name: prerelease
description: List, install, or create manifest-declared GitHub prerelease archives. Use for /prerelease --list, --install, or --create operations.
---

# Prerelease

Operate only on GitHub prerelease archives declared by
`release/publish-artifacts.toml`. Never publish Homebrew, crates.io, PyPI,
Winget, Scoop, or another package channel from this skill.

## Step 1 — Verify dependencies

```bash
which python3 && python3 --version
python3 -c 'import sys; raise SystemExit("Python 3.11+ is required") if sys.version_info < (3, 11) else None'
which git && git --version
which gh && gh --version
gh auth status
```

If a dependency is absent from `PATH`, check common installation locations as
described in
[references/installation-and-troubleshooting.md](references/installation-and-troubleshooting.md).
Stop if the required command or GitHub authentication is unavailable.

## Route the requested mode

Read and follow exactly one mode file; do not load the other mode procedures.

- `--list`: read [list.md](list.md).
- `--install [X.Y.Z]`: read [install.md](install.md). Omit the version to
  select the latest matching prerelease.
- `--create [X.Y.Z]`: read [create.md](create.md). Omit the version to invoke
  the manifest-declared patch-version bump and tag command.

Require exactly one mode. Run from the consumer repository root, fail closed
when `[prerelease]` is absent, and take all repository-specific values from the
manifest. Do not hardcode project names, tag prefixes, binaries, targets,
installation paths, selectors, or verification commands.
