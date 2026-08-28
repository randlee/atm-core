#!/usr/bin/env python3
from __future__ import annotations

import argparse
import tarfile
import tomllib
import zipfile
from pathlib import Path

def expected_members(manifest_path: Path, windows: bool) -> set[str]:
    """Return ATM-owned archive members declared by the consumer manifest."""

    manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    names = {entry["name"] for entry in manifest["release_binaries"]}
    binary_members = {f"bin/{name}.exe" if windows else f"bin/{name}" for name in names}
    bundled_members: set[str] = set()
    repo_root = manifest_path.parent.parent
    for binary in manifest["release_binaries"]:
        for bundle in binary.get("bundled_paths", []):
            source = repo_root / bundle["source"]
            destination = Path(bundle["destination"])
            if source.is_file():
                bundled_members.add(destination.as_posix())
                continue
            if not source.is_dir():
                raise SystemExit(f"bundled path source is missing: {source}")
            bundled_members.update(
                (destination / path.relative_to(source)).as_posix()
                for path in source.rglob("*")
                if path.is_file()
            )
    return binary_members | bundled_members


def normalize_member(name: str) -> str:
    return name.removeprefix("./").strip("/")


def archive_members(archive_path: Path) -> set[str]:
    if archive_path.suffix == ".zip":
        with zipfile.ZipFile(archive_path) as archive:
            return {normalize_member(name) for name in archive.namelist() if not name.endswith("/")}
    if archive_path.suffixes[-2:] == [".tar", ".gz"]:
        with tarfile.open(archive_path, "r:gz") as archive:
            return {normalize_member(member.name) for member in archive.getmembers() if member.isfile()}
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
