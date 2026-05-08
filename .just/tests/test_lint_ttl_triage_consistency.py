from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import textwrap
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_ttl_triage_consistency import collect_ttl_triage_violations


def write_ttl(path: Path, body: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        textwrap.dedent(
            f"""\
            @prefix triage: <urn:atm:triage:> .

            {body}
            """
        ),
        encoding="utf-8",
    )


class LintTtlTriageConsistencyTests(unittest.TestCase):
    def test_flags_status_aggregate_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            write_ttl(
                repo_root / ".triage/phase-R/findings/F001.ttl",
                """
                triage:F001
                  a triage:Finding ;
                  triage:status "fixed" ;
                  triage:findingAggregate "open" .
                """,
            )

            violations = collect_ttl_triage_violations(repo_root)

            self.assertEqual(len(violations), 1)
            self.assertIn("status=fixed but findingAggregate=open", violations[0].message)

    def test_flags_fixed_aggregate_with_open_branch_status(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            write_ttl(
                repo_root / ".triage/phase-R/findings/F002.ttl",
                """
                triage:F002
                  a triage:Finding ;
                  triage:status "fixed" ;
                  triage:findingAggregate "fixed" ;
                  triage:sweepResults (
                    [
                      triage:branch "R.17" ;
                      triage:branchStatus "open"
                    ]
                  ) .
                """,
            )

            violations = collect_ttl_triage_violations(repo_root)

            self.assertEqual(len(violations), 1)
            self.assertIn("aggregate=fixed but branch status remains open", violations[0].message)

    def test_passes_consistent_fixed_record(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            write_ttl(
                repo_root / ".triage/phase-R/findings/F003.ttl",
                """
                triage:F003
                  a triage:Finding ;
                  triage:status "fixed" ;
                  triage:findingAggregate "fixed" ;
                  triage:sweepResults (
                    [
                      triage:branch "R.17" ;
                      triage:branchStatus "fixed"
                    ]
                  ) .
                """,
            )

            self.assertEqual(collect_ttl_triage_violations(repo_root), [])


if __name__ == "__main__":
    unittest.main()
