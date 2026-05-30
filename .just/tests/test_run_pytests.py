from __future__ import annotations

from io import StringIO
from pathlib import Path
import sys
import tempfile
import textwrap
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from run_pytests import build_suite
from run_pytests import count_fixtures
from run_pytests import print_fixture_summary


class RunPytestsTests(unittest.TestCase):
    def write_test_file(self, root: Path, name: str, body: str) -> None:
        tests_dir = root / ".just" / "tests"
        tests_dir.mkdir(parents=True, exist_ok=True)
        (tests_dir / name).write_text(body, encoding="utf-8")

    def test_count_fixtures_reports_testcase_counts(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_test_file(
                repo_root,
                "test_sample.py",
                textwrap.dedent(
                    """\
                    import unittest

                    class AlphaTests(unittest.TestCase):
                        def test_one(self):
                            pass

                        def test_two(self):
                            pass

                    class BetaTests(unittest.TestCase):
                        def test_three(self):
                            pass
                    """
                ),
            )

            suite = build_suite(repo_root)
            self.assertEqual(
                count_fixtures(suite),
                [
                    ("test_sample.AlphaTests", 2),
                    ("test_sample.BetaTests", 1),
                ],
            )

    def test_print_fixture_summary_includes_total(self) -> None:
        buffer = StringIO()
        with mock.patch("sys.stdout", buffer):
            print_fixture_summary([("test_alpha.AlphaTests", 2), ("test_beta.BetaTests", 1)])

        self.assertEqual(
            buffer.getvalue(),
            "pytests fixture counts:\n"
            "  test_alpha.AlphaTests: 2\n"
            "  test_beta.BetaTests: 1\n"
            "pytests fixtures total: 2\n",
        )


if __name__ == "__main__":
    unittest.main()
