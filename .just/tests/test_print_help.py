from __future__ import annotations

from pathlib import Path
import sys
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from print_help import render_help


class PrintHelpTests(unittest.TestCase):
    def test_render_help_mentions_new_lint_entries(self) -> None:
        output = render_help("atm-core")
        self.assertIn("version latest", output)
        self.assertIn("python-tools", output)
        self.assertIn("lint fast", output)
        self.assertIn("lint modules", output)
        self.assertIn("lint daemon-singleton", output)
        self.assertIn("lint boundaries", output)
        self.assertIn("lint unix-gating", output)
        self.assertIn("lint runtime-waits", output)
        self.assertIn("lint sc-boundary", output)
        self.assertIn("lint sc-portability", output)
        self.assertIn("lint manifests", output)
        self.assertIn("lint silent-emit", output)
        self.assertIn("lint function-length", output)
        self.assertIn("lint legacy-mailbox-paths", output)
        self.assertIn("lint capability-degradation", output)
        self.assertIn("lint fixed-sleep", output)
        self.assertIn("lint ttl-triage", output)
        self.assertIn("lint pytests", output)
        self.assertIn("fmt apply", output)
        self.assertIn("view boundaries", output)
        self.assertIn("view lines", output)
        self.assertIn("view modules", output)
        self.assertIn("view deps", output)
        self.assertIn("view unsafe", output)


if __name__ == "__main__":
    unittest.main()
