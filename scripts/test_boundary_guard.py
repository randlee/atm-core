from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts import check_boundary_guard as guard


RUNTIME_DOC = """
[dependencies]
allowed_dependents = ["atm", "atm-daemon", "atm-daemon-bootstrap"]
forbidden_edges = ["atm -> atm-rusqlite", "atm-daemon -> atm-rusqlite", "atm-runtime -> atm-daemon"]

[implementation]
visibility = "public"
constructor = "public"

[references]
forbidden = []

[testing]
forbidden_test_bypasses = []
"""

SQLITE_DOC = """
[dependencies]
allowed_dependents = ["atm-runtime", "atm-daemon-bootstrap", "atm-runtime-test-support"]
forbidden_edges = ["atm -> atm-rusqlite", "atm-daemon -> atm-rusqlite", "atm-graft -> atm-rusqlite"]

[implementation]
visibility = "private"
constructor = "private"

[references]
forbidden = ["rusqlite::Connection"]

[testing]
forbidden_test_bypasses = ["rusqlite::Connection"]
"""


class BoundaryGuardTests(unittest.TestCase):
    def _write_repo(self, root: Path) -> None:
        (root / "boundaries/atm-runtime").mkdir(parents=True, exist_ok=True)
        (root / "boundaries/atm-rusqlite").mkdir(parents=True, exist_ok=True)
        (root / "crates/atm-daemon/src").mkdir(parents=True, exist_ok=True)
        (root / "crates/atm-daemon").mkdir(parents=True, exist_ok=True)
        (root / "boundaries/atm-runtime/runtime-composition.toml").write_text(RUNTIME_DOC)
        for relative_path in guard.SQLITE_BOUNDARY_FILES:
            (root / relative_path).write_text(SQLITE_DOC)
        (root / "crates/atm-daemon/Cargo.toml").write_text(
            "[package]\nname='atm-daemon'\nversion='1.2.1'\n\n[dependencies]\natm-core={ path='../atm-core' }\n"
        )
        (root / "crates/atm-daemon/src/lib.rs").write_text("pub fn daemon() {}\n")

    def test_compare_boundary_policy_detects_relaxations(self) -> None:
        base_doc = {
            "dependencies": {
                "allowed_dependents": ["atm-runtime"],
                "forbidden_edges": ["atm-daemon -> atm-rusqlite", "atm -> atm-rusqlite"],
            },
            "implementation": {"visibility": "private", "constructor": "private"},
            "testing": {"forbidden_test_bypasses": ["rusqlite::Connection"]},
            "references": {"forbidden": ["SharedDb"]},
        }
        current_doc = {
            "dependencies": {
                "allowed_dependents": ["atm-runtime", "atm-daemon"],
                "forbidden_edges": ["atm -> atm-rusqlite"],
            },
            "implementation": {"visibility": "public", "constructor": "public"},
            "testing": {"forbidden_test_bypasses": []},
            "references": {"forbidden": []},
        }
        relaxations = guard.compare_boundary_policy(
            Path("boundaries/atm-rusqlite/sqlite-boundary-assembly.toml"),
            base_doc,
            current_doc,
        )
        fields = {item["field"] for item in relaxations}
        self.assertEqual(
            fields,
            {
                "allowed_dependents",
                "forbidden_edges",
                "visibility",
                "constructor",
                "forbidden_test_bypasses",
                "forbidden",
            },
        )

    def test_required_policy_rejects_lingering_daemon_allowlist(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            self._write_repo(root)
            sqlite_path = root / guard.SQLITE_BOUNDARY_FILES[0]
            sqlite_path.write_text(SQLITE_DOC.replace('"atm-runtime", ', '"atm-daemon", "atm-runtime", ', 1))
            violations = guard.check_required_boundary_policy(root)
            self.assertTrue(any("allowed dependent" in item["detail"] for item in violations))

    def test_required_policy_rejects_missing_forbidden_edge(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            self._write_repo(root)
            runtime_path = root / guard.RUNTIME_COMPOSITION
            runtime_path.write_text(RUNTIME_DOC.replace('"atm-daemon -> atm-rusqlite", ', "", 1))
            violations = guard.check_required_boundary_policy(root)
            self.assertTrue(any("runtime-composition.toml" in item["detail"] for item in violations))

    def test_code_edge_rejects_manifest_and_source_reference(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            self._write_repo(root)
            (root / "crates/atm-daemon/Cargo.toml").write_text(
                "[package]\nname='atm-daemon'\nversion='1.2.1'\n\n[dependencies]\natm-rusqlite={ path='../atm-rusqlite' }\n"
            )
            (root / "crates/atm-daemon/src/lib.rs").write_text("use atm_rusqlite::SqliteBoundaryAssembly;\n")
            violations = guard.check_forbidden_code_edge(root)
            self.assertTrue(any("Cargo.toml" in item["detail"] for item in violations))
            self.assertTrue(any("source" in item["detail"] for item in violations))


if __name__ == "__main__":
    unittest.main()
