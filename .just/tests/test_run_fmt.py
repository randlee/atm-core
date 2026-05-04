from __future__ import annotations

from pathlib import Path
import sys
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from run_fmt import resolve_command


class RunFmtTests(unittest.TestCase):
    def test_resolve_command_accepts_aliases(self) -> None:
        self.assertEqual(resolve_command("check"), ["just", "_fmt-check"])
        self.assertEqual(resolve_command("write"), ["just", "_fmt-write"])
        self.assertEqual(resolve_command("apply"), ["just", "_fmt-write"])

    def test_resolve_command_rejects_unknown_mode(self) -> None:
        self.assertIsNone(resolve_command("unknown"))


if __name__ == "__main__":
    unittest.main()
