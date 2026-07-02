#!/usr/bin/env python3
from __future__ import annotations

import argparse
import tarfile
import zipfile
from pathlib import Path


def expected_members(manifest_path: Path, windows: bool) -> set[str]:
    import tomllib

    data = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    binaries = data.get("release_binaries", [])
    names = {entry["name"] for entry in binaries}
    return {f"{name}.exe" if windows else name for name in names}


def archive_members(archive_path: Path) -> set[str]:
    if archive_path.suffix == ".zip":
        with zipfile.ZipFile(archive_path) as archive:
            return {Path(name).name for name in archive.namelist() if not name.endswith("/")}
    if archive_path.suffixes[-2:] == [".tar", ".gz"]:
        with tarfile.open(archive_path, "r:gz") as archive:
            return {Path(member.name).name for member in archive.getmembers() if member.isfile()}
    raise SystemExit(f"unsupported archive type: {archive_path}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify release archive membership against the manifest")
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--archive", required=True)
    args = parser.parse_args()

    archive_path = Path(args.archive)
    windows = archive_path.suffix == ".zip"
    expected = expected_members(Path(args.manifest), windows)
    actual = archive_members(archive_path)
    missing = sorted(expected - actual)
    if missing:
        raise SystemExit(
            f"{archive_path.name} missing expected members: {', '.join(missing)}; actual members: {', '.join(sorted(actual))}"
        )
    print(f"ok: {archive_path.name} contains expected members: {', '.join(sorted(expected))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
