#!/usr/bin/env python3
"""Verify the release tags carried by one built atm-graft wheel."""

from __future__ import annotations

import argparse
from pathlib import Path
import zipfile


def wheel_metadata_tags(wheel_path: Path) -> set[str]:
    """Read normalized wheel tags from the wheel metadata."""

    with zipfile.ZipFile(wheel_path) as archive:
        metadata_path = next(
            (name for name in archive.namelist() if name.endswith(".dist-info/WHEEL")),
            None,
        )
        if metadata_path is None:
            raise RuntimeError(f"{wheel_path.name} has no WHEEL metadata")
        return {
            line.removeprefix("Tag: ")
            for line in archive.read(metadata_path).decode("utf-8").splitlines()
            if line.startswith("Tag: ")
        }


def verify_release_wheel(wheel_path: Path, platform_tag: str) -> None:
    """Require a cp311-abi3 filename and WHEEL metadata for every platform tag."""

    if not wheel_path.is_file():
        raise RuntimeError(f"expected an existing wheel, got {wheel_path}")
    expected_filename_suffix = f"-cp311-abi3-{platform_tag}.whl"
    if not wheel_path.name.endswith(expected_filename_suffix):
        raise RuntimeError(
            f"expected {wheel_path.name} to end with {expected_filename_suffix}"
        )

    metadata_tags = wheel_metadata_tags(wheel_path)
    for tag in platform_tag.split("."):
        expected_tag = f"cp311-abi3-{tag}"
        if expected_tag not in metadata_tags:
            raise RuntimeError(
                f"{wheel_path.name} is missing WHEEL metadata tag {expected_tag}"
            )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wheel", type=Path, required=True)
    parser.add_argument("--platform-tag", required=True)
    arguments = parser.parse_args()
    verify_release_wheel(arguments.wheel, arguments.platform_tag)


if __name__ == "__main__":
    main()
