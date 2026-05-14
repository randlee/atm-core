from __future__ import annotations

from pathlib import Path
import importlib.util
import sys
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts" / "check-function-length.py"
SPEC = importlib.util.spec_from_file_location("check_function_length", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise ImportError(f"unable to import {SCRIPT_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class CheckFunctionLengthTests(unittest.TestCase):
    def test_find_function_spans_ignores_test_attributes(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            path = Path(tempdir) / "sample.rs"
            path.write_text(
                """\
#[test]
fn test_only() {
    let value = 1;
}

fn production() {
    let value = 1;
}
""",
                encoding="utf-8",
            )
            spans = MODULE.find_function_spans(path)

        self.assertEqual([span.name for span in spans], ["production"])

    def test_overlaps_changed_lines_detects_diff_overlap(self) -> None:
        function = MODULE.FunctionSpan(path=Path("crates/atm-core/src/example.rs"), name="demo", start_line=10, end_line=20)

        self.assertTrue(MODULE.overlaps_changed_lines(function, {9, 10}))
        self.assertTrue(MODULE.overlaps_changed_lines(function, {20}))
        self.assertFalse(MODULE.overlaps_changed_lines(function, {21, 22}))


if __name__ == "__main__":
    unittest.main()
