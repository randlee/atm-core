"""Unit tests for the ripgrep-free source-pattern assertion used by smoke rows."""
from __future__ import annotations

import contextlib
import importlib.util
import io
from pathlib import Path
import sys
import tempfile
import unittest


def load_module():
    path = Path(__file__).with_name("source_pattern.py")
    spec = importlib.util.spec_from_file_location("atm_smoke_source_pattern", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


PATTERN = load_module()


class SourcePatternTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def write(self, name: str, text: str) -> Path:
        path = self.root / name
        path.write_text(text, encoding="utf-8")
        return path

    def invoke(self, pattern: str, path: Path) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = PATTERN.main(["source_pattern.py", pattern, str(path)])
        return code, out.getvalue(), err.getvalue()

    def test_pattern_found_reports_line_number_and_text(self) -> None:
        path = self.write(
            "member_mutation.rs",
            "// header\n"
            "fn other() {}\n"
            "fn build_member_add_roster_record(request: &AddMemberRequest) -> RosterEntry {\n",
        )
        code, stdout, stderr = self.invoke(
            r"fn build_member_add_roster_record\(request: &AddMemberRequest\) -> RosterEntry",
            path,
        )
        self.assertEqual(code, PATTERN.EXIT_MATCHED)
        self.assertEqual(stderr, "")
        self.assertIn(":3:", stdout)
        self.assertIn("fn build_member_add_roster_record", stdout)

    def test_search_pattern_returns_first_match_position(self) -> None:
        path = self.write("ci.yml", "a\nb\nname: Run replacement-workspace tests\n")
        found = PATTERN.search_pattern(path, "Run replacement-workspace tests")
        assert found is not None
        line_number, line = found
        self.assertEqual(line_number, 3)
        self.assertIn("Run replacement-workspace tests", line)

    def test_pattern_absent_fails_without_matching_output(self) -> None:
        path = self.write("member_mutation.rs", "fn unrelated() {}\n")
        code, stdout, stderr = self.invoke(r"validate_update_member_caller\(", path)
        self.assertEqual(code, PATTERN.EXIT_NOT_MATCHED)
        self.assertEqual(stdout, "")
        self.assertIn("no line matched", stderr)
        self.assertIsNone(PATTERN.search_pattern(path, r"validate_update_member_caller\("))

    def test_missing_file_is_reported_as_unreadable_rather_than_crashing(self) -> None:
        missing = self.root / "does-not-exist.rs"
        code, stdout, stderr = self.invoke("anything", missing)
        self.assertEqual(code, PATTERN.EXIT_UNREADABLE)
        self.assertEqual(stdout, "")
        self.assertIn("cannot read file", stderr)
        self.assertIn("does-not-exist.rs", stderr)
        with self.assertRaises(PATTERN.SourcePatternError):
            PATTERN.search_pattern(missing, "anything")

    def test_smoke_rows_no_longer_shell_out_to_ripgrep(self) -> None:
        smoke = Path(__file__).parent
        for name in ("run.py", "run_thorough.py"):
            source = (smoke / name).read_text(encoding="utf-8")
            self.assertNotIn('"rg",', source, f"{name} still invokes the external rg binary")
            self.assertIn("scripts/smoke/source_pattern.py", source)


if __name__ == "__main__":
    unittest.main()
