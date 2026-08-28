from __future__ import annotations

import argparse
from pathlib import Path

from release_manifest import check_version_publication, registry_version_state


def cmd_check_version_unpublished(args: argparse.Namespace) -> int:
    """Detect already-published crates via the contract's exact version lookup."""
    unexpected, preserved = check_version_publication(
        Path(args.manifest), args.version, args.already_published_channels
    )
    if unexpected:
        raise SystemExit("release version already published for: " + ", ".join(sorted(unexpected)))
    if preserved:
        print(
            "ok: crates_io is preserved from a prior release run; version already published for: "
            + ", ".join(sorted(preserved))
        )
        return 0
    print(f"ok: no publishable artifacts found at version {args.version}")
    return 0


def cmd_registry_status(args: argparse.Namespace) -> int:
    """Resolve one public-registry URL to published or absent, failing closed."""
    print(registry_version_state(args.url, timeout=args.timeout))
    return 0
