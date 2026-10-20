from __future__ import annotations

from pathlib import Path
import sys
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_boundaries import BoundaryRecord
from view_boundaries import boundary_doc_rows
from view_boundaries import render_summary_text


class ViewBoundariesTests(unittest.TestCase):
    def test_boundary_doc_rows_aggregates_by_doc(self) -> None:
        repo_root = Path("/tmp/repo")
        records = [
            BoundaryRecord(
                boundary_id="BOUNDARY-A",
                owner_package="atm-core",
                owner_crate_path="atm_core",
                name="Alpha",
                public_trait="MailStore",
                public_facade=None,
                implementation_type="TypeA",
                implementation_module="atm_core::module_a",
                implementation_visibility="private",
                implementation_constructor="private",
                composition_roots=(),
                io_owns=(),
                io_forbidden=(),
                allowed_dependents=("atm",),
                allowed_dependencies=("atm-core",),
                allowed_dev_dependencies=(),
                forbidden_edges=("atm -> atm-rusqlite",),
                references_scope="outside_owner_crate",
                forbidden_references=("TypeA",),
                allowed_test_double_paths=("atm_core::tests::Double",),
                forbidden_test_bypasses=("rusqlite::Connection",),
                io_forbidden_source_modules=(),
                no_in_repo_implementation=False,
                lint_rules=("LINT_A",),
                review_gates=("no_public_impl",),
                status_state="planned",
                source_path=Path("docs/atm-core/boundaries.md"),
                start_line=1,
                raw={},
            ),
            BoundaryRecord(
                boundary_id="BOUNDARY-B",
                owner_package="atm-core",
                owner_crate_path="atm_core",
                name="Beta",
                public_trait="TaskStore",
                public_facade=None,
                implementation_type="TypeB",
                implementation_module="atm_core::module_b",
                implementation_visibility="private",
                implementation_constructor="private",
                composition_roots=(),
                io_owns=(),
                io_forbidden=(),
                allowed_dependents=("atm",),
                allowed_dependencies=("atm-core",),
                allowed_dev_dependencies=(),
                forbidden_edges=("atm -> atm-rusqlite",),
                references_scope="outside_owner_crate",
                forbidden_references=("TypeB",),
                allowed_test_double_paths=("atm_core::tests::Double",),
                forbidden_test_bypasses=("rusqlite::Connection",),
                io_forbidden_source_modules=(),
                no_in_repo_implementation=False,
                lint_rules=("LINT_B",),
                review_gates=("no_public_impl",),
                status_state="active",
                source_path=Path("docs/atm-core/boundaries.md"),
                start_line=10,
                raw={},
            ),
        ]

        rows = boundary_doc_rows(repo_root, records)

        self.assertEqual(
            rows,
            [
                {
                    "doc": "docs/atm-core/boundaries.md",
                    "records": "2",
                    "active": "1",
                    "planned": "1",
                    "deferred": "0",
                    "retired": "0",
                }
            ],
        )

    def test_boundary_doc_rows_counts_landed_states_as_active(self) -> None:
        # `just view boundaries` raised KeyError on stub_landed and
        # concrete_landed records, which boundaries/ uses.
        base = self._record("planned")
        records = [
            self._record(state)
            for state in ("stub_landed", "concrete_landed", "unix_implemented_windows_pending", "deferred", "retired")
        ] + [base]

        rows = boundary_doc_rows(Path("/tmp/repo"), records)

        self.assertEqual(
            rows,
            [
                {
                    "doc": "boundaries/atm-core/a.toml",
                    "records": "6",
                    "active": "3",
                    "planned": "1",
                    "deferred": "1",
                    "retired": "1",
                }
            ],
        )

    @staticmethod
    def _record(state: str) -> BoundaryRecord:
        return BoundaryRecord(
            boundary_id=f"BOUNDARY-{state}",
            owner_package="atm-core",
            owner_crate_path="atm_core",
            name=state,
            public_trait="MailStore",
            public_facade=None,
            implementation_type=None,
            implementation_module=None,
            implementation_visibility="trait_only",
            implementation_constructor="none",
            composition_roots=(),
            io_owns=(),
            io_forbidden=(),
            allowed_dependents=(),
            allowed_dependencies=(),
            allowed_dev_dependencies=(),
            forbidden_edges=(),
            references_scope="outside_owner_crate",
            forbidden_references=(),
            allowed_test_double_paths=(),
            forbidden_test_bypasses=(),
            io_forbidden_source_modules=(),
            no_in_repo_implementation=False,
            lint_rules=("LINT_A",),
            review_gates=("no_public_impl",),
            status_state=state,
            source_path=Path("boundaries/atm-core/a.toml"),
            start_line=1,
            raw={},
        )

    def test_render_summary_text_includes_counts(self) -> None:
        text = render_summary_text(
            Path("/tmp/repo"),
            [
                {
                    "doc": "docs/atm-core/boundaries.md",
                    "records": "2",
                    "active": "1",
                    "planned": "1",
                    "retired": "0",
                }
            ],
            0,
        )

        self.assertIn("docs analyzed: 1", text)
        self.assertIn("violations: 0", text)
        self.assertIn("docs/atm-core/boundaries.md", text)


if __name__ == "__main__":
    unittest.main()
