"""Temporary legacy artifact-staging coverage retained pending production proof."""

from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

from prepare_hermes_atm_publish_artifacts import prepare_publish_artifacts
from prepare_hermes_atm_publish_artifacts import WHEEL_ARTIFACT_PREFIX
from prepare_hermes_atm_publish_artifacts import WHEEL_ARTIFACTS


class PrepareHermesAtmPublishArtifactsTests(unittest.TestCase):
    def write_artifacts(self, root: Path) -> None:
        for platform in WHEEL_ARTIFACTS:
            directory = root / f"{WHEEL_ARTIFACT_PREFIX}{platform}"
            directory.mkdir(parents=True)
            (directory / f"atm_graft-1.4.2-cp311-abi3-{platform}.whl").touch()
            (directory / "hermes_atm-1.4.2-py3-none-any.whl").touch()
        sdist_directory = root / "atm-graft-sdist"
        sdist_directory.mkdir()
        (sdist_directory / "atm_graft-1.4.2.tar.gz").touch()

    def test_stages_all_native_wheels_one_universal_wheel_and_sdist(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            artifacts = Path(temporary_directory) / "artifacts"
            self.write_artifacts(artifacts)
            output = Path(temporary_directory) / "publish-dist"

            staged = prepare_publish_artifacts(artifacts, output)

            self.assertEqual(len(staged), 7)
            self.assertEqual(len(list(output.glob("atm_graft*.whl"))), 5)
            self.assertEqual(len(list(output.glob("hermes_atm*.whl"))), 1)
            self.assertEqual(len(list(output.glob("atm_graft*.tar.gz"))), 1)

    def test_rejects_missing_platform_native_wheel(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            artifacts = Path(temporary_directory) / "artifacts"
            self.write_artifacts(artifacts)
            missing = artifacts / f"{WHEEL_ARTIFACT_PREFIX}linux-aarch64" / "atm_graft-1.4.2-cp311-abi3-linux-aarch64.whl"
            missing.unlink()

            with self.assertRaisesRegex(SystemExit, "linux-aarch64 atm-graft wheel"):
                prepare_publish_artifacts(artifacts, Path(temporary_directory) / "publish-dist")

    def test_rejects_duplicate_source_distribution(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            artifacts = Path(temporary_directory) / "artifacts"
            self.write_artifacts(artifacts)
            (artifacts / "atm-graft-sdist" / "atm_graft-1.4.3.tar.gz").touch()

            with self.assertRaisesRegex(SystemExit, "source distribution"):
                prepare_publish_artifacts(artifacts, Path(temporary_directory) / "publish-dist")


if __name__ == "__main__":
    unittest.main()
