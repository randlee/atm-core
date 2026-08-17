#!/usr/bin/env python3
"""Validate and stage the exact Hermes ATM files eligible for a PyPI upload."""

from __future__ import annotations

import argparse
import shutil
from pathlib import Path


WHEEL_ARTIFACTS = (
    "linux-x86_64",
    "linux-musl-x86_64",
    "linux-aarch64",
    "macos-aarch64",
    "windows-x86_64",
)
WHEEL_ARTIFACT_PREFIX = "hermes-atm-wheels-"
SDIST_ARTIFACT = "atm-graft-sdist"


def exactly_one(directory: Path, pattern: str, description: str) -> Path:
    matches = sorted(directory.glob(pattern))
    if len(matches) != 1:
        found = ", ".join(path.name for path in matches) or "none"
        raise SystemExit(
            f"expected exactly one {description} matching {pattern!r} in {directory}; found {found}"
        )
    return matches[0]


def prepare_publish_artifacts(artifacts_directory: Path, output_directory: Path) -> list[Path]:
    """Copy the platform-native wheels, one universal wheel, and the atm-graft sdist."""

    if output_directory.exists():
        shutil.rmtree(output_directory)
    output_directory.mkdir(parents=True)

    universal_wheel: Path | None = None
    staged: list[Path] = []
    for platform in WHEEL_ARTIFACTS:
        artifact_directory = artifacts_directory / f"{WHEEL_ARTIFACT_PREFIX}{platform}"
        native_wheel = exactly_one(artifact_directory, "atm_graft*.whl", f"{platform} atm-graft wheel")
        hermes_wheel = exactly_one(artifact_directory, "hermes_atm*.whl", f"{platform} hermes-atm wheel")
        staged.append(Path(shutil.copy2(native_wheel, output_directory / native_wheel.name)))
        if universal_wheel is None:
            universal_wheel = hermes_wheel
        elif hermes_wheel.name != universal_wheel.name:
            raise SystemExit(
                f"hermes-atm wheel filename differs for {platform}: "
                f"expected {universal_wheel.name}, found {hermes_wheel.name}"
            )

    if universal_wheel is None:
        raise SystemExit("no Hermes ATM wheel artifacts were supplied")
    staged.append(Path(shutil.copy2(universal_wheel, output_directory / universal_wheel.name)))

    sdist_directory = artifacts_directory / SDIST_ARTIFACT
    sdist = exactly_one(sdist_directory, "atm_graft*.tar.gz", "atm-graft source distribution")
    staged.append(Path(shutil.copy2(sdist, output_directory / sdist.name)))

    if len(staged) != len(WHEEL_ARTIFACTS) + 2:
        raise SystemExit(f"expected seven publish artifacts, staged {len(staged)}")
    return staged


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    staged = prepare_publish_artifacts(args.artifacts, args.output)
    for artifact in staged:
        print(artifact)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
