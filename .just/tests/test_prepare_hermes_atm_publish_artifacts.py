"""The PyPI staging helper delegates artifact selection to the release manifest reader."""

from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

from prepare_hermes_atm_publish_artifacts import prepare_publish_artifacts


class PrepareHermesAtmPublishArtifactsTests(unittest.TestCase):
    def test_staging_uses_manifest_verifier_and_returns_copied_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            artifacts = root / "artifacts"
            artifacts.mkdir()
            output = root / "publish-dist"
            manifest = root / "publish-artifacts.toml"
            manifest.touch()

            def verify(command: list[str], *, check: bool) -> None:
                self.assertTrue(check)
                self.assertEqual(command[1:3], [".github/scripts/release_artifacts.py", "verify-python-release-assets"])
                self.assertIn("--manifest", command)
                self.assertEqual(command[command.index("--manifest") + 1], str(manifest))
                self.assertEqual(command[command.index("--asset-dir") + 1], str(artifacts))
                self.assertEqual(command[command.index("--copy-to") + 1], str(output))
                (output / "manifest-wheel.whl").touch()

            with mock.patch("prepare_hermes_atm_publish_artifacts.subprocess.run", side_effect=verify):
                staged = prepare_publish_artifacts(artifacts, output, manifest)

            self.assertEqual([path.name for path in staged], ["manifest-wheel.whl"])


if __name__ == "__main__":
    unittest.main()
