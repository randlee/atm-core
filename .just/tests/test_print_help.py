from __future__ import annotations

from pathlib import Path
import sys
import subprocess
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from print_help import render_help


class PrintHelpTests(unittest.TestCase):
    def test_render_help_mentions_new_lint_entries(self) -> None:
        output = render_help("atm-core")
        self.assertIn("version latest", output)
        self.assertIn("setup", output)
        self.assertIn("coverage", output)
        self.assertIn("test python|build|workspace|cli-surface|herdr", output)
        self.assertIn("upgrade", output)
        self.assertIn("lint sc-runtime", output)
        self.assertIn("lint ci [host|all]", output)
        self.assertNotIn("lint runtime-waits", output)
        self.assertNotIn("lint sc-boundary", output)
        self.assertNotIn("lint sc-portability", output)
        self.assertIn("lint function-length", output)
        self.assertIn("fmt apply", output)
        self.assertIn("view boundaries", output)
        self.assertIn("view lines", output)
        self.assertIn("view modules", output)
        self.assertIn("view deps", output)
        self.assertIn("view unsafe", output)

    def test_advertised_fmt_selector_runs(self) -> None:
        result = subprocess.run(["just", "lint", "fmt"], cwd=JUST_DIR.parent, capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr or result.stdout)


if __name__ == "__main__":
    unittest.main()
