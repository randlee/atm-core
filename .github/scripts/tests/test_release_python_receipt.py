from __future__ import annotations

import io
import json
import sys
import tarfile
import zipfile
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / ".github" / "scripts"))
from release_python_receipt import create_receipt  # noqa: E402


def test_receipt_binds_source_manifest_version_and_checksums(tmp_path: Path) -> None:
    manifest = tmp_path / "publish-artifacts.toml"
    manifest.write_text(
        """[[crates]]
artifact = "fixture"
package = "fixture"
publish_order = 1

[[python_distributions]]
name = "fixture"
sdist = true
wheels = ["ubuntu-latest"]
""",
        encoding="utf-8",
    )
    assets = tmp_path / "assets"
    assets.mkdir()
    with zipfile.ZipFile(assets / "fixture-1.4.3-py3-none-any.whl", "w") as wheel:
        wheel.writestr("fixture-1.4.3.dist-info/METADATA", "Name: fixture\nVersion: 1.4.3\n")
    with tarfile.open(assets / "fixture-1.4.3.tar.gz", "w:gz") as sdist:
        metadata = b"Name: fixture\nVersion: 1.4.3\n"
        info = tarfile.TarInfo("fixture-1.4.3/PKG-INFO")
        info.size = len(metadata)
        sdist.addfile(info, io.BytesIO(metadata))

    receipt = create_receipt(manifest, assets, "1.4.3", "a" * 40)

    assert receipt["source_commit"] == "a" * 40
    assert receipt["version"] == "1.4.3"
    assert len(receipt["manifest_sha256"]) == 64
    assert {item["filename"] for item in receipt["artifacts"]} == {
        "fixture-1.4.3-py3-none-any.whl",
        "fixture-1.4.3.tar.gz",
    }


def test_receipt_rejects_version_drift(tmp_path: Path) -> None:
    manifest = tmp_path / "publish-artifacts.toml"
    manifest.write_text(
        """[[crates]]
artifact = "fixture"
package = "fixture"
publish_order = 1

[[python_distributions]]
name = "fixture"
sdist = false
wheels = ["ubuntu-latest"]
""",
        encoding="utf-8",
    )
    assets = tmp_path / "assets"
    assets.mkdir()
    with zipfile.ZipFile(assets / "fixture.whl", "w") as wheel:
        wheel.writestr("fixture.dist-info/METADATA", "Name: fixture\nVersion: 1.4.2\n")

    with pytest.raises(SystemExit, match="expected version 1.4.3"):
        create_receipt(manifest, assets, "1.4.3", "a" * 40)


def test_receipt_rejects_incomplete_manifest_asset_set(tmp_path: Path) -> None:
    manifest = tmp_path / "publish-artifacts.toml"
    manifest.write_text(
        """[[crates]]
artifact = "fixture"
package = "fixture"
publish_order = 1

[[python_distributions]]
name = "fixture"
sdist = true
wheels = ["ubuntu-latest"]
""",
        encoding="utf-8",
    )
    assets = tmp_path / "assets"
    assets.mkdir()
    with zipfile.ZipFile(assets / "fixture.whl", "w") as wheel:
        wheel.writestr("fixture.dist-info/METADATA", "Name: fixture\nVersion: 1.4.3\n")

    with pytest.raises(SystemExit, match="Python artifact set mismatch"):
        create_receipt(manifest, assets, "1.4.3", "a" * 40)
