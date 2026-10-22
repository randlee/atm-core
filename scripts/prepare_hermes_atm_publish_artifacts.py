#!/usr/bin/env python3
"""Validate and stage the manifest-declared Python distributions for PyPI."""

from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import subprocess
import sys


def prepare_publish_artifacts(
    artifacts_directory: Path, output_directory: Path, manifest_path: Path
) -> list[Path]:
    """Use the release manifest verifier to copy the exact Python asset set."""
    if output_directory.exists():
        shutil.rmtree(output_directory)
    output_directory.mkdir(parents=True)
    subprocess.run(
        [
            sys.executable,
            ".github/scripts/release_artifacts.py",
            "verify-python-release-assets",
            "--manifest",
            str(manifest_path),
            "--asset-dir",
            str(artifacts_directory),
            "--copy-to",
            str(output_directory),
        ],
        check=True,
    )
    return sorted(path for path in output_directory.iterdir() if path.is_file())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()

    for artifact in prepare_publish_artifacts(args.artifacts, args.output, args.manifest):
        print(artifact)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
