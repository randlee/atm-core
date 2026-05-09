"""Unit tests for find_todos.py."""

from __future__ import annotations

import importlib.util
import io
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest.mock import patch

_SCRIPT = Path(__file__).parent / "find_todos.py"
_SPEC = importlib.util.spec_from_file_location("find_todos", _SCRIPT)
_MOD = importlib.util.module_from_spec(_SPEC)
sys.modules[_SPEC.name] = _MOD
_SPEC.loader.exec_module(_MOD)


class TestFindTodos(unittest.TestCase):
    def test_extract_rows_normalizes_tags_and_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            nested = root / "crates" / "atm" / "src"
            nested.mkdir(parents=True)
            source = nested / "example.rs"
            source.write_text(
                textwrap.dedent(
                    """
                    // TODO(P6): tighten the acceptance rule
                    // TODO fix this helper
                    // TODO:
                    """
                ).strip()
                + "\n",
                encoding="utf-8",
            )

            rows = _MOD.extract_rows(root)

        self.assertEqual(
            rows,
            [
                _MOD.TodoRow(
                    path="crates/atm/src/example.rs",
                    line=1,
                    tag="P6",
                    text="tighten the acceptance rule",
                ),
                _MOD.TodoRow(
                    path="crates/atm/src/example.rs",
                    line=2,
                    tag="untagged",
                    text="fix this helper",
                ),
                _MOD.TodoRow(
                    path="crates/atm/src/example.rs",
                    line=3,
                    tag="untagged",
                    text="<empty>",
                ),
            ],
        )

    def test_main_prints_file_line_tag_text_rows(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            crate_dir = root / "crates" / "atm" / "src"
            crate_dir.mkdir(parents=True)
            source = crate_dir / "example.rs"
            source.write_text("// TODO(fix): remove stale wording\n", encoding="utf-8")
            stdout = io.StringIO()
            with patch("sys.stdout", stdout):
                rc = _MOD.main([str(root)])

        self.assertEqual(rc, 0)
        self.assertEqual(
            stdout.getvalue(),
            "crates/atm/src/example.rs:1:fix:remove stale wording\n",
        )

    def test_ignores_non_target_directories(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ignored = root / "target"
            ignored.mkdir()
            (ignored / "generated.rs").write_text("TODO(skip): hidden\n", encoding="utf-8")

            rows = _MOD.extract_rows(root)

        self.assertEqual(rows, [])


if __name__ == "__main__":
    unittest.main()
