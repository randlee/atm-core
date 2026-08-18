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

    def test_flags_flat_branch_status_predicate_form(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            write_ttl(
                repo_root / ".triage/phase-R/findings/F004.ttl",
                """
                triage:F004
                  a triage:Finding ;
                  triage:status "fixed" ;
                  triage:findingAggregate "fixed" ;
                  triage:branchR17Status "open" .
                """,
            )

            violations = collect_ttl_triage_violations(repo_root)

            self.assertEqual(len(violations), 1)
            self.assertIn("aggregate=fixed but branch status remains open", violations[0].message)

    def test_flags_legacy_dash_sprint_key_with_actionable_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            write_ttl(
                repo_root / ".triage/phase-an/findings/F005.ttl",
                """
                triage:F005
                  a triage:Finding ;
                  triage:status "open" ;
                  triage:foundIn triage:AN-S1 .
                """,
            )

            violations = collect_ttl_triage_violations(repo_root)

            self.assertEqual(len(violations), 1)
            self.assertIn("TTL.QA_RUN_KEY_MISMATCH", violations[0].message)
            self.assertIn("canonical candidate=AN.1", violations[0].message)

    def test_flags_case_only_sprint_key_before_persistence(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            write_ttl(
                repo_root / ".triage/phase-an/findings/F006.ttl",
                """
                triage:F006
                  a triage:Finding ;
                  triage:status "open" ;
                  triage:aich_sprint "an.8" .
                """,
            )

            violations = collect_ttl_triage_violations(repo_root)

            self.assertEqual(len(violations), 1)
            self.assertIn("NAMING.NON_CANONICAL", violations[0].message)
            self.assertIn("canonical candidate=AN.8", violations[0].message)

    def test_accepts_canonical_sprint_fields(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            write_ttl(
                repo_root / ".triage/phase-an/findings/F007.ttl",
                """
                triage:F007
                  a triage:Finding ;
                  triage:status "open" ;
                  triage:foundIn triage:AN.8 ;
                  triage:aich_sprint "AN.8" .
                """,
            )

            self.assertEqual(collect_ttl_triage_violations(repo_root), [])

    def test_historical_allowlist_suppresses_only_listed_path_and_value(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / ".just").mkdir()
            (repo_root / ".just/ttl-naming-legacy-allowlist.txt").write_text(
                ".triage/phase-an/findings/F008.ttl\tAN-S1\n", encoding="utf-8"
            )
            write_ttl(
                repo_root / ".triage/phase-an/findings/F008.ttl",
                """
                triage:F008
                  a triage:Finding ;
                  triage:status "open" ;
                  triage:foundIn triage:AN-S1 .
                """,
            )
            write_ttl(
                repo_root / ".triage/phase-an/findings/F009.ttl",
                """
                triage:F009
                  a triage:Finding ;
                  triage:status "open" ;
                  triage:foundIn triage:AN-S2 .
                """,
            )

            violations = collect_ttl_triage_violations(repo_root)

            self.assertEqual(len(violations), 1)
            self.assertIn("F009.ttl", violations[0].path)

    def test_flags_unknown_sprint_format_in_explicit_sprint_field(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            write_ttl(
                repo_root / ".triage/phase-an/findings/F010.ttl",
                """
                triage:F010
                  a triage:Finding ;
                  triage:status "open" ;
                  triage:aich_sprint "Phase AN / Sprint 1" .
                """,
            )

            violations = collect_ttl_triage_violations(repo_root)

            self.assertEqual(len(violations), 1)
            self.assertIn("NAMING.UNKNOWN_SPRINT_FORMAT", violations[0].message)


if __name__ == "__main__":
    unittest.main()
