from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import unittest

MODULE = Path(__file__).parents[1] / "transitions.py"
FIXTURES = Path(__file__).parent / "fixtures"
SPEC = importlib.util.spec_from_file_location("transitions", MODULE)
assert SPEC and SPEC.loader
transitions = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(transitions)


class TransitionTests(unittest.TestCase):
    def test_json_transition_fixtures(self):
        for path in sorted(FIXTURES.glob("transition-*.json")):
            with self.subTest(fixture=path.name):
                fixture = json.loads(path.read_text())
                self.assertEqual(transitions.next_transition(fixture["state"]), fixture["expected"])


if __name__ == "__main__": unittest.main()
