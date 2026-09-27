from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest


MODULE = Path(__file__).parents[1] / "assignment-gates.py"
SPEC = importlib.util.spec_from_file_location("assignment_gates", MODULE)
assert SPEC and SPEC.loader
gates = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gates)


class DifficultyGateTests(unittest.TestCase):
    def test_luna_refuses_hard_work(self):
        bead = {"metadata": {"difficulty": "hard"}}
        self.assertEqual("DIFFICULTY_MISMATCH", gates.refusal_for_difficulty(
            bead, [{"identity": "luna", "model": "gpt-6-luna"}], "luna"))

    def test_astra_accepts_hard_work(self):
        bead = {"metadata": {"difficulty": "hard"}}
        self.assertIsNone(gates.refusal_for_difficulty(
            bead, [{"identity": "astra", "model": "gpt-6-astra"}], "astra"))


if __name__ == "__main__":
    unittest.main()
