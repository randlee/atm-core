from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest


SCRIPT_PATH = Path(__file__).resolve().parents[2] / "scripts" / "check-nudge-taxonomy.py"
SPEC = importlib.util.spec_from_file_location("check_nudge_taxonomy", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
CHECK = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CHECK
SPEC.loader.exec_module(CHECK)


class CheckNudgeTaxonomyTests(unittest.TestCase):
    def test_test_only_sources_and_inline_modules_are_exempt(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            (root / "crates/example/tests").mkdir(parents=True)
            (root / "crates/example/src").mkdir(parents=True)
            (root / "crates/example/tests/integration.rs").write_text(
                "fn integration_nudge_identifier() {}\n", encoding="utf-8"
            )
            (root / "crates/example/src/fixture_tests.rs").write_text(
                "fn fixture_nudge_identifier() {}\n", encoding="utf-8"
            )
            (root / "crates/example/src/lib.rs").write_text(
                "#[cfg(test)]\n"
                "mod nudge_mode_tests {\n"
                "    fn inline_nudge_identifier() {}\n"
                "}\n"
                "fn production_nudge_identifier() {}\n"
                "fn another_nudge_tests() {}\n",
                encoding="utf-8",
            )

            violations = CHECK.find_violations(root)

        self.assertEqual(
            [(violation.path.as_posix(), violation.line_number) for violation in violations],
            [("crates/example/src/lib.rs", 5)],
        )
        self.assertIn("production_nudge_identifier", violations[0].line)


if __name__ == "__main__":
    unittest.main()
