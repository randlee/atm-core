from __future__ import annotations

from pathlib import Path
import sys
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from view_unsafe import geiger_command


class ViewUnsafeTests(unittest.TestCase):
    def test_geiger_command_uses_manifest_path_and_output_format(self) -> None:
        manifest_path = Path("/tmp/repo/crates/atm-core/Cargo.toml")
        command = geiger_command(manifest_path, "agent-team-mail-core", "Json")

        self.assertEqual(
            command,
            [
                "cargo",
                "geiger",
                "--package",
                "agent-team-mail-core",
                "--manifest-path",
                str(manifest_path),
                "--all-targets",
                "--output-format",
                "Json",
            ],
        )


if __name__ == "__main__":
    unittest.main()
